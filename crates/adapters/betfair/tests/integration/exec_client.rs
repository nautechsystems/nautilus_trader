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

use std::{
    cell::RefCell,
    collections::{HashMap, HashSet, VecDeque},
    net::SocketAddr,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use nautilus_betfair::{
    common::{
        consts::{
            BETFAIR_CLIENT_ID, BETFAIR_VENUE, METHOD_CANCEL_ORDERS, METHOD_GET_ACCOUNT_FUNDS,
            METHOD_LIST_CURRENT_ORDERS, METHOD_LIST_EVENTS, METHOD_LIST_MARKET_CATALOGUE,
            METHOD_PLACE_ORDERS, METHOD_REPLACE_ORDERS,
        },
        parse::{make_customer_order_ref, make_customer_order_ref_legacy, parse_betfair_timestamp},
    },
    config::BetfairExecutionClientConfig,
    execution::BetfairExecutionClient,
    http::parse::parse_current_order_report,
    stream::config::BetfairStreamConfig,
};
use nautilus_common::{
    cache::Cache,
    clients::ExecutionClient,
    live::runner::{replace_system_event_sender, set_data_event_sender, set_exec_event_sender},
    messages::{
        DataEvent, ExecutionEvent,
        execution::{
            ExecutionReport,
            cancel::{BatchCancelOrders, CancelAllOrders, CancelOrder},
            modify::ModifyOrder,
            query::QueryOrder,
            report::{
                GenerateFillReports, GenerateFillReportsBuilder, GenerateOrderStatusReportsBuilder,
            },
            submit::{SubmitOrder, SubmitOrderList},
        },
        system::SocketState,
    },
    testing::wait_until_async,
};
use nautilus_core::{UUID4, UnixNanos, time::get_atomic_clock_realtime};
use nautilus_live::{ExecutionClientCore, SocketReconnectRegistry, SocketReconnectRequestOutcome};
use nautilus_model::{
    data::Data,
    enums::{AccountType, OmsType, OrderSide, OrderStatus, OrderType, TimeInForce},
    events::{OrderAccepted, OrderDeniedReason, OrderEventAny, OrderUpdated},
    identifiers::{
        AccountId, ClientId, ClientOrderId, InstrumentId, OrderListId, StrategyId, TraderId,
        VenueOrderId,
    },
    orders::{Order, OrderAny, OrderList, builder::OrderTestBuilder},
    reports::OrderStatusReport,
    types::{Currency, Price, Quantity},
};
use rstest::rstest;
use rust_decimal::Decimal;
use serde_json::Value;
use tokio::io::AsyncWriteExt;
use ustr::Ustr;

use crate::common::*;

fn create_test_execution_client_with_config(
    addr: SocketAddr,
    stream_port: u16,
    config: BetfairExecutionClientConfig,
) -> (
    BetfairExecutionClient,
    tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
    tokio::sync::mpsc::UnboundedReceiver<DataEvent>,
    Rc<RefCell<Cache>>,
) {
    create_test_execution_client_with_configs(addr, plain_stream_config(stream_port), config)
}

fn create_test_execution_client_with_configs(
    addr: SocketAddr,
    stream_config: BetfairStreamConfig,
    config: BetfairExecutionClientConfig,
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
        stream_config,
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
    create_test_execution_client_with_config(
        addr,
        stream_port,
        BetfairExecutionClientConfig::default(),
    )
}

async fn connect_execution_ready(client: &mut BetfairExecutionClient) {
    client.connect().await.unwrap();
    wait_for_connection_state(client, true).await;
    assert!(client.is_connected());
}

async fn wait_for_connection_state(client: &BetfairExecutionClient, expected: bool) {
    // `timeout_at` polls the inner future first, so reject a state observed after its deadline.
    let deadline = tokio::time::Instant::now() + PHASE_TIMEOUT;
    tokio::time::timeout_at(
        deadline,
        wait_until_async(
            || async {
                client.is_connected() == expected && tokio::time::Instant::now() <= deadline
            },
            PHASE_TIMEOUT,
        ),
    )
    .await
    .unwrap_or_else(|_| {
        panic!(
            "deadline elapsed waiting for connection state {expected}; state after deadline: {}",
            client.is_connected()
        )
    });
}

async fn wait_for_reconciliation_state(client: &BetfairExecutionClient, expected: bool) {
    tokio::time::timeout(
        PHASE_TIMEOUT,
        client.wait_for_reconciliation_state(expected),
    )
    .await
    .unwrap_or_else(|_| {
        panic!(
            "deadline elapsed waiting for reconciliation state {expected}; state after deadline: {}",
            client.is_reconciling()
        )
    });
}

#[tokio::test]
async fn test_mock_state_notifier_contract() {
    assert_mock_state_notifier_contract().await;
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
    assert!(
        !client.provides_bulk_position_coverage(InstrumentId::from("1.234567-12345-0.0.BETFAIR"))
    );
    assert!(!client.is_connected());
}

#[rstest]
#[tokio::test]
async fn test_exec_client_connect_disconnect() {
    let (addr, state) = start_mock_http().await;
    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, _rx, _data_rx, _cache) = create_test_execution_client(addr, stream_port);
    let (subscription_received_tx, subscription_received) = tokio::sync::watch::channel(false);

    let (server_done_tx, server_done_rx) = tokio::sync::oneshot::channel();

    let server = tokio::spawn(async move {
        let (mut reader, mut write_half) = accept_and_auth(&listener).await;

        // Capture the order subscription sent after the initial auth handshake
        let mut line = String::new();
        tokio::io::AsyncBufReadExt::read_line(&mut reader, &mut line)
            .await
            .unwrap();

        let json: Value = serde_json::from_str(line.trim()).unwrap();
        assert_eq!(json["op"], "orderSubscription");
        subscription_received_tx.send_replace(true);
        let id = json["id"].as_u64().unwrap();
        write_half
            .write_all(
                format!(
                    "{{\"op\":\"status\",\"id\":{id},\"statusCode\":\"SUCCESS\",\"connectionClosed\":false}}\r\n\
                     {{\"op\":\"ocm\",\"id\":{id},\"pt\":1000,\"ct\":\"SUB_IMAGE\",\"oc\":[]}}\r\n",
                )
                .as_bytes(),
            )
            .await
            .unwrap();

        let _ = server_done_rx.await;
        drop(write_half);
    });

    client.connect().await.unwrap();

    wait_for_connection_state(&client, true).await;

    assert!(client.is_connected());
    assert!(state.login_count.load(std::sync::atomic::Ordering::Relaxed) > 0);

    wait_for_watch(
        subscription_received,
        "stream subscription receipt",
        |received| *received,
    )
    .await;

    client.disconnect().await.unwrap();
    assert!(!client.is_connected());

    let _ = server_done_tx.send(());
    server.await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_exec_client_failed_startup_rolls_back_session_before_retry() {
    let (addr, state) = start_mock_http().await;
    let (stream_port, listener) = start_mock_stream().await;
    state.accounts_error_overrides.lock().insert(
        METHOD_GET_ACCOUNT_FUNDS.to_string(),
        load_json_fixture("rest/account_jsonrpc_error_invalid_input_live.json"),
    );
    let (mut client, _rx, _data_rx, _cache) = create_test_execution_client(addr, stream_port);

    let first_result = client.connect().await;
    assert!(first_result.is_err());

    state.accounts_error_overrides.lock().clear();
    let (server_done_tx, server_done_rx) = tokio::sync::oneshot::channel();

    let server = tokio::spawn(async move {
        let (mut reader, mut write_half) = accept_and_auth(&listener).await;
        let mut line = String::new();
        tokio::io::AsyncBufReadExt::read_line(&mut reader, &mut line)
            .await
            .unwrap();
        let message: Value = serde_json::from_str(line.trim()).unwrap();
        let id = message["id"].as_u64().unwrap();
        write_half
            .write_all(
                format!(
                    "{{\"op\":\"status\",\"id\":{id},\"statusCode\":\"SUCCESS\",\"connectionClosed\":false}}\r\n\
                     {{\"op\":\"ocm\",\"id\":{id},\"pt\":1000,\"ct\":\"SUB_IMAGE\",\"oc\":[]}}\r\n",
                )
                .as_bytes(),
            )
            .await
            .unwrap();
        let _ = server_done_rx.await;
    });

    client.connect().await.unwrap();
    wait_for_connection_state(&client, true).await;
    client.disconnect().await.unwrap();
    let _ = server_done_tx.send(());
    server.await.unwrap();

    assert_eq!(state.login_count.load(Ordering::Relaxed), 2);
    assert!(!client.is_connected());
}

#[rstest]
#[tokio::test]
async fn test_exec_client_canceled_startup_allows_immediate_retry() {
    let (addr, state) = start_mock_http().await;
    let (stream_port, listener) = start_mock_stream().await;
    let response_gate = MockResponseGate {
        method: METHOD_GET_ACCOUNT_FUNDS.to_string(),
        waiters: Arc::new(AtomicUsize::new(0)),
        semaphore: Arc::new(tokio::sync::Semaphore::new(0)),
    };
    *state.accounts_response_gate.lock() = Some(response_gate.clone());
    let (mut client, _rx, _data_rx, _cache) = create_test_execution_client(addr, stream_port);
    let (server_done_tx, server_done_rx) = tokio::sync::oneshot::channel();

    let server = tokio::spawn(async move {
        let (mut reader, mut write_half) = accept_and_auth(&listener).await;
        let mut line = String::new();
        tokio::io::AsyncBufReadExt::read_line(&mut reader, &mut line)
            .await
            .unwrap();
        let message: Value = serde_json::from_str(line.trim()).unwrap();
        let id = message["id"].as_u64().unwrap();
        write_half
            .write_all(
                format!(
                    "{{\"op\":\"status\",\"id\":{id},\"statusCode\":\"SUCCESS\",\"connectionClosed\":false}}\r\n\
                     {{\"op\":\"ocm\",\"id\":{id},\"pt\":1000,\"ct\":\"SUB_IMAGE\",\"oc\":[]}}\r\n",
                )
                .as_bytes(),
            )
            .await
            .unwrap();
        let _ = server_done_rx.await;
    });

    {
        let connect = client.connect();
        tokio::pin!(connect);
        tokio::select! {
            biased;
            result = tokio::time::timeout(Duration::from_secs(2), async {
                while response_gate.waiters.load(Ordering::Relaxed) == 0 {
                    tokio::task::yield_now().await;
                }
            }) => result.expect("account request should reach the response gate"),
            result = &mut connect => panic!("first connect completed before cancellation: {result:?}"),
        }
    }
    *state.accounts_response_gate.lock() = None;
    response_gate.semaphore.add_permits(1);

    client.connect().await.unwrap();
    wait_for_connection_state(&client, true).await;
    client.disconnect().await.unwrap();
    let _ = server_done_tx.send(());
    server.await.unwrap();

    assert_eq!(state.login_count.load(Ordering::Relaxed), 2);
    assert!(!client.is_connected());
}

#[rstest]
#[tokio::test]
async fn test_exec_client_publishes_socket_state_and_registers_reconnect() {
    const ENDPOINT: &str = "betfair-user-streams";

    let (system_tx, mut system_rx) = tokio::sync::mpsc::unbounded_channel();
    replace_system_event_sender(system_tx);

    let (addr, state) = start_mock_http().await;
    let response: Value =
        serde_json::from_str(&load_fixture("rest/list_current_orders_empty.json")).unwrap();
    state.betting_overrides.lock().insert(
        METHOD_LIST_CURRENT_ORDERS.to_string(),
        response["result"].clone(),
    );
    let (stream_port, listener) = start_mock_stream().await;
    let registry = SocketReconnectRegistry::default();
    let (mut client, _rx, _data_rx, _cache) =
        registry.scope(|| create_test_execution_client(addr, stream_port));
    let endpoint = Ustr::from(ENDPOINT);
    assert!(registry.handle(*BETFAIR_CLIENT_ID, endpoint).is_none());
    let (initial_tx, initial_rx) = tokio::sync::oneshot::channel();
    let (replacement_tx, replacement_rx) = tokio::sync::oneshot::channel();
    let (activate_tx, activate_rx) = tokio::sync::oneshot::channel();
    let (activated_tx, activated_rx) = tokio::sync::oneshot::channel();
    let (server_done_tx, server_done_rx) = tokio::sync::oneshot::channel();

    let server = tokio::spawn(async move {
        let (mut initial_reader, _initial_write_half, auth) =
            accept_and_capture_auth(&listener).await;
        let mut subscription = String::new();
        tokio::io::AsyncBufReadExt::read_line(&mut initial_reader, &mut subscription)
            .await
            .unwrap();
        let _ = initial_tx.send((auth, subscription));

        let (socket, _) = tokio::time::timeout(Duration::from_secs(5), listener.accept())
            .await
            .expect("controller reconnect must open a replacement execution socket")
            .unwrap();
        let (read_half, mut replacement_write_half) = socket.into_split();
        let mut replacement_reader = tokio::io::BufReader::new(read_half);
        let mut auth = String::new();
        tokio::io::AsyncBufReadExt::read_line(&mut replacement_reader, &mut auth)
            .await
            .unwrap();
        let mut subscription = String::new();
        tokio::io::AsyncBufReadExt::read_line(&mut replacement_reader, &mut subscription)
            .await
            .unwrap();
        let _ = replacement_tx.send((auth, subscription));

        let _ = activate_rx.await;
        tokio::io::AsyncWriteExt::write_all(
            &mut replacement_write_half,
            b"{\"op\":\"connection\",\"connectionId\":\"replacement\"}\r\n",
        )
        .await
        .unwrap();
        let _ = activated_tx.send(());

        let _ = server_done_rx.await;
    });

    client.connect().await.unwrap();

    let connected = next_socket_state(&mut system_rx).await;
    assert_eq!(connected.client_id, *BETFAIR_CLIENT_ID);
    assert_eq!(connected.venue, Some(*BETFAIR_VENUE));
    assert_eq!(connected.endpoint, endpoint);
    assert_eq!(connected.state, SocketState::Connected);

    let (initial_auth, initial_subscription) = initial_rx.await.unwrap();
    let initial_auth: Value = serde_json::from_str(initial_auth.trim()).unwrap();
    let initial_subscription: Value = serde_json::from_str(initial_subscription.trim()).unwrap();
    assert_eq!(initial_auth["op"], "authentication");
    assert_eq!(initial_auth["session"], "SESSION_TOKEN");
    assert_eq!(initial_subscription["op"], "orderSubscription");

    let reconnect = registry
        .handle(*BETFAIR_CLIENT_ID, endpoint)
        .expect("active execution socket must register reconnect control");
    assert_eq!(
        reconnect.request_reconnect(),
        SocketReconnectRequestOutcome::Accepted,
    );
    assert_eq!(
        reconnect.request_reconnect(),
        SocketReconnectRequestOutcome::AlreadyReconnecting,
    );

    let lost = next_socket_state(&mut system_rx).await;
    assert_eq!(lost.client_id, *BETFAIR_CLIENT_ID);
    assert_eq!(lost.venue, Some(*BETFAIR_VENUE));
    assert_eq!(lost.endpoint, endpoint);
    assert_eq!(lost.state, SocketState::Disconnected);

    let recovered = next_socket_state(&mut system_rx).await;
    assert_eq!(recovered.client_id, *BETFAIR_CLIENT_ID);
    assert_eq!(recovered.venue, Some(*BETFAIR_VENUE));
    assert_eq!(recovered.endpoint, endpoint);
    assert_eq!(recovered.state, SocketState::Connected);
    assert!(client.is_reconciling());

    let (replacement_auth, replacement_subscription) = replacement_rx.await.unwrap();
    let replacement_auth: Value = serde_json::from_str(replacement_auth.trim()).unwrap();
    let replacement_subscription: Value =
        serde_json::from_str(replacement_subscription.trim()).unwrap();
    assert_eq!(replacement_auth, initial_auth);
    assert_eq!(replacement_subscription, initial_subscription);

    let _ = activate_tx.send(());
    activated_rx.await.unwrap();

    client.disconnect().await.unwrap();
    assert!(registry.handle(*BETFAIR_CLIENT_ID, endpoint).is_none());
    assert_eq!(
        reconnect.request_reconnect(),
        SocketReconnectRequestOutcome::Closed,
    );

    let _ = server_done_tx.send(());
    server.await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_exec_client_connect_emits_account_state() {
    let (addr, _state) = start_mock_http().await;
    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, _cache) = create_test_execution_client(addr, stream_port);

    let (server_done_tx, server_done_rx) = tokio::sync::oneshot::channel();

    let server = tokio::spawn(async move {
        let (_reader, write_half) = accept_and_activate(&listener).await;
        let _ = server_done_rx.await;
        drop(write_half);
    });

    connect_execution_ready(&mut client).await;

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
    let _ = server_done_tx.send(());
    server.await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_ocm_handler_emits_order_status_report() {
    let (addr, _state) = start_mock_http().await;
    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, _cache) = create_test_execution_client(addr, stream_port);

    let ocm_fixture = load_fixture("stream/ocm_FILLED.json");

    let (server_done_tx, server_done_rx) = tokio::sync::oneshot::channel();

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

        let _ = server_done_rx.await;
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
    let _ = server_done_tx.send(());
    server.await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_ocm_voided_order_emits_data_event() {
    let (addr, _state) = start_mock_http().await;
    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, _rx, mut data_rx, _cache) = create_test_execution_client(addr, stream_port);

    let ocm_fixture = load_fixture("stream/ocm_VOIDED.json");

    let (server_done_tx, server_done_rx) = tokio::sync::oneshot::channel();

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
        let _ = server_done_rx.await;
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
    let _ = server_done_tx.send(());
    server.await.unwrap();
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
        .insert(METHOD_CANCEL_ORDERS.to_string(), v["result"].clone());

    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, cache) = create_test_execution_client(addr, stream_port);

    let (server_done_tx, server_done_rx) = tokio::sync::oneshot::channel();

    let server = tokio::spawn(async move {
        let (_reader, write_half) = accept_and_activate(&listener).await;
        let _ = server_done_rx.await;
        drop(write_half);
    });

    connect_execution_ready(&mut client).await;

    while rx.try_recv().is_ok() {}

    let order = make_accepted_test_order("1.179082386-235-0.BETFAIR", "O-001", "1", "2.58", "10");
    add_order_to_cache(&cache, order);

    let cmd = make_cancel_order("1.179082386-235-0.BETFAIR", "O-001", "1");
    client.cancel_order(cmd).unwrap();

    wait_for_mock_state(&state, "METHOD_CANCEL_ORDERS request count >= 1", |state| {
        betting_method_count(state, METHOD_CANCEL_ORDERS) >= 1
    })
    .await;

    let mut rejected_seen = false;

    while let Ok(Some(event)) = tokio::time::timeout(Duration::from_millis(500), rx.recv()).await {
        if matches!(
            event,
            ExecutionEvent::Order(OrderEventAny::CancelRejected(_))
        ) {
            rejected_seen = true;
            break;
        }
    }
    assert!(
        !rejected_seen,
        "BetTakenOrLapsed should not emit cancel rejected"
    );

    client.disconnect().await.unwrap();
    let _ = server_done_tx.send(());
    server.await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_cancel_order_sp_bet_taken_or_lapsed_emits_rejected() {
    let (addr, state) = start_mock_http().await;

    let fixture = load_fixture("rest/betting_cancel_orders_bet_taken_or_lapsed.json");
    let v: Value = serde_json::from_str(&fixture).unwrap();
    state
        .betting_overrides
        .lock()
        .insert(METHOD_CANCEL_ORDERS.to_string(), v["result"].clone());

    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, cache) = create_test_execution_client(addr, stream_port);

    let (server_done_tx, server_done_rx) = tokio::sync::oneshot::channel();

    let server = tokio::spawn(async move {
        let (_reader, write_half) = accept_and_activate(&listener).await;
        let _ = server_done_rx.await;
        drop(write_half);
    });

    connect_execution_ready(&mut client).await;

    while rx.try_recv().is_ok() {}

    let order = make_accepted_sp_test_order("1.179082386-235-0.BETFAIR", "O-SP-001", "1", "2");
    add_order_to_cache(&cache, order);

    let cmd = make_cancel_order("1.179082386-235-0.BETFAIR", "O-SP-001", "1");
    client.cancel_order(cmd).unwrap();

    wait_for_mock_state(&state, "METHOD_CANCEL_ORDERS request count >= 1", |state| {
        betting_method_count(state, METHOD_CANCEL_ORDERS) >= 1
    })
    .await;

    let mut rejected_seen = false;

    while let Ok(Some(event)) = tokio::time::timeout(Duration::from_millis(500), rx.recv()).await {
        if matches!(
            event,
            ExecutionEvent::Order(OrderEventAny::CancelRejected(_))
        ) {
            rejected_seen = true;
            break;
        }
    }

    assert!(
        rejected_seen,
        "BetTakenOrLapsed on an SP bet must emit cancel rejected"
    );

    client.disconnect().await.unwrap();
    let _ = server_done_tx.send(());
    server.await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_cancel_order_instruction_failure_emits_rejected() {
    let (addr, state) = start_mock_http().await;

    let fixture = load_fixture("rest/betting_cancel_orders_error.json");
    let v: Value = serde_json::from_str(&fixture).unwrap();
    state
        .betting_overrides
        .lock()
        .insert(METHOD_CANCEL_ORDERS.to_string(), v["result"].clone());

    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, _cache) = create_test_execution_client(addr, stream_port);

    let (server_done_tx, server_done_rx) = tokio::sync::oneshot::channel();

    let server = tokio::spawn(async move {
        let (_reader, write_half) = accept_and_activate(&listener).await;
        let _ = server_done_rx.await;
        drop(write_half);
    });

    connect_execution_ready(&mut client).await;

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
            assert_eq!(rejected.reason, "ErrorInOrder");
        }
        other => panic!("Expected CancelRejected event, found: {other:?}"),
    }

    client.disconnect().await.unwrap();
    let _ = server_done_tx.send(());
    server.await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_cancel_order_definitive_result_failure_without_instructions_emits_rejected() {
    let (addr, state) = start_mock_http().await;

    let fixture = load_fixture("rest/betting_cancel_orders_result_failure.json");
    let v: Value = serde_json::from_str(&fixture).unwrap();
    state
        .betting_overrides
        .lock()
        .insert(METHOD_CANCEL_ORDERS.to_string(), v["result"].clone());

    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, _cache) = create_test_execution_client(addr, stream_port);

    let (server_done_tx, server_done_rx) = tokio::sync::oneshot::channel();

    let server = tokio::spawn(async move {
        let (_reader, write_half) = accept_and_activate(&listener).await;
        let _ = server_done_rx.await;
        drop(write_half);
    });

    connect_execution_ready(&mut client).await;

    while rx.try_recv().is_ok() {}

    let cmd = make_cancel_order("1.179082386-235-0.BETFAIR", "O-003", "1");
    client.cancel_order(cmd).unwrap();

    wait_for_mock_state(&state, "METHOD_CANCEL_ORDERS request count >= 1", |state| {
        betting_method_count(state, METHOD_CANCEL_ORDERS) >= 1
    })
    .await;

    let mut rejected_count = 0;

    while let Ok(Some(event)) = tokio::time::timeout(Duration::from_millis(500), rx.recv()).await {
        if let ExecutionEvent::Order(OrderEventAny::CancelRejected(rejected)) = event {
            assert_eq!(rejected.client_order_id, ClientOrderId::from("O-003"));
            assert!(rejected.reason.contains("MarketSuspended"));
            rejected_count += 1;
        }
    }
    assert_eq!(
        rejected_count, 1,
        "definitive cancel failure must emit exactly one CancelRejected",
    );

    client.disconnect().await.unwrap();
    let _ = server_done_tx.send(());
    server.await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_cancel_order_ambiguous_5xx_emits_no_rejected() {
    let (addr, state) = start_mock_http().await;
    state
        .betting_status_overrides
        .lock()
        .insert(METHOD_CANCEL_ORDERS.to_string(), 502);

    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, _cache) = create_test_execution_client(addr, stream_port);

    let (server_done_tx, server_done_rx) = tokio::sync::oneshot::channel();

    let server = tokio::spawn(async move {
        let (_reader, write_half) = accept_and_activate(&listener).await;
        let _ = server_done_rx.await;
        drop(write_half);
    });

    connect_execution_ready(&mut client).await;

    while rx.try_recv().is_ok() {}

    let cmd = make_cancel_order("1.179082386-235-0.BETFAIR", "O-003-5XX", "1");
    client.cancel_order(cmd).unwrap();

    wait_for_mock_state(&state, "METHOD_CANCEL_ORDERS request count >= 1", |state| {
        betting_method_count(state, METHOD_CANCEL_ORDERS) >= 1
    })
    .await;

    tokio::time::sleep(Duration::from_secs(1)).await;

    let mut rejected_seen = false;

    while let Ok(Some(event)) = tokio::time::timeout(Duration::from_millis(500), rx.recv()).await {
        if let ExecutionEvent::Order(OrderEventAny::CancelRejected(rejected)) = event {
            assert_eq!(rejected.client_order_id, ClientOrderId::from("O-003-5XX"));
            rejected_seen = true;
            break;
        }
    }
    assert!(
        !rejected_seen,
        "ambiguous cancel 5xx must not emit CancelRejected",
    );

    client.disconnect().await.unwrap();
    let _ = server_done_tx.send(());
    server.await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_cancel_order_timeout_status_emits_no_rejected() {
    let (addr, state) = start_mock_http().await;

    let fixture = load_fixture("rest/betting_cancel_orders_success.json");
    let mut v: Value = serde_json::from_str(&fixture).unwrap();
    v["result"]["status"] = Value::String("TIMEOUT".to_string());
    state
        .betting_overrides
        .lock()
        .insert(METHOD_CANCEL_ORDERS.to_string(), v["result"].clone());

    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, _cache) = create_test_execution_client(addr, stream_port);

    let (server_done_tx, server_done_rx) = tokio::sync::oneshot::channel();

    let server = tokio::spawn(async move {
        let (_reader, write_half) = accept_and_activate(&listener).await;
        let _ = server_done_rx.await;
        drop(write_half);
    });

    connect_execution_ready(&mut client).await;

    while rx.try_recv().is_ok() {}

    let cmd = make_cancel_order("1.179082386-235-0.BETFAIR", "O-003-TIMEOUT", "1");
    client.cancel_order(cmd).unwrap();

    wait_for_mock_state(&state, "METHOD_CANCEL_ORDERS request count >= 1", |state| {
        betting_method_count(state, METHOD_CANCEL_ORDERS) >= 1
    })
    .await;

    let mut rejected_seen = false;

    while let Ok(Some(event)) = tokio::time::timeout(Duration::from_millis(500), rx.recv()).await {
        if let ExecutionEvent::Order(OrderEventAny::CancelRejected(rejected)) = event {
            assert_eq!(
                rejected.client_order_id,
                ClientOrderId::from("O-003-TIMEOUT")
            );
            rejected_seen = true;
            break;
        }
    }
    assert!(
        !rejected_seen,
        "cancel timeout status must not emit CancelRejected",
    );

    client.disconnect().await.unwrap();
    let _ = server_done_tx.send(());
    server.await.unwrap();
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
        .insert(METHOD_CANCEL_ORDERS.to_string(), v["result"].clone());

    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, _cache) = create_test_execution_client(addr, stream_port);

    let (server_done_tx, server_done_rx) = tokio::sync::oneshot::channel();

    let server = tokio::spawn(async move {
        let (_reader, write_half) = accept_and_activate(&listener).await;
        let _ = server_done_rx.await;
        drop(write_half);
    });

    connect_execution_ready(&mut client).await;

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
    let _ = server_done_tx.send(());
    server.await.unwrap();
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

fn make_accepted_sp_test_order(
    instrument_id: &str,
    client_order_id: &str,
    venue_order_id: &str,
    quantity: &str,
) -> OrderAny {
    let mut order = OrderTestBuilder::new(OrderType::Market)
        .trader_id(TraderId::from("TESTER-001"))
        .strategy_id(StrategyId::from("S-001"))
        .instrument_id(InstrumentId::from(instrument_id))
        .client_order_id(ClientOrderId::from(client_order_id))
        .side(OrderSide::Buy)
        .quantity(Quantity::from(quantity))
        .time_in_force(TimeInForce::AtTheClose)
        .build();
    order
        .apply(OrderEventAny::Accepted(OrderAccepted::new(
            TraderId::from("TESTER-001"),
            StrategyId::from("S-001"),
            InstrumentId::from(instrument_id),
            ClientOrderId::from(client_order_id),
            VenueOrderId::from(venue_order_id),
            AccountId::from("BETFAIR-001"),
            UUID4::new(),
            UnixNanos::default(),
            UnixNanos::default(),
            false,
        )))
        .unwrap();
    order
}

fn make_reduce_only_test_order(
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
        .reduce_only(true)
        .build()
}

fn make_accepted_test_order(
    instrument_id: &str,
    client_order_id: &str,
    venue_order_id: &str,
    price: &str,
    quantity: &str,
) -> OrderAny {
    let mut order = make_test_order(instrument_id, client_order_id, price, quantity);
    order
        .apply(OrderEventAny::Accepted(OrderAccepted::new(
            TraderId::from("TESTER-001"),
            StrategyId::from("S-001"),
            InstrumentId::from(instrument_id),
            ClientOrderId::from(client_order_id),
            VenueOrderId::from(venue_order_id),
            AccountId::from("BETFAIR-001"),
            UUID4::new(),
            UnixNanos::from(1),
            UnixNanos::from(1),
            false,
        )))
        .unwrap();
    order
}

fn make_accepted_test_order_for(
    instrument_id: &str,
    client_order_id: &str,
    venue_order_id: &str,
    strategy_id: &str,
    account_id: &str,
    side: OrderSide,
) -> OrderAny {
    let mut order = OrderTestBuilder::new(OrderType::Limit)
        .trader_id(TraderId::from("TESTER-001"))
        .strategy_id(StrategyId::from(strategy_id))
        .instrument_id(InstrumentId::from(instrument_id))
        .client_order_id(ClientOrderId::from(client_order_id))
        .side(side)
        .price(Price::from("2.50"))
        .quantity(Quantity::from("10"))
        .time_in_force(TimeInForce::Gtc)
        .build();
    order
        .apply(OrderEventAny::Accepted(OrderAccepted::new(
            TraderId::from("TESTER-001"),
            StrategyId::from(strategy_id),
            InstrumentId::from(instrument_id),
            ClientOrderId::from(client_order_id),
            VenueOrderId::from(venue_order_id),
            AccountId::from(account_id),
            UUID4::new(),
            UnixNanos::from(1),
            UnixNanos::from(1),
            false,
        )))
        .unwrap();
    order
}

fn assert_valid_customer_ref(params: &Value) -> &str {
    let customer_ref = params["customerRef"]
        .as_str()
        .expect("order request must include customerRef");
    assert_eq!(customer_ref.len(), 32);
    assert!(
        customer_ref
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
        "customerRef must be lowercase hexadecimal: {customer_ref}",
    );
    customer_ref
}

fn add_order_to_cache(cache: &Rc<RefCell<Cache>>, order: OrderAny) {
    add_order_to_cache_for_client(cache, order, Some(*BETFAIR_CLIENT_ID));
}

fn add_order_to_cache_for_client(
    cache: &Rc<RefCell<Cache>>,
    order: OrderAny,
    client_id: Option<ClientId>,
) {
    cache
        .borrow_mut()
        .add_order(order, None, client_id, false)
        .unwrap();
}

fn make_cancel_all_orders_cmd(
    instrument_id: &str,
    order_side: Option<OrderSide>,
) -> CancelAllOrders {
    CancelAllOrders::new(
        TraderId::from("TESTER-001"),
        Some(*BETFAIR_CLIENT_ID),
        StrategyId::from("S-001"),
        InstrumentId::from(instrument_id),
        order_side,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    )
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

fn make_price_modify_order_cmd(
    instrument_id: &str,
    client_order_id: &str,
    venue_order_id: &str,
    price: &str,
) -> ModifyOrder {
    make_modify_order_cmd(
        instrument_id,
        client_order_id,
        venue_order_id,
        None,
        Some(Price::from(price)),
    )
}

fn make_quantity_modify_order_cmd(
    instrument_id: &str,
    client_order_id: &str,
    venue_order_id: &str,
    quantity: &str,
) -> ModifyOrder {
    make_modify_order_cmd(
        instrument_id,
        client_order_id,
        venue_order_id,
        Some(Quantity::from(quantity)),
        None,
    )
}

fn make_modify_order_cmd(
    instrument_id: &str,
    client_order_id: &str,
    venue_order_id: &str,
    quantity: Option<Quantity>,
    price: Option<Price>,
) -> ModifyOrder {
    ModifyOrder::new(
        TraderId::from("TESTER-001"),
        Some(*BETFAIR_CLIENT_ID),
        StrategyId::from("S-001"),
        InstrumentId::from(instrument_id),
        ClientOrderId::from(client_order_id),
        Some(VenueOrderId::from(venue_order_id)),
        quantity,
        price,
        None,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None, // correlation_id
    )
}

async fn submit_and_await_accept(
    client: &BetfairExecutionClient,
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
    order: &OrderAny,
    venue_order_id: &str,
) {
    client.submit_order(make_submit_order_cmd(order)).unwrap();

    loop {
        match tokio::time::timeout(Duration::from_secs(5), rx.recv()).await {
            Ok(Some(ExecutionEvent::Order(OrderEventAny::Accepted(accepted)))) => {
                assert_eq!(accepted.client_order_id, order.client_order_id());
                assert_eq!(accepted.venue_order_id, VenueOrderId::from(venue_order_id));
                break;
            }
            Ok(Some(_)) => {}
            other => panic!("order was not accepted before modify: {other:?}"),
        }
    }
}

async fn drain_events(
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
    settle: Duration,
) -> Vec<ExecutionEvent> {
    let mut events = Vec::new();

    while let Ok(Some(event)) = tokio::time::timeout(settle, rx.recv()).await {
        events.push(event);
    }

    events
}

fn order_updates(events: &[ExecutionEvent]) -> Vec<&OrderUpdated> {
    events
        .iter()
        .filter_map(|event| match event {
            ExecutionEvent::Order(OrderEventAny::Updated(updated)) => Some(updated),
            _ => None,
        })
        .collect()
}

fn assert_no_accept_or_modify_reject(events: &[ExecutionEvent]) {
    assert!(
        !events.iter().any(|event| matches!(
            event,
            ExecutionEvent::Order(OrderEventAny::Accepted(_) | OrderEventAny::ModifyRejected(_))
        )),
        "startup-restored modify must not emit acceptance or rejection: {events:?}",
    );
}

#[rstest]
#[tokio::test]
async fn test_submit_order_success_emits_accepted() {
    let (addr, _state) = start_mock_http().await;
    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, cache) = create_test_execution_client(addr, stream_port);

    let (server_done_tx, server_done_rx) = tokio::sync::oneshot::channel();

    let server = tokio::spawn(async move {
        let (_reader, write_half) = accept_and_activate(&listener).await;
        let _ = server_done_rx.await;
        drop(write_half);
    });

    connect_execution_ready(&mut client).await;

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
    let _ = server_done_tx.send(());
    server.await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_submit_order_retry_reuses_customer_ref() {
    let (addr, state) = start_mock_http().await;
    state
        .betting_status_one_shot_overrides
        .lock()
        .insert(METHOD_PLACE_ORDERS.to_string(), 502);

    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, cache) = create_test_execution_client(addr, stream_port);
    let (server_done_tx, server_done_rx) = tokio::sync::oneshot::channel();

    let server = tokio::spawn(async move {
        let (_reader, write_half) = accept_and_activate(&listener).await;
        let _ = server_done_rx.await;
        drop(write_half);
    });

    connect_execution_ready(&mut client).await;

    while rx.try_recv().is_ok() {}

    let order = make_test_order(
        "1.181005744-86362-0.BETFAIR",
        "O-SUBMIT-RETRY",
        "2.58",
        "10",
    );
    add_order_to_cache(&cache, order.clone());
    client.submit_order(make_submit_order_cmd(&order)).unwrap();

    wait_for_mock_state(&state, "METHOD_PLACE_ORDERS request count == 2", |state| {
        betting_method_count(state, METHOD_PLACE_ORDERS) == 2
    })
    .await;

    let params: Vec<Value> = state
        .betting_request_params
        .lock()
        .iter()
        .filter(|(method, _)| method == METHOD_PLACE_ORDERS)
        .map(|(_, params)| params.clone())
        .collect();
    assert_eq!(params.len(), 2, "one 502 must cause exactly one retry");
    assert_eq!(params[0], params[1], "retry must reuse the request body");
    assert_eq!(
        assert_valid_customer_ref(&params[0]),
        assert_valid_customer_ref(&params[1]),
    );

    let mut submitted = 0;
    let mut accepted = 0;

    while let Ok(Some(event)) = tokio::time::timeout(Duration::from_millis(500), rx.recv()).await {
        match event {
            ExecutionEvent::Order(OrderEventAny::Submitted(_)) => submitted += 1,
            ExecutionEvent::Order(OrderEventAny::Accepted(_)) => accepted += 1,
            _ => {}
        }
    }
    assert_eq!(submitted, 1);
    assert_eq!(accepted, 1);

    client.disconnect().await.unwrap();
    let _ = server_done_tx.send(());
    server.await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_submit_retry_preserves_earlier_ambiguity_before_no_session() {
    let (addr, state) = start_mock_http().await;
    state
        .betting_status_one_shot_overrides
        .lock()
        .insert(METHOD_PLACE_ORDERS.to_string(), 502);
    state.betting_error_overrides.lock().insert(
        METHOD_PLACE_ORDERS.to_string(),
        betting_api_error("NO_SESSION"),
    );

    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, cache) = create_test_execution_client(addr, stream_port);
    let (server_done_tx, server_done_rx) = tokio::sync::oneshot::channel();

    let server = tokio::spawn(async move {
        let (_reader, write_half) = accept_and_activate(&listener).await;
        let _ = server_done_rx.await;
        drop(write_half);
    });

    connect_execution_ready(&mut client).await;

    while rx.try_recv().is_ok() {}

    let order = make_test_order(
        "1.181005744-86362-0.BETFAIR",
        "O-SUBMIT-AMBIG-NO-SESSION",
        "2.58",
        "10",
    );
    add_order_to_cache(&cache, order.clone());
    client.submit_order(make_submit_order_cmd(&order)).unwrap();

    wait_for_mock_state(&state, "METHOD_PLACE_ORDERS request count == 2", |state| {
        betting_method_count(state, METHOD_PLACE_ORDERS) == 2
    })
    .await;

    let mut submitted = 0;
    let mut rejected = 0;

    while let Ok(Some(event)) = tokio::time::timeout(Duration::from_millis(500), rx.recv()).await {
        match event {
            ExecutionEvent::Order(OrderEventAny::Submitted(_)) => submitted += 1,
            ExecutionEvent::Order(OrderEventAny::Rejected(_)) => rejected += 1,
            _ => {}
        }
    }
    assert_eq!(submitted, 1);
    assert_eq!(
        rejected, 0,
        "a later NO_SESSION cannot make an earlier 502 definitive",
    );

    let params: Vec<Value> = state
        .betting_request_params
        .lock()
        .iter()
        .filter(|(method, _)| method == METHOD_PLACE_ORDERS)
        .map(|(_, params)| params.clone())
        .collect();
    assert_eq!(params.len(), 2);
    assert_eq!(params[0], params[1]);

    client.disconnect().await.unwrap();
    let _ = server_done_tx.send(());
    server.await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_concurrent_submit_retry_state_is_request_local() {
    const ORDER_COUNT: usize = 8;

    let (addr, state) = start_mock_http().await;
    state
        .betting_status_one_shot_overrides
        .lock()
        .insert(METHOD_PLACE_ORDERS.to_string(), 502);

    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, cache) = create_test_execution_client(addr, stream_port);
    let (server_done_tx, server_done_rx) = tokio::sync::oneshot::channel();

    let server = tokio::spawn(async move {
        let (_reader, write_half) = accept_and_activate(&listener).await;
        let _ = server_done_rx.await;
        drop(write_half);
    });

    connect_execution_ready(&mut client).await;

    while rx.try_recv().is_ok() {}

    for index in 0..ORDER_COUNT {
        let order = make_test_order(
            "1.181005744-86362-0.BETFAIR",
            &format!("O-STRESS-{index}"),
            "2.58",
            "10",
        );
        add_order_to_cache(&cache, order.clone());
        client.submit_order(make_submit_order_cmd(&order)).unwrap();
    }

    wait_for_mock_state(
        &state,
        "METHOD_PLACE_ORDERS request count == ORDER_COUNT + 1",
        |state| betting_method_count(state, METHOD_PLACE_ORDERS) == ORDER_COUNT + 1,
    )
    .await;

    let params: Vec<Value> = state
        .betting_request_params
        .lock()
        .iter()
        .filter(|(method, _)| method == METHOD_PLACE_ORDERS)
        .map(|(_, params)| params.clone())
        .collect();
    let mut by_order_ref: HashMap<String, Vec<Value>> = HashMap::new();

    for params in params {
        let order_ref = params["instructions"][0]["customerOrderRef"]
            .as_str()
            .expect("place instruction must include customerOrderRef")
            .to_string();
        by_order_ref.entry(order_ref).or_default().push(params);
    }

    assert_eq!(by_order_ref.len(), ORDER_COUNT);
    assert_eq!(
        by_order_ref
            .values()
            .filter(|requests| requests.len() == 2)
            .count(),
        1,
        "one logical request must own the single retry",
    );
    let mut customer_refs = HashSet::new();

    for requests in by_order_ref.values() {
        let customer_ref = assert_valid_customer_ref(&requests[0]);
        assert!(
            customer_refs.insert(customer_ref.to_string()),
            "logical requests must have distinct customerRef values",
        );

        if requests.len() == 2 {
            assert_eq!(requests[0], requests[1]);
        } else {
            assert_eq!(requests.len(), 1);
        }
    }

    client.disconnect().await.unwrap();
    let _ = server_done_tx.send(());
    server.await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_submit_order_timeout_report_stays_submitted() {
    let (addr, state) = start_mock_http().await;
    let fixture = load_fixture("rest/betting_place_order_success.json");
    let mut response: Value = serde_json::from_str(&fixture).unwrap();
    response["result"]["status"] = Value::String("TIMEOUT".to_string());
    state
        .betting_overrides
        .lock()
        .insert(METHOD_PLACE_ORDERS.to_string(), response["result"].clone());

    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, cache) = create_test_execution_client(addr, stream_port);
    let (server_done_tx, server_done_rx) = tokio::sync::oneshot::channel();

    let server = tokio::spawn(async move {
        let (_reader, write_half) = accept_and_activate(&listener).await;
        let _ = server_done_rx.await;
        drop(write_half);
    });

    connect_execution_ready(&mut client).await;

    while rx.try_recv().is_ok() {}

    let order = make_test_order(
        "1.181005744-86362-0.BETFAIR",
        "O-SUBMIT-TIMEOUT",
        "2.58",
        "10",
    );
    add_order_to_cache(&cache, order.clone());
    client.submit_order(make_submit_order_cmd(&order)).unwrap();

    let mut submitted = 0;
    let mut terminal = 0;

    while let Ok(Some(event)) = tokio::time::timeout(Duration::from_millis(500), rx.recv()).await {
        match event {
            ExecutionEvent::Order(OrderEventAny::Submitted(_)) => submitted += 1,
            ExecutionEvent::Order(OrderEventAny::Accepted(_) | OrderEventAny::Rejected(_)) => {
                terminal += 1;
            }
            _ => {}
        }
    }
    assert_eq!(submitted, 1);
    assert_eq!(
        terminal, 0,
        "timeout report must remain open for reconciliation"
    );

    client.disconnect().await.unwrap();
    let _ = server_done_tx.send(());
    server.await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_submit_order_error_emits_rejected() {
    let (addr, state) = start_mock_http().await;

    let fixture = load_fixture("rest/betting_place_order_error.json");
    let v: Value = serde_json::from_str(&fixture).unwrap();
    state
        .betting_overrides
        .lock()
        .insert(METHOD_PLACE_ORDERS.to_string(), v["result"].clone());

    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, cache) = create_test_execution_client(addr, stream_port);

    let (server_done_tx, server_done_rx) = tokio::sync::oneshot::channel();

    let server = tokio::spawn(async move {
        let (_reader, write_half) = accept_and_activate(&listener).await;
        let _ = server_done_rx.await;
        drop(write_half);
    });

    connect_execution_ready(&mut client).await;

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
            assert_eq!(rejected.reason, "ErrorInOrder");
        }
        other => panic!("Expected OrderRejected event, found: {other:?}"),
    }

    client.disconnect().await.unwrap();
    let _ = server_done_tx.send(());
    server.await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_modify_order_price_and_quantity_rejects() {
    let (addr, _state) = start_mock_http().await;
    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, cache) = create_test_execution_client(addr, stream_port);

    let (server_done_tx, server_done_rx) = tokio::sync::oneshot::channel();

    let server = tokio::spawn(async move {
        let (_reader, write_half) = accept_and_activate(&listener).await;
        let _ = server_done_rx.await;
        drop(write_half);
    });

    connect_execution_ready(&mut client).await;

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
                    .contains("cannot modify price and quantity simultaneously"),
                "Expected simultaneous modify reason, found: {}",
                rejected.reason,
            );
        }
        other => panic!("Expected ModifyRejected event, found: {other:?}"),
    }

    client.disconnect().await.unwrap();
    let _ = server_done_tx.send(());
    server.await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_modify_order_no_effective_change_rejects() {
    let (addr, _state) = start_mock_http().await;
    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, cache) = create_test_execution_client(addr, stream_port);

    let (server_done_tx, server_done_rx) = tokio::sync::oneshot::channel();

    let server = tokio::spawn(async move {
        let (_reader, write_half) = accept_and_activate(&listener).await;
        let _ = server_done_rx.await;
        drop(write_half);
    });

    connect_execution_ready(&mut client).await;

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
                rejected.reason.contains("no effective change"),
                "Expected no effective change reason, found: {}",
                rejected.reason,
            );
        }
        other => panic!("Expected ModifyRejected event, found: {other:?}"),
    }

    client.disconnect().await.unwrap();
    let _ = server_done_tx.send(());
    server.await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_cancel_all_orders_sends_request() {
    let (addr, state) = start_mock_http().await;
    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, _cache) = create_test_execution_client(addr, stream_port);

    let (server_done_tx, server_done_rx) = tokio::sync::oneshot::channel();

    let server = tokio::spawn(async move {
        let (_reader, write_half) = accept_and_activate(&listener).await;
        let _ = server_done_rx.await;
        drop(write_half);
    });

    connect_execution_ready(&mut client).await;

    while rx.try_recv().is_ok() {}

    let cmd = CancelAllOrders::new(
        TraderId::from("TESTER-001"),
        Some(*BETFAIR_CLIENT_ID),
        StrategyId::from("S-001"),
        InstrumentId::from("1.179082386-235-0.BETFAIR"),
        None,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None, // correlation_id
    );
    client.cancel_all_orders(cmd).unwrap();

    wait_for_mock_state(&state, "METHOD_CANCEL_ORDERS request count >= 1", |state| {
        betting_method_count(state, METHOD_CANCEL_ORDERS) >= 1
    })
    .await;

    let params = state
        .betting_request_params
        .lock()
        .iter()
        .find(|(method, _)| method == METHOD_CANCEL_ORDERS)
        .cloned()
        .expect("cancelOrders call must be recorded")
        .1;
    assert_valid_customer_ref(&params);
    assert!(params.get("instructions").is_none());
    // Cancel-all is fire-and-forget, so this test covers dispatch and payload only.

    client.disconnect().await.unwrap();
    let _ = server_done_tx.send(());
    server.await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_cancel_all_orders_invalid_instrument_emits_no_rejected_locally() {
    let (addr, state) = start_mock_http().await;
    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, _cache) = create_test_execution_client(addr, stream_port);

    let (server_done_tx, server_done_rx) = tokio::sync::oneshot::channel();

    let server = tokio::spawn(async move {
        let (_reader, write_half) = accept_and_activate(&listener).await;
        let _ = server_done_rx.await;
        drop(write_half);
    });

    connect_execution_ready(&mut client).await;

    while rx.try_recv().is_ok() {}

    let cmd = CancelAllOrders::new(
        TraderId::from("TESTER-001"),
        Some(*BETFAIR_CLIENT_ID),
        StrategyId::from("S-001"),
        InstrumentId::from("INVALID.BETFAIR"),
        None,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None, // correlation_id
    );
    client.cancel_all_orders(cmd).unwrap();

    let mut rejected_seen = false;

    while let Ok(Some(event)) = tokio::time::timeout(Duration::from_millis(500), rx.recv()).await {
        if matches!(
            event,
            ExecutionEvent::Order(OrderEventAny::CancelRejected(_))
        ) {
            rejected_seen = true;
            break;
        }
    }
    assert!(
        !rejected_seen,
        "local cancel-all validation failure must not emit CancelRejected",
    );
    assert_eq!(state.betting_request_count.load(Ordering::Relaxed), 0);

    client.disconnect().await.unwrap();
    let _ = server_done_tx.send(());
    server.await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_cancel_all_orders_ambiguous_5xx_emits_no_rejected() {
    let (addr, state) = start_mock_http().await;
    state
        .betting_status_overrides
        .lock()
        .insert(METHOD_CANCEL_ORDERS.to_string(), 502);

    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, _cache) = create_test_execution_client(addr, stream_port);

    let (server_done_tx, server_done_rx) = tokio::sync::oneshot::channel();

    let server = tokio::spawn(async move {
        let (_reader, write_half) = accept_and_activate(&listener).await;
        let _ = server_done_rx.await;
        drop(write_half);
    });

    connect_execution_ready(&mut client).await;

    while rx.try_recv().is_ok() {}

    let cmd = CancelAllOrders::new(
        TraderId::from("TESTER-001"),
        Some(*BETFAIR_CLIENT_ID),
        StrategyId::from("S-001"),
        InstrumentId::from("1.179082386-235-0.BETFAIR"),
        None,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None, // correlation_id
    );
    client.cancel_all_orders(cmd).unwrap();

    wait_for_mock_state(&state, "METHOD_CANCEL_ORDERS request count >= 1", |state| {
        betting_method_count(state, METHOD_CANCEL_ORDERS) >= 1
    })
    .await;

    tokio::time::sleep(Duration::from_secs(1)).await;

    let mut rejected_seen = false;

    while let Ok(Some(event)) = tokio::time::timeout(Duration::from_millis(500), rx.recv()).await {
        if matches!(
            event,
            ExecutionEvent::Order(OrderEventAny::CancelRejected(_))
        ) {
            rejected_seen = true;
            break;
        }
    }
    assert!(
        !rejected_seen,
        "ambiguous cancel-all 5xx must not emit CancelRejected",
    );

    client.disconnect().await.unwrap();
    let _ = server_done_tx.send(());
    server.await.unwrap();
}

#[rstest]
#[case(OrderSide::Buy, ["101", "102"])]
#[case(OrderSide::Sell, ["201", "202"])]
#[tokio::test]
async fn test_cancel_all_orders_filters_side_instrument_and_owner(
    #[case] order_side: OrderSide,
    #[case] expected_bet_ids: [&str; 2],
) {
    let (addr, state) = start_mock_http().await;
    let fixture = load_fixture("rest/betting_cancel_orders_batch_partial_failure.json");
    let response: Value = serde_json::from_str(&fixture).unwrap();
    state
        .betting_overrides
        .lock()
        .insert(METHOD_CANCEL_ORDERS.to_string(), response["result"].clone());
    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, cache) = create_test_execution_client(addr, stream_port);
    let (server_done_tx, server_done_rx) = tokio::sync::oneshot::channel();

    let server = tokio::spawn(async move {
        let (_reader, write_half) = accept_and_activate(&listener).await;
        let _ = server_done_rx.await;
        drop(write_half);
    });

    connect_execution_ready(&mut client).await;

    while rx.try_recv().is_ok() {}

    let instrument_id = "1.179082386-235-0.BETFAIR";
    let other_instrument_id = "1.179082386-999-0.BETFAIR";
    let other_client_id = ClientId::from("BETFAIR-OTHER");
    let orders = [
        (
            instrument_id,
            "O-BUY-LOCAL",
            "101",
            "S-001",
            "BETFAIR-001",
            OrderSide::Buy,
            Some(*BETFAIR_CLIENT_ID),
        ),
        (
            instrument_id,
            "O-BUY-EXTERNAL",
            "102",
            "EXTERNAL",
            "BETFAIR-001",
            OrderSide::Buy,
            Some(*BETFAIR_CLIENT_ID),
        ),
        (
            instrument_id,
            "O-SELL-LOCAL",
            "201",
            "S-002",
            "BETFAIR-001",
            OrderSide::Sell,
            Some(*BETFAIR_CLIENT_ID),
        ),
        (
            instrument_id,
            "O-SELL-EXTERNAL",
            "202",
            "EXTERNAL",
            "BETFAIR-001",
            OrderSide::Sell,
            Some(*BETFAIR_CLIENT_ID),
        ),
        (
            other_instrument_id,
            "O-BUY-OTHER-INSTRUMENT",
            "103",
            "S-001",
            "BETFAIR-001",
            OrderSide::Buy,
            Some(*BETFAIR_CLIENT_ID),
        ),
        (
            other_instrument_id,
            "O-SELL-OTHER-INSTRUMENT",
            "203",
            "S-001",
            "BETFAIR-001",
            OrderSide::Sell,
            Some(*BETFAIR_CLIENT_ID),
        ),
        (
            instrument_id,
            "O-BUY-OTHER-ACCOUNT",
            "104",
            "S-001",
            "BETFAIR-OTHER",
            OrderSide::Buy,
            Some(*BETFAIR_CLIENT_ID),
        ),
        (
            instrument_id,
            "O-SELL-OTHER-ACCOUNT",
            "204",
            "S-001",
            "BETFAIR-OTHER",
            OrderSide::Sell,
            Some(*BETFAIR_CLIENT_ID),
        ),
        (
            instrument_id,
            "O-BUY-OTHER-CLIENT",
            "105",
            "S-001",
            "BETFAIR-001",
            OrderSide::Buy,
            Some(other_client_id),
        ),
        (
            instrument_id,
            "O-SELL-OTHER-CLIENT",
            "205",
            "S-001",
            "BETFAIR-001",
            OrderSide::Sell,
            Some(other_client_id),
        ),
        (
            instrument_id,
            "O-BUY-NO-CLIENT",
            "106",
            "S-001",
            "BETFAIR-001",
            OrderSide::Buy,
            None,
        ),
        (
            instrument_id,
            "O-SELL-NO-CLIENT",
            "206",
            "S-001",
            "BETFAIR-001",
            OrderSide::Sell,
            None,
        ),
    ];

    for (instrument, client_order_id, venue_order_id, strategy, account, side, source) in orders {
        let order = make_accepted_test_order_for(
            instrument,
            client_order_id,
            venue_order_id,
            strategy,
            account,
            side,
        );
        add_order_to_cache_for_client(&cache, order, source);
    }
    cache.borrow_mut().build_index();

    client
        .cancel_all_orders(make_cancel_all_orders_cmd(instrument_id, Some(order_side)))
        .unwrap();

    wait_for_mock_state(&state, "METHOD_CANCEL_ORDERS request count == 1", |state| {
        betting_method_count(state, METHOD_CANCEL_ORDERS) == 1
    })
    .await;

    let params = state
        .betting_request_params
        .lock()
        .iter()
        .find(|(method, _)| method == METHOD_CANCEL_ORDERS)
        .unwrap()
        .1
        .clone();
    let mut bet_ids = params["instructions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|instruction| instruction["betId"].as_str().unwrap().to_string())
        .collect::<Vec<_>>();
    bet_ids.sort();
    let mut expected_bet_ids = expected_bet_ids.map(str::to_string).to_vec();
    expected_bet_ids.sort();

    assert_eq!(params["marketId"], "1.179082386");
    assert_eq!(bet_ids, expected_bet_ids);
    assert_valid_customer_ref(&params);
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(rx.try_recv().is_err());

    client.disconnect().await.unwrap();
    let _ = server_done_tx.send(());
    server.await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_cancel_all_orders_side_with_no_matches_sends_no_request() {
    let (addr, state) = start_mock_http().await;
    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, cache) = create_test_execution_client(addr, stream_port);
    let (server_done_tx, server_done_rx) = tokio::sync::oneshot::channel();

    let server = tokio::spawn(async move {
        let (_reader, write_half) = accept_and_activate(&listener).await;
        let _ = server_done_rx.await;
        drop(write_half);
    });

    connect_execution_ready(&mut client).await;

    while rx.try_recv().is_ok() {}

    let instrument_id = "1.179082386-235-0.BETFAIR";
    let order = make_accepted_test_order_for(
        instrument_id,
        "O-SELL-ONLY",
        "201",
        "S-001",
        "BETFAIR-001",
        OrderSide::Sell,
    );
    add_order_to_cache(&cache, order);
    cache.borrow_mut().build_index();

    client
        .cancel_all_orders(make_cancel_all_orders_cmd(
            instrument_id,
            Some(OrderSide::Buy),
        ))
        .unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;

    assert_eq!(betting_method_count(&state, METHOD_CANCEL_ORDERS), 0);
    assert!(rx.try_recv().is_err());

    client.disconnect().await.unwrap();
    let _ = server_done_tx.send(());
    server.await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_cancel_all_orders_side_splits_more_than_sixty_instructions() {
    let (addr, state) = start_mock_http().await;
    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, cache) = create_test_execution_client(addr, stream_port);
    let (server_done_tx, server_done_rx) = tokio::sync::oneshot::channel();

    let server = tokio::spawn(async move {
        let (_reader, write_half) = accept_and_activate(&listener).await;
        let _ = server_done_rx.await;
        drop(write_half);
    });

    connect_execution_ready(&mut client).await;

    while rx.try_recv().is_ok() {}

    let instrument_id = "1.179082386-235-0.BETFAIR";
    let mut expected_bet_ids = HashSet::new();

    for index in 0..61 {
        let client_order_id = format!("O-BUY-{index:03}");
        let venue_order_id = format!("{}", 1_000 + index);
        expected_bet_ids.insert(venue_order_id.clone());
        let order = make_accepted_test_order_for(
            instrument_id,
            &client_order_id,
            &venue_order_id,
            "S-001",
            "BETFAIR-001",
            OrderSide::Buy,
        );
        add_order_to_cache(&cache, order);
    }
    cache.borrow_mut().build_index();

    client
        .cancel_all_orders(make_cancel_all_orders_cmd(
            instrument_id,
            Some(OrderSide::Buy),
        ))
        .unwrap();

    wait_for_mock_state(&state, "METHOD_CANCEL_ORDERS request count == 2", |state| {
        betting_method_count(state, METHOD_CANCEL_ORDERS) == 2
    })
    .await;

    let params = state
        .betting_request_params
        .lock()
        .iter()
        .filter(|(method, _)| method == METHOD_CANCEL_ORDERS)
        .map(|(_, params)| params.clone())
        .collect::<Vec<_>>();
    let instruction_counts = params
        .iter()
        .map(|params| params["instructions"].as_array().unwrap().len())
        .collect::<Vec<_>>();
    let actual_bet_ids = params
        .iter()
        .flat_map(|params| params["instructions"].as_array().unwrap())
        .map(|instruction| instruction["betId"].as_str().unwrap().to_string())
        .collect::<HashSet<_>>();
    let customer_refs = params
        .iter()
        .map(assert_valid_customer_ref)
        .collect::<HashSet<_>>();

    assert_eq!(instruction_counts, vec![60, 1]);
    assert_eq!(actual_bet_ids, expected_bet_ids);
    assert_eq!(customer_refs.len(), 2);
    assert!(rx.try_recv().is_err());

    client.disconnect().await.unwrap();
    let _ = server_done_tx.send(());
    server.await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_cancel_all_orders_side_missing_venue_id_fails_closed() {
    let (addr, state) = start_mock_http().await;
    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, cache) = create_test_execution_client(addr, stream_port);
    let (server_done_tx, server_done_rx) = tokio::sync::oneshot::channel();

    let server = tokio::spawn(async move {
        let (_reader, write_half) = accept_and_activate(&listener).await;
        let _ = server_done_rx.await;
        drop(write_half);
    });

    connect_execution_ready(&mut client).await;

    while rx.try_recv().is_ok() {}

    let instrument_id = "1.179082386-235-0.BETFAIR";
    let valid = make_accepted_test_order_for(
        instrument_id,
        "O-BUY-VALID",
        "101",
        "S-001",
        "BETFAIR-001",
        OrderSide::Buy,
    );
    let mut missing = make_accepted_test_order_for(
        instrument_id,
        "O-BUY-NO-VENUE-ID",
        "102",
        "S-002",
        "BETFAIR-001",
        OrderSide::Buy,
    );

    match &mut missing {
        OrderAny::Limit(order) => order.venue_order_id = None,
        _ => unreachable!(),
    }
    add_order_to_cache(&cache, valid);
    add_order_to_cache(&cache, missing);
    cache.borrow_mut().build_index();

    client
        .cancel_all_orders(make_cancel_all_orders_cmd(
            instrument_id,
            Some(OrderSide::Buy),
        ))
        .unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;

    assert_eq!(betting_method_count(&state, METHOD_CANCEL_ORDERS), 0);
    assert!(rx.try_recv().is_err());

    client.disconnect().await.unwrap();
    let _ = server_done_tx.send(());
    server.await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_cancel_all_orders_side_ambiguous_outcome_emits_no_order_event() {
    let (addr, state) = start_mock_http().await;
    state
        .betting_status_overrides
        .lock()
        .insert(METHOD_CANCEL_ORDERS.to_string(), 502);
    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, cache) = create_test_execution_client(addr, stream_port);
    let (server_done_tx, server_done_rx) = tokio::sync::oneshot::channel();

    let server = tokio::spawn(async move {
        let (_reader, write_half) = accept_and_activate(&listener).await;
        let _ = server_done_rx.await;
        drop(write_half);
    });

    connect_execution_ready(&mut client).await;

    while rx.try_recv().is_ok() {}

    let instrument_id = "1.179082386-235-0.BETFAIR";
    let order = make_accepted_test_order_for(
        instrument_id,
        "O-BUY-AMBIGUOUS",
        "101",
        "S-001",
        "BETFAIR-001",
        OrderSide::Buy,
    );
    add_order_to_cache(&cache, order);
    cache.borrow_mut().build_index();

    client
        .cancel_all_orders(make_cancel_all_orders_cmd(
            instrument_id,
            Some(OrderSide::Buy),
        ))
        .unwrap();

    wait_for_mock_state(&state, "METHOD_CANCEL_ORDERS request count == 2", |state| {
        betting_method_count(state, METHOD_CANCEL_ORDERS) == 2
    })
    .await;
    tokio::time::sleep(Duration::from_secs(1)).await;

    let params = state
        .betting_request_params
        .lock()
        .iter()
        .filter(|(method, _)| method == METHOD_CANCEL_ORDERS)
        .map(|(_, params)| params.clone())
        .collect::<Vec<_>>();
    assert_eq!(params.len(), 2, "one 502 must cause exactly one retry");
    assert_eq!(params[0], params[1], "retry must reuse the request body");
    assert_eq!(
        assert_valid_customer_ref(&params[0]),
        assert_valid_customer_ref(&params[1]),
    );
    assert!(rx.try_recv().is_err());

    client.disconnect().await.unwrap();
    let _ = server_done_tx.send(());
    server.await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_ocm_handler_emits_cancel_event() {
    let (addr, _state) = start_mock_http().await;
    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, _cache) = create_test_execution_client(addr, stream_port);

    let ocm_fixture = load_fixture("stream/ocm_CANCEL.json");

    let (server_done_tx, server_done_rx) = tokio::sync::oneshot::channel();

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

        let _ = server_done_rx.await;
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
    let _ = server_done_tx.send(());
    server.await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_ocm_handler_handles_mixed_updates() {
    let (addr, _state) = start_mock_http().await;
    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, _cache) = create_test_execution_client(addr, stream_port);

    let ocm_fixture = load_fixture("stream/ocm_MIXED.json");

    let (server_done_tx, server_done_rx) = tokio::sync::oneshot::channel();

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

        let _ = server_done_rx.await;
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
    let _ = server_done_tx.send(());
    server.await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_ocm_handler_handles_full_image() {
    let (addr, _state) = start_mock_http().await;
    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, _cache) = create_test_execution_client(addr, stream_port);

    let mut ocm_fixture = load_json_fixture("stream/ocm_FULL_IMAGE.json");
    ocm_fixture["id"] = Value::from(2);

    let (server_done_tx, server_done_rx) = tokio::sync::oneshot::channel();

    let server = tokio::spawn(async move {
        let (mut reader, mut write_half) = accept_and_auth(&listener).await;

        let mut line = String::new();
        tokio::io::AsyncBufReadExt::read_line(&mut reader, &mut line)
            .await
            .unwrap();

        tokio::io::AsyncWriteExt::write_all(
            &mut write_half,
            format!("{ocm_fixture}\r\n").as_bytes(),
        )
        .await
        .unwrap();

        let _ = server_done_rx.await;
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
    let _ = server_done_tx.send(());
    server.await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_ocm_voided_partial_emits_both_fill_and_void() {
    let (addr, _state) = start_mock_http().await;
    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, mut data_rx, _cache) = create_test_execution_client(addr, stream_port);

    let ocm_fixture = load_fixture("stream/ocm_VOIDED_partial.json");

    let (server_done_tx, server_done_rx) = tokio::sync::oneshot::channel();

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

        let _ = server_done_rx.await;
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
    let _ = server_done_tx.send(());
    server.await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_ocm_no_void_event_when_sv_zero() {
    let (addr, _state) = start_mock_http().await;
    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, mut data_rx, _cache) = create_test_execution_client(addr, stream_port);

    let ocm_fixture = load_fixture("stream/ocm_FILLED_sv_zero.json");

    let (server_done_tx, server_done_rx) = tokio::sync::oneshot::channel();

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

        let _ = server_done_rx.await;
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
    let _ = server_done_tx.send(());
    server.await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_submit_order_registers_customer_order_ref() {
    let (addr, state) = start_mock_http().await;
    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, cache) = create_test_execution_client(addr, stream_port);

    let (server_done_tx, server_done_rx) = tokio::sync::oneshot::channel();

    let server = tokio::spawn(async move {
        let (_reader, write_half) = accept_and_activate(&listener).await;
        let _ = server_done_rx.await;
        drop(write_half);
    });

    connect_execution_ready(&mut client).await;

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
        .iter()
        .any(|m| m == METHOD_PLACE_ORDERS);
    assert!(has_place_orders, "Expected placeOrders call");

    client.disconnect().await.unwrap();
    let _ = server_done_tx.send(());
    server.await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_submit_order_denies_reduce_only_before_submission() {
    let (addr, state) = start_mock_http().await;
    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, cache) = create_test_execution_client(addr, stream_port);
    let (server_done_tx, server_done_rx) = tokio::sync::oneshot::channel();

    let server = tokio::spawn(async move {
        let (_reader, write_half) = accept_and_activate(&listener).await;
        let _ = server_done_rx.await;
        drop(write_half);
    });

    let instrument_id = "1.181005744-86362-0.BETFAIR";
    let client_order_id = ClientOrderId::from("O-REDUCE-ONLY");
    let order = make_reduce_only_test_order(instrument_id, client_order_id.as_str(), "2.58", "10");
    add_order_to_cache(&cache, order.clone());

    connect_execution_ready(&mut client).await;

    while rx.try_recv().is_ok() {}

    client.submit_order(make_submit_order_cmd(&order)).unwrap();

    let events = drain_events(&mut rx, Duration::from_millis(300)).await;
    let denials = events
        .iter()
        .filter_map(|event| match event {
            ExecutionEvent::Order(OrderEventAny::Denied(denied)) => {
                Some((denied.client_order_id, denied.reason.as_str()))
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(denials, vec![(client_order_id, "UNSUPPORTED_REDUCE_ONLY")],);
    assert!(
        !events.iter().any(|event| matches!(
            event,
            ExecutionEvent::Order(OrderEventAny::Submitted(_) | OrderEventAny::Accepted(_))
        )),
        "reduce-only order advanced before denial: {events:?}",
    );
    assert_eq!(betting_method_count(&state, METHOD_PLACE_ORDERS), 0);

    client.disconnect().await.unwrap();
    let _ = server_done_tx.send(());
    server.await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_submit_order_list_denies_all_orders_when_reduce_only_is_present() {
    let (addr, state) = start_mock_http().await;
    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, cache) = create_test_execution_client(addr, stream_port);
    let (server_done_tx, server_done_rx) = tokio::sync::oneshot::channel();

    let server = tokio::spawn(async move {
        let (_reader, write_half) = accept_and_activate(&listener).await;
        let _ = server_done_rx.await;
        drop(write_half);
    });

    let instrument_id = "1.181005744-86362-0.BETFAIR";
    let reduce_only =
        make_reduce_only_test_order(instrument_id, "O-LIST-REDUCE-ONLY", "2.58", "10");
    let valid = make_test_order(instrument_id, "O-LIST-VALID", "3.00", "5");
    for order in [&reduce_only, &valid] {
        add_order_to_cache(&cache, order.clone());
    }

    connect_execution_ready(&mut client).await;

    while rx.try_recv().is_ok() {}

    let (cmd, _) = make_submit_order_list_cmd(instrument_id, &[reduce_only.clone(), valid.clone()]);
    client.submit_order_list(cmd).unwrap();

    let events = drain_events(&mut rx, Duration::from_millis(300)).await;
    let denials = events
        .iter()
        .filter_map(|event| match event {
            ExecutionEvent::Order(OrderEventAny::Denied(denied)) => {
                Some((denied.client_order_id, denied.reason.as_str()))
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        denials,
        vec![
            (reduce_only.client_order_id(), "UNSUPPORTED_REDUCE_ONLY",),
            (valid.client_order_id(), "UNSUPPORTED_REDUCE_ONLY"),
        ],
    );
    assert!(
        !events.iter().any(|event| matches!(
            event,
            ExecutionEvent::Order(OrderEventAny::Submitted(_) | OrderEventAny::Accepted(_))
        )),
        "invalid order list advanced before denial: {events:?}",
    );
    assert_eq!(betting_method_count(&state, METHOD_PLACE_ORDERS), 0);

    client.disconnect().await.unwrap();
    let _ = server_done_tx.send(());
    server.await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_submit_order_denies_active_customer_order_ref_collision() {
    let (addr, state) = start_mock_http().await;
    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, cache) = create_test_execution_client(addr, stream_port);
    let (server_done_tx, server_done_rx) = tokio::sync::oneshot::channel();

    let server = tokio::spawn(async move {
        let (_reader, write_half) = accept_and_activate(&listener).await;
        let _ = server_done_rx.await;
        drop(write_half);
    });
    let suffix = "12345678901234567890123456789012";
    let active_id = format!("ACTIVE-{suffix}");
    let colliding_id = format!("COLLIDING-{suffix}");
    let instrument_id = "1.181005744-86362-0.BETFAIR";
    let active = make_accepted_test_order(instrument_id, &active_id, "228302937743", "2.58", "10");
    let colliding = make_test_order(instrument_id, &colliding_id, "2.58", "10");
    add_order_to_cache(&cache, active);
    add_order_to_cache(&cache, colliding.clone());

    connect_execution_ready(&mut client).await;

    while rx.try_recv().is_ok() {}

    client
        .submit_order(make_submit_order_cmd(&colliding))
        .unwrap();

    let event = tokio::time::timeout(Duration::from_secs(2), rx.recv())
        .await
        .expect("timeout waiting for collision denial")
        .expect("execution event channel closed");
    let ExecutionEvent::Order(OrderEventAny::Denied(denied)) = event else {
        panic!("customerOrderRef collision must emit OrderDenied: {event:?}");
    };
    assert_eq!(denied.client_order_id, ClientOrderId::from(colliding_id));
    assert_eq!(
        denied.reason,
        OrderDeniedReason::ValidationFailed {
            detail: format!("customerOrderRef {suffix} collides with another tracked order"),
        }
        .to_string(),
    );
    let settled = drain_events(&mut rx, Duration::from_millis(300)).await;
    assert!(
        !settled.iter().any(|event| matches!(
            event,
            ExecutionEvent::Order(OrderEventAny::Submitted(_) | OrderEventAny::Accepted(_))
        )),
        "colliding submission advanced before denial: {settled:?}",
    );
    assert_eq!(betting_method_count(&state, METHOD_PLACE_ORDERS), 0);

    client.disconnect().await.unwrap();
    let _ = server_done_tx.send(());
    server.await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_submit_order_list_denies_only_customer_order_ref_collision() {
    let (addr, state) = start_mock_http().await;
    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, cache) = create_test_execution_client(addr, stream_port);
    let (server_done_tx, server_done_rx) = tokio::sync::oneshot::channel();

    let server = tokio::spawn(async move {
        let (_reader, write_half) = accept_and_activate(&listener).await;
        let _ = server_done_rx.await;
        drop(write_half);
    });
    let suffix = "12345678901234567890123456789012";
    let active_id = format!("ACTIVE-{suffix}");
    let colliding_id = format!("COLLIDING-{suffix}");
    let valid_id = "O-LIST-COLLISION-VALID";
    let instrument_id = "1.181005744-86362-0.BETFAIR";
    let active = make_accepted_test_order(instrument_id, &active_id, "228302937700", "2.58", "10");
    let colliding = make_test_order(instrument_id, &colliding_id, "2.58", "10");
    let valid = make_test_order(instrument_id, valid_id, "3.00", "5");
    for order in [&active, &colliding, &valid] {
        add_order_to_cache(&cache, order.clone());
    }

    connect_execution_ready(&mut client).await;

    while rx.try_recv().is_ok() {}

    let (cmd, _) = make_submit_order_list_cmd(instrument_id, &[colliding.clone(), valid.clone()]);
    client.submit_order_list(cmd).unwrap();

    let events = drain_events(&mut rx, Duration::from_millis(500)).await;
    let denied = events
        .iter()
        .filter_map(|event| match event {
            ExecutionEvent::Order(OrderEventAny::Denied(denied)) => Some(denied.client_order_id),
            _ => None,
        })
        .collect::<Vec<_>>();
    let submitted = events
        .iter()
        .filter_map(|event| match event {
            ExecutionEvent::Order(OrderEventAny::Submitted(submitted)) => {
                Some(submitted.client_order_id)
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    let accepted = events
        .iter()
        .filter_map(|event| match event {
            ExecutionEvent::Order(OrderEventAny::Accepted(accepted)) => {
                Some(accepted.client_order_id)
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(denied, vec![ClientOrderId::from(colliding_id)]);
    assert_eq!(submitted, vec![ClientOrderId::from(valid_id)]);
    assert_eq!(accepted, vec![ClientOrderId::from(valid_id)]);
    assert_eq!(betting_method_count(&state, METHOD_PLACE_ORDERS), 1);
    let params = state
        .betting_request_params
        .lock()
        .iter()
        .find(|(method, _)| method == METHOD_PLACE_ORDERS)
        .cloned()
        .expect("valid order list leg must reach placeOrders")
        .1;
    let instructions = params["instructions"].as_array().unwrap();
    assert_eq!(instructions.len(), 1);
    let expected_customer_order_ref = make_customer_order_ref(valid_id);
    assert_eq!(
        instructions[0]["customerOrderRef"].as_str(),
        Some(expected_customer_order_ref.as_str()),
    );

    client.disconnect().await.unwrap();
    let _ = server_done_tx.send(());
    server.await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_ocm_filled_no_avp_uses_order_price() {
    let (addr, _state) = start_mock_http().await;
    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, _cache) = create_test_execution_client(addr, stream_port);

    let ocm_fixture = load_fixture("stream/ocm_FILLED_no_avp.json");

    let (server_done_tx, server_done_rx) = tokio::sync::oneshot::channel();

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

        let _ = server_done_rx.await;
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
    let _ = server_done_tx.send(());
    server.await.unwrap();
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
        .insert(METHOD_LIST_CURRENT_ORDERS.to_string(), v["result"].clone());

    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, _cache) = create_test_execution_client(addr, stream_port);

    let (server_done_tx, server_done_rx) = tokio::sync::oneshot::channel();

    let server = tokio::spawn(async move {
        let (_reader, write_half) = accept_and_activate(&listener).await;
        let _ = server_done_rx.await;
        drop(write_half);
    });

    connect_execution_ready(&mut client).await;

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
    let _ = server_done_tx.send(());
    server.await.unwrap();
}

#[rstest]
#[case::small(0, false, false, 0.0)]
#[case::large(30_000, false, false, 0.0)]
#[case::unknown_successor(0, true, false, 0.0)]
#[case::terminal_successor(30_000, true, true, 0.0)]
#[case::voided_successor(0, false, false, 1.0)]
#[tokio::test]
async fn test_open_only_repeated_replacements_bound_history(
    #[case] history_count: usize,
    #[case] pending: bool,
    #[case] terminal: bool,
    #[case] voided: f64,
) {
    let (addr, state) = start_mock_http().await;
    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, cache) = create_test_execution_client(addr, stream_port);
    let (server_done_tx, server_done_rx) = tokio::sync::oneshot::channel();

    let server = tokio::spawn(async move {
        let (_reader, write_half) = accept_and_activate(&listener).await;
        let _ = server_done_rx.await;
        drop(write_half);
    });

    let instrument_id = "1.179082386-235.BETFAIR";
    let client_order_id = "O-SCOPED-REPLACEMENTS";
    let bet_ids = ["scoped-0", "scoped-1", "scoped-2"];
    add_order_to_cache(
        &cache,
        make_accepted_test_order(instrument_id, client_order_id, bet_ids[0], "2.50", "10"),
    );
    connect_execution_ready(&mut client).await;

    while rx.try_recv().is_ok() {}

    replace_order_for_reports(
        &client,
        &mut rx,
        &state,
        make_price_modify_order_cmd(instrument_id, client_order_id, bet_ids[0], "3.00"),
        bet_ids[1],
        false,
    )
    .await;
    replace_order_for_reports(
        &client,
        &mut rx,
        &state,
        make_price_modify_order_cmd(instrument_id, client_order_id, bet_ids[1], "4.00"),
        bet_ids[2],
        pending,
    )
    .await;

    let mut successor = current_order_leg(
        bet_ids[2],
        client_order_id,
        5.0,
        if terminal { 5.0 } else { 1.0 },
        if terminal { 0.0 } else { 4.0 - voided },
        4.0,
    );
    successor["sizeVoided"] = Value::from(voided);
    successor["sizeCancelled"] = Value::from(0.0);
    let mut orders = vec![
        current_order_leg(bet_ids[0], client_order_id, 10.0, 2.0, 0.0, 2.5),
        current_order_leg(bet_ids[1], client_order_id, 8.0, 3.0, 0.0, 3.0),
        successor.clone(),
    ];
    orders.extend((0..19).map(|index| {
        current_order_leg(&format!("open-{index}"), "foreign-open", 7.0, 0.0, 7.0, 2.0)
    }));

    orders.extend((0..history_count).map(|index| {
        current_order_leg(
            &format!("history-{index}"),
            "foreign-history",
            9.0,
            9.0,
            0.0,
            8.0,
        )
    }));

    *state.betting_current_orders.lock() = Some(orders);
    let command = GenerateOrderStatusReportsBuilder::default()
        .ts_init(UnixNanos::default())
        .open_only(true)
        .build()
        .unwrap();
    let first = client
        .generate_order_status_reports(&command)
        .await
        .unwrap();
    let recovered = drain_events(&mut rx, Duration::from_millis(100)).await;
    let updates = order_updates(&recovered);
    assert_eq!(updates.len(), usize::from(pending));

    if pending {
        assert_eq!(
            updates[0].venue_order_id,
            Some(VenueOrderId::from(bet_ids[2]))
        );
        assert_eq!(updates[0].quantity, Quantity::from("10"));
    }

    assert_eq!(first.len(), if pending { 19 } else { 20 });

    let reports = client
        .generate_order_status_reports(&command)
        .await
        .unwrap();
    let requests: Vec<_> = state
        .betting_request_params
        .lock()
        .iter()
        .filter(|(method, _)| method == METHOD_LIST_CURRENT_ORDERS)
        .map(|(_, params)| params.clone())
        .collect();
    assert_eq!(reports.len(), if terminal { 19 } else { 20 });

    let expected_requests = if pending && !terminal { 5 } else { 4 };
    assert_eq!(requests.len(), expected_requests);

    for params in &requests {
        if params["orderProjection"] == "ALL" {
            assert!(params["betIds"].is_array() || params["customerOrderRefs"].is_array());
        } else {
            assert_eq!(params["orderProjection"], "EXECUTABLE");
        }
    }

    assert_eq!(requests[0]["orderProjection"], "EXECUTABLE");
    assert_eq!(
        requests[1]["betIds"],
        serde_json::json!([bet_ids[0], bet_ids[1]])
    );
    assert_eq!(
        state
            .betting_current_orders_returned
            .load(Ordering::Relaxed),
        if !pending {
            44
        } else if terminal {
            43
        } else {
            47
        }
    );
    assert!(rx.try_recv().is_err());

    if terminal {
        let mut full = command.clone();
        full.open_only = false;
        let full_reports = client.generate_order_status_reports(&full).await.unwrap();
        let report = full_reports
            .iter()
            .find(|report| report.venue_order_id.as_str() == bet_ids[2])
            .unwrap();
        assert_replacement_report(
            report,
            successor,
            client_order_id,
            "10",
            Decimal::new(34, 1),
        );
    } else {
        let report = reports
            .iter()
            .find(|report| report.venue_order_id.as_str() == bet_ids[2])
            .unwrap();
        assert_replacement_report(report, successor, client_order_id, "6", Decimal::from(3));
    }

    client.disconnect().await.unwrap();
    let _ = server_done_tx.send(());
    server.await.unwrap();
}

fn current_order_leg(
    bet_id: &str,
    client_order_id: &str,
    quantity: f64,
    matched: f64,
    remaining: f64,
    price: f64,
) -> Value {
    let mut order =
        load_json_fixture("rest/list_current_orders_executable.json")["result"]["currentOrders"][0]
            .clone();
    order["betId"] = Value::from(bet_id);
    order["marketId"] = Value::from("1.179082386");
    order["selectionId"] = Value::from(235);
    order["customerOrderRef"] = Value::from(client_order_id);
    order["priceSize"]["price"] = Value::from(price);
    order["priceSize"]["size"] = Value::from(quantity);
    order["sizeMatched"] = Value::from(matched);
    order["sizeRemaining"] = Value::from(remaining);
    order["sizeCancelled"] = Value::from(quantity - matched - remaining);
    order["averagePriceMatched"] = Value::from(price);
    order["status"] = Value::from(if remaining == 0.0 {
        "EXECUTION_COMPLETE"
    } else {
        "EXECUTABLE"
    });

    order
}

async fn replace_order_for_reports(
    client: &BetfairExecutionClient,
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
    state: &MockState,
    command: ModifyOrder,
    successor: &str,
    pending: bool,
) {
    let mut response =
        load_json_fixture("rest/betting_replace_orders_success.json")["result"].clone();
    response["instructionReports"][0]["placeInstructionReport"]["betId"] = Value::from(successor);
    state
        .betting_overrides
        .lock()
        .insert(METHOD_REPLACE_ORDERS.to_string(), response);

    if pending {
        state
            .betting_status_overrides
            .lock()
            .insert(METHOD_REPLACE_ORDERS.to_string(), 502);
    }

    let previous_requests = betting_method_count(state, METHOD_REPLACE_ORDERS);
    client.modify_order(command).unwrap();

    if pending {
        wait_for_mock_state(state, "ambiguous replace retries once", |state| {
            betting_method_count(state, METHOD_REPLACE_ORDERS) == previous_requests + 2
        })
        .await;

        assert!(
            drain_events(rx, Duration::from_millis(100))
                .await
                .is_empty()
        );
        return;
    }

    let event = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .unwrap()
        .unwrap();

    let ExecutionEvent::Order(OrderEventAny::Updated(updated)) = event else {
        panic!("expected successful replacement update, was {event:?}");
    };

    assert_eq!(updated.venue_order_id, Some(VenueOrderId::from(successor)));
}

fn assert_replacement_report(
    report: &OrderStatusReport,
    successor: Value,
    client_order_id: &str,
    filled: &str,
    avg_px: Decimal,
) {
    let successor = serde_json::from_value(successor).unwrap();
    let mut expected =
        parse_current_order_report(&successor, report.account_id, report.ts_init).unwrap();
    expected.report_id = report.report_id;
    expected.client_order_id = Some(ClientOrderId::from(client_order_id));
    expected.quantity = Quantity::from("10");
    expected.filled_qty = Quantity::from(filled);
    expected.avg_px = Some(avg_px);
    assert_eq!(report, &expected);
}

#[rstest]
#[tokio::test]
async fn test_open_only_ambiguous_reference_preserves_known_bet_accounting() {
    let (addr, state) = start_mock_http().await;
    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, cache) = create_test_execution_client(addr, stream_port);
    let (server_done_tx, server_done_rx) = tokio::sync::oneshot::channel();

    let server = tokio::spawn(async move {
        let (_reader, write_half) = accept_and_activate(&listener).await;
        let _ = server_done_rx.await;
        drop(write_half);
    });

    let reference = "12345678901234567890123456789012";
    let first = format!("{reference}-first");
    let second = format!("{reference}-second");
    let instrument_id = "1.179082386-235.BETFAIR";
    add_order_to_cache(
        &cache,
        make_accepted_test_order(instrument_id, &first, "ambiguous-old", "2.50", "10"),
    );
    add_order_to_cache(
        &cache,
        make_accepted_test_order(instrument_id, &second, "ambiguous-other", "7.00", "13"),
    );
    connect_execution_ready(&mut client).await;

    while rx.try_recv().is_ok() {}
    replace_order_for_reports(
        &client,
        &mut rx,
        &state,
        make_price_modify_order_cmd(instrument_id, &first, "ambiguous-old", "3.00"),
        "ambiguous-new",
        false,
    )
    .await;
    let successor = current_order_leg("ambiguous-new", reference, 8.0, 1.0, 7.0, 3.0);
    let other = current_order_leg("ambiguous-other", reference, 13.0, 4.0, 9.0, 7.0);
    *state.betting_current_orders.lock() = Some(vec![
        current_order_leg("ambiguous-old", reference, 10.0, 2.0, 0.0, 2.5),
        successor.clone(),
        other.clone(),
    ]);
    let command = GenerateOrderStatusReportsBuilder::default()
        .ts_init(UnixNanos::default())
        .open_only(true)
        .build()
        .unwrap();
    let reports = client
        .generate_order_status_reports(&command)
        .await
        .unwrap();

    let mut expected: Vec<_> = [successor, other]
        .into_iter()
        .zip(&reports)
        .map(|(order, report)| {
            let order = serde_json::from_value(order).unwrap();
            let mut expected =
                parse_current_order_report(&order, report.account_id, report.ts_init).unwrap();
            expected.report_id = report.report_id;
            expected.client_order_id = None;
            expected
        })
        .collect();

    expected[0].quantity = Quantity::from("10");
    expected[0].filled_qty = Quantity::from("3");
    expected[0].avg_px = Some(Decimal::from(8) / Decimal::from(3));
    let requests: Vec<_> = state
        .betting_request_params
        .lock()
        .iter()
        .filter(|(method, _)| method == METHOD_LIST_CURRENT_ORDERS)
        .map(|(_, params)| params.clone())
        .collect();
    assert_eq!(reports.len(), 2);
    assert_eq!(reports, expected);
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0]["orderProjection"], "EXECUTABLE");
    assert_eq!(requests[1]["orderProjection"], "ALL");
    assert_eq!(requests[1]["betIds"], serde_json::json!(["ambiguous-old"]));
    assert_eq!(
        state
            .betting_current_orders_returned
            .load(Ordering::Relaxed),
        3
    );
    assert!(rx.try_recv().is_err());
    client.disconnect().await.unwrap();
    let _ = server_done_tx.send(());
    server.await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_open_only_missing_replacement_history_fails_without_consuming_recovery() {
    let (addr, state) = start_mock_http().await;
    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, cache) = create_test_execution_client(addr, stream_port);
    let (server_done_tx, server_done_rx) = tokio::sync::oneshot::channel();

    let server = tokio::spawn(async move {
        let (_reader, write_half) = accept_and_activate(&listener).await;
        let _ = server_done_rx.await;
        drop(write_half);
    });

    let instrument_id = "1.179082386-235.BETFAIR";
    let client_order_id = "O-MISSING-HISTORY";
    add_order_to_cache(
        &cache,
        make_accepted_test_order(instrument_id, client_order_id, "missing-old", "2.50", "10"),
    );
    connect_execution_ready(&mut client).await;

    while rx.try_recv().is_ok() {}
    replace_order_for_reports(
        &client,
        &mut rx,
        &state,
        make_price_modify_order_cmd(instrument_id, client_order_id, "missing-old", "3.00"),
        "missing-new",
        true,
    )
    .await;
    let successor = current_order_leg("missing-new", client_order_id, 6.0, 2.0, 4.0, 3.0);
    *state.betting_current_orders.lock() = Some(vec![successor.clone()]);
    let command = GenerateOrderStatusReportsBuilder::default()
        .ts_init(UnixNanos::default())
        .open_only(true)
        .build()
        .unwrap();
    let error = client
        .generate_order_status_reports(&command)
        .await
        .unwrap_err();
    assert_eq!(
        error.to_string(),
        "Missing Betfair order history for tracked Bet ID missing-old"
    );
    assert!(rx.try_recv().is_err());

    *state.betting_current_orders.lock() = Some(vec![
        current_order_leg("missing-old", client_order_id, 10.0, 4.0, 0.0, 2.5),
        successor.clone(),
    ]);
    let resolving = client
        .generate_order_status_reports(&command)
        .await
        .unwrap();
    let events = drain_events(&mut rx, Duration::from_millis(100)).await;
    let reports = client
        .generate_order_status_reports(&command)
        .await
        .unwrap();
    assert!(resolving.is_empty());
    assert_eq!(order_updates(&events).len(), 1);
    assert_eq!(reports.len(), 1);
    assert_replacement_report(
        &reports[0],
        successor,
        client_order_id,
        "6",
        Decimal::from(8) / Decimal::from(3),
    );
    client.disconnect().await.unwrap();
    let _ = server_done_tx.send(());
    server.await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_open_only_supplemental_orders_preserve_reconcile_market_scope() {
    let (addr, state) = start_mock_http().await;
    let (stream_port, listener) = start_mock_stream().await;

    let config = BetfairExecutionClientConfig {
        reconcile_market_ids_only: true,
        reconcile_market_ids: Some(vec!["1.179082386".to_string()]),
        ..Default::default()
    };

    let (mut client, mut rx, _data_rx, cache) =
        create_test_execution_client_with_config(addr, stream_port, config);
    let (server_done_tx, server_done_rx) = tokio::sync::oneshot::channel();

    let server = tokio::spawn(async move {
        let (_reader, write_half) = accept_and_activate(&listener).await;
        let _ = server_done_rx.await;
        drop(write_half);
    });

    add_order_to_cache(
        &cache,
        make_accepted_test_order(
            "1.179082386-235.BETFAIR",
            "O-MARKET-A",
            "market-a",
            "2.50",
            "10",
        ),
    );
    add_order_to_cache(
        &cache,
        make_accepted_test_order(
            "1.179082387-235.BETFAIR",
            "O-MARKET-B",
            "market-b",
            "2.50",
            "10",
        ),
    );
    connect_execution_ready(&mut client).await;

    while rx.try_recv().is_ok() {}
    state
        .betting_status_overrides
        .lock()
        .insert(METHOD_CANCEL_ORDERS.to_string(), 502);
    client
        .modify_order(make_quantity_modify_order_cmd(
            "1.179082387-235.BETFAIR",
            "O-MARKET-B",
            "market-b",
            "4",
        ))
        .unwrap();
    wait_for_mock_state(
        &state,
        "out-of-scope reduction remains ambiguous",
        |state| betting_method_count(state, METHOD_CANCEL_ORDERS) == 2,
    )
    .await;
    assert!(
        drain_events(&mut rx, Duration::from_millis(100))
            .await
            .is_empty()
    );
    let first = current_order_leg("market-a", "O-MARKET-A", 10.0, 0.0, 10.0, 2.5);
    let mut second = current_order_leg("market-b", "O-MARKET-B", 10.0, 0.0, 4.0, 2.5);
    second["marketId"] = Value::from("1.179082387");
    *state.betting_current_orders.lock() = Some(vec![first.clone(), second]);
    let command = GenerateOrderStatusReportsBuilder::default()
        .ts_init(UnixNanos::default())
        .open_only(true)
        .build()
        .unwrap();
    let reports = client
        .generate_order_status_reports(&command)
        .await
        .unwrap();
    let requests: Vec<_> = state
        .betting_request_params
        .lock()
        .iter()
        .filter(|(method, _)| method == METHOD_LIST_CURRENT_ORDERS)
        .map(|(_, params)| params.clone())
        .collect();
    assert_eq!(
        requests,
        vec![
            serde_json::json!({"orderProjection": "EXECUTABLE", "marketIds": ["1.179082386"]}),
            serde_json::json!({"orderProjection": "ALL", "betIds": ["market-b"]}),
        ]
    );
    assert_eq!(
        state
            .betting_current_orders_returned
            .load(Ordering::Relaxed),
        2
    );
    assert_eq!(reports.len(), 1);
    let first = serde_json::from_value(first).unwrap();
    let mut expected =
        parse_current_order_report(&first, reports[0].account_id, reports[0].ts_init).unwrap();
    expected.client_order_id = Some(ClientOrderId::from("O-MARKET-A"));
    expected.report_id = reports[0].report_id;
    assert_eq!(reports[0], expected);
    assert!(rx.try_recv().is_err());
    client.disconnect().await.unwrap();
    let _ = server_done_tx.send(());
    server.await.unwrap();
}

#[rstest]
#[case::failed_fetch(false)]
#[case::cancelled_fetch(true)]
#[tokio::test]
async fn test_open_only_interrupted_supplemental_fetch_preserves_pending_replace(
    #[case] cancel_fetch: bool,
) {
    let (addr, state) = start_mock_http().await;
    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, cache) = create_test_execution_client(addr, stream_port);
    let (server_done_tx, server_done_rx) = tokio::sync::oneshot::channel();

    let server = tokio::spawn(async move {
        let (_reader, write_half) = accept_and_activate(&listener).await;
        let _ = server_done_rx.await;
        drop(write_half);
    });

    let instrument_id = "1.179082386-235.BETFAIR";
    let client_order_id = "O-INTERRUPTED-REPLACE";
    add_order_to_cache(
        &cache,
        make_accepted_test_order(
            instrument_id,
            client_order_id,
            "interrupted-old",
            "2.50",
            "10",
        ),
    );
    connect_execution_ready(&mut client).await;

    while rx.try_recv().is_ok() {}
    replace_order_for_reports(
        &client,
        &mut rx,
        &state,
        make_price_modify_order_cmd(instrument_id, client_order_id, "interrupted-old", "3.00"),
        "interrupted-new",
        true,
    )
    .await;
    let successor = current_order_leg("interrupted-new", client_order_id, 6.0, 2.0, 0.0, 3.0);
    *state.betting_current_orders.lock() = Some(vec![
        current_order_leg("interrupted-old", client_order_id, 10.0, 4.0, 0.0, 2.5),
        successor.clone(),
    ]);

    let gate = MockResponseGate {
        method: METHOD_LIST_CURRENT_ORDERS.to_string(),
        waiters: Arc::new(AtomicUsize::new(0)),
        semaphore: Arc::new(tokio::sync::Semaphore::new(2)),
    };

    *state.betting_response_gate.lock() = Some(gate.clone());
    let command = GenerateOrderStatusReportsBuilder::default()
        .ts_init(UnixNanos::default())
        .open_only(true)
        .build()
        .unwrap();
    {
        let pending = client.generate_order_status_reports(&command);
        tokio::pin!(pending);
        tokio::select! {
            result = &mut pending => panic!("supplemental fetch completed before interruption: {result:?}"),
            () = wait_for_mock_state(&state, "successor reference request blocked", |_| gate.waiters.load(Ordering::Relaxed) == 3) => {}
        }

        if !cancel_fetch {
            state.betting_error_one_shot_overrides.lock().insert(
                METHOD_LIST_CURRENT_ORDERS.to_string(),
                betting_api_error("INVALID_INPUT_DATA"),
            );
            gate.semaphore.add_permits(1);
            let error = pending.await.unwrap_err().to_string();
            assert!(error.contains("INVALID_INPUT_DATA"), "{error}");
        }
    }

    if cancel_fetch {
        gate.semaphore.add_permits(1);
        wait_for_mock_state(&state, "cancelled request settled", |state| {
            state
                .betting_current_orders_returned
                .load(Ordering::Relaxed)
                == 3
        })
        .await;
    }

    *state.betting_response_gate.lock() = None;
    assert!(rx.try_recv().is_err());
    let resolving = client
        .generate_order_status_reports(&command)
        .await
        .unwrap();
    let events = drain_events(&mut rx, Duration::from_millis(100)).await;
    let updates = order_updates(&events);
    let repeated = client
        .generate_order_status_reports(&command)
        .await
        .unwrap();
    let mut full = command;
    full.open_only = false;
    let reports = client.generate_order_status_reports(&full).await.unwrap();
    assert!(resolving.is_empty());
    assert!(repeated.is_empty());
    assert_eq!(events.len(), 1);
    assert_eq!(updates.len(), 1);
    assert_eq!(
        updates[0].client_order_id,
        ClientOrderId::from(client_order_id)
    );
    assert_eq!(
        updates[0].venue_order_id,
        Some(VenueOrderId::from("interrupted-new"))
    );
    assert_eq!(updates[0].quantity, Quantity::from("10"));
    assert_eq!(updates[0].price, Some(Price::from("3.00")));
    assert_eq!(
        reports
            .iter()
            .map(|report| report.venue_order_id.as_str())
            .collect::<Vec<_>>(),
        vec!["interrupted-old", "interrupted-new"],
    );
    assert_replacement_report(
        &reports[1],
        successor,
        client_order_id,
        "6",
        Decimal::from(8) / Decimal::from(3),
    );
    assert!(rx.try_recv().is_err());
    client.disconnect().await.unwrap();
    let _ = server_done_tx.send(());
    server.await.unwrap();
}

#[rstest]
#[case::replacement(true, false)]
#[case::reduction(false, false)]
#[case::reduction_after_replacement(false, true)]
#[tokio::test]
async fn test_open_only_recovers_terminal_modifications(
    #[case] replacement: bool,
    #[case] replaced: bool,
) {
    let (addr, state) = start_mock_http().await;
    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, cache) = create_test_execution_client(addr, stream_port);
    let (server_done_tx, server_done_rx) = tokio::sync::oneshot::channel();

    let server = tokio::spawn(async move {
        let (_reader, write_half) = accept_and_activate(&listener).await;
        let _ = server_done_rx.await;
        drop(write_half);
    });

    let instrument_id = "1.179082386-235.BETFAIR";
    let client_order_id = "O-TERMINAL-MODIFY";
    add_order_to_cache(
        &cache,
        make_accepted_test_order(instrument_id, client_order_id, "terminal-old", "2.50", "10"),
    );
    connect_execution_ready(&mut client).await;

    while rx.try_recv().is_ok() {}

    if replaced {
        replace_order_for_reports(
            &client,
            &mut rx,
            &state,
            make_price_modify_order_cmd(instrument_id, client_order_id, "terminal-old", "3.00"),
            "terminal-new",
            false,
        )
        .await;
    }

    if replacement {
        replace_order_for_reports(
            &client,
            &mut rx,
            &state,
            make_price_modify_order_cmd(instrument_id, client_order_id, "terminal-old", "3.00"),
            "terminal-new",
            true,
        )
        .await;
    } else {
        state
            .betting_status_overrides
            .lock()
            .insert(METHOD_CANCEL_ORDERS.to_string(), 502);
        client
            .modify_order(make_quantity_modify_order_cmd(
                instrument_id,
                client_order_id,
                if replaced {
                    "terminal-new"
                } else {
                    "terminal-old"
                },
                if replaced { "8" } else { "4" },
            ))
            .unwrap();

        wait_for_mock_state(&state, "ambiguous reduction retries once", |state| {
            betting_method_count(state, METHOD_CANCEL_ORDERS) == 2
        })
        .await;

        assert!(
            drain_events(&mut rx, Duration::from_millis(100))
                .await
                .is_empty()
        );
    }

    let mut orders = vec![current_order_leg(
        "terminal-old",
        client_order_id,
        10.0,
        4.0,
        0.0,
        2.5,
    )];

    if replaced {
        orders.push(current_order_leg(
            "terminal-new",
            client_order_id,
            6.0,
            4.0,
            0.0,
            3.0,
        ));
    }

    *state.betting_current_orders.lock() = Some(orders);
    let command = GenerateOrderStatusReportsBuilder::default()
        .ts_init(UnixNanos::default())
        .open_only(true)
        .build()
        .unwrap();
    let reports = client
        .generate_order_status_reports(&command)
        .await
        .unwrap();
    let repeated = client
        .generate_order_status_reports(&command)
        .await
        .unwrap();
    let requests: Vec<_> = state
        .betting_request_params
        .lock()
        .iter()
        .filter(|(method, _)| method == METHOD_LIST_CURRENT_ORDERS)
        .map(|(_, params)| params.clone())
        .collect();
    assert!(reports.is_empty());
    assert!(repeated.is_empty());
    assert_eq!(requests.len(), if replacement { 4 } else { 3 });
    assert_eq!(requests[0]["orderProjection"], "EXECUTABLE");
    assert_eq!(
        requests[1]["betIds"],
        if replaced {
            serde_json::json!(["terminal-new", "terminal-old"])
        } else {
            serde_json::json!(["terminal-old"])
        }
    );
    assert_eq!(requests.last().unwrap()["orderProjection"], "EXECUTABLE");
    assert!(rx.try_recv().is_err());
    let mut full = command;
    full.open_only = false;
    let full_reports = client.generate_order_status_reports(&full).await.unwrap();
    assert_eq!(
        full_reports
            .iter()
            .map(|report| report.venue_order_id.as_str())
            .collect::<Vec<_>>(),
        if replaced {
            vec!["terminal-old", "terminal-new"]
        } else {
            vec!["terminal-old"]
        },
    );
    let report = full_reports.last().unwrap();
    assert_eq!(
        report.quantity,
        Quantity::from(if replacement {
            "10"
        } else if replaced {
            "8"
        } else {
            "4"
        })
    );
    assert_eq!(
        report.filled_qty,
        Quantity::from(if replaced { "8" } else { "4" })
    );
    assert_eq!(
        report.avg_px,
        Some(Decimal::new(if replaced { 275 } else { 250 }, 2))
    );

    if replaced {
        assert_eq!(report.venue_order_id, VenueOrderId::from("terminal-new"));
    }

    client.disconnect().await.unwrap();
    let _ = server_done_tx.send(());
    server.await.unwrap();
}

#[rstest]
#[case::overlapping_pages(false, false)]
#[case::empty_page(true, false)]
#[case::session_refresh(false, true)]
#[tokio::test]
async fn test_open_only_supplemental_pages_preserve_last_observation_and_retry(
    #[case] empty: bool,
    #[case] retry: bool,
) {
    let (addr, state) = start_mock_http().await;
    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, cache) = create_test_execution_client(addr, stream_port);
    let (server_done_tx, server_done_rx) = tokio::sync::oneshot::channel();

    let server = tokio::spawn(async move {
        let (_reader, write_half) = accept_and_activate(&listener).await;
        let _ = server_done_rx.await;
        drop(write_half);
    });

    let instrument_id = "1.179082386-235.BETFAIR";
    let client_order_id = "O-PAGED-REPLACEMENT";
    add_order_to_cache(
        &cache,
        make_accepted_test_order(instrument_id, client_order_id, "paged-old", "2.50", "10"),
    );
    connect_execution_ready(&mut client).await;

    while rx.try_recv().is_ok() {}
    replace_order_for_reports(
        &client,
        &mut rx,
        &state,
        make_price_modify_order_cmd(instrument_id, client_order_id, "paged-old", "3.00"),
        "paged-new",
        false,
    )
    .await;
    let successor = current_order_leg("paged-new", client_order_id, 6.0, 2.0, 4.0, 3.0);

    let first_page = if empty {
        vec![]
    } else {
        vec![current_order_leg(
            "paged-old",
            client_order_id,
            10.0,
            3.0,
            0.0,
            2.5,
        )]
    };

    state.betting_response_sequences.lock().insert(METHOD_LIST_CURRENT_ORDERS.to_string(), VecDeque::from([
        serde_json::json!({"currentOrders": [successor.clone()], "moreAvailable": false}),
        serde_json::json!({"currentOrders": first_page, "moreAvailable": true}),
        serde_json::json!({"currentOrders": [current_order_leg("paged-old", client_order_id, 10.0, 4.0, 0.0, 2.5)], "moreAvailable": false}),
    ]));

    let gate = MockResponseGate {
        method: METHOD_LIST_CURRENT_ORDERS.to_string(),
        semaphore: Arc::new(tokio::sync::Semaphore::new(1)),
        waiters: Arc::new(AtomicUsize::new(0)),
    };

    if retry {
        *state.betting_response_gate.lock() = Some(gate.clone());
    }

    let inject_session_error = async {
        if !retry {
            return;
        }

        wait_for_mock_state(&state, "supplemental request is blocked", |_| {
            gate.waiters.load(Ordering::Relaxed) == 2
        })
        .await;

        state.betting_error_one_shot_overrides.lock().insert(
            METHOD_LIST_CURRENT_ORDERS.to_string(),
            betting_api_error("NO_SESSION"),
        );
        gate.semaphore.add_permits(3);
    };

    let command = GenerateOrderStatusReportsBuilder::default()
        .ts_init(UnixNanos::default())
        .open_only(true)
        .build()
        .unwrap();
    let (result, ()) = tokio::join!(
        client.generate_order_status_reports(&command),
        inject_session_error
    );
    let requests: Vec<_> = state
        .betting_request_params
        .lock()
        .iter()
        .filter(|(method, _)| method == METHOD_LIST_CURRENT_ORDERS)
        .map(|(_, params)| params.clone())
        .collect();

    if empty {
        assert_eq!(
            result.unwrap_err().to_string(),
            "listCurrentOrders returned an empty page with moreAvailable=true"
        );
        assert_eq!(requests.len(), 2);
    } else {
        let reports = result.unwrap();
        assert_eq!(reports.len(), 1);
        assert_replacement_report(
            &reports[0],
            successor,
            client_order_id,
            "6",
            Decimal::from(8) / Decimal::from(3),
        );
        assert_eq!(requests.len(), if retry { 4 } else { 3 });
        assert_eq!(requests.last().unwrap()["fromRecord"], 1);
    }

    assert_eq!(
        requests[0],
        serde_json::json!({"orderProjection": "EXECUTABLE"})
    );
    assert_eq!(
        requests[1],
        serde_json::json!({"orderProjection": "ALL", "betIds": ["paged-old"]})
    );
    assert_eq!(
        state.keep_alive_count.load(Ordering::Relaxed),
        usize::from(retry)
    );

    if retry {
        assert_eq!(requests[1], requests[2]);
    }

    assert!(rx.try_recv().is_err());
    client.disconnect().await.unwrap();
    let _ = server_done_tx.send(());
    server.await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_open_only_supplemental_fetch_allows_stream_reduction_to_resolve_once() {
    let (addr, state) = start_mock_http().await;
    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, cache) = create_test_execution_client(addr, stream_port);
    let (ocm_tx, mut ocm_rx) = tokio::sync::mpsc::unbounded_channel::<String>();

    let server = tokio::spawn(async move {
        let (_reader, mut write_half) = accept_and_activate(&listener).await;

        while let Some(line) = ocm_rx.recv().await {
            write_half
                .write_all(format!("{line}\r\n").as_bytes())
                .await
                .unwrap();
        }
    });

    let instrument_id = "1.179082386-235.BETFAIR";
    let client_order_id = "O-SCOPED-STREAM";
    let bet_id = "stream-reduction";
    add_order_to_cache(
        &cache,
        make_accepted_test_order(instrument_id, client_order_id, bet_id, "2.50", "10"),
    );
    connect_execution_ready(&mut client).await;

    while rx.try_recv().is_ok() {}
    state
        .betting_status_overrides
        .lock()
        .insert(METHOD_CANCEL_ORDERS.to_string(), 502);
    client
        .modify_order(make_quantity_modify_order_cmd(
            instrument_id,
            client_order_id,
            bet_id,
            "4",
        ))
        .unwrap();
    wait_for_mock_state(&state, "ambiguous reduction retries once", |state| {
        betting_method_count(state, METHOD_CANCEL_ORDERS) == 2
    })
    .await;

    assert!(
        drain_events(&mut rx, Duration::from_millis(100))
            .await
            .is_empty()
    );
    *state.betting_current_orders.lock() = Some(vec![current_order_leg(
        bet_id,
        client_order_id,
        10.0,
        4.0,
        0.0,
        2.5,
    )]);

    let gate = MockResponseGate {
        method: METHOD_LIST_CURRENT_ORDERS.to_string(),
        semaphore: Arc::new(tokio::sync::Semaphore::new(1)),
        waiters: Arc::new(AtomicUsize::new(0)),
    };

    *state.betting_response_gate.lock() = Some(gate.clone());

    let stream_reduction = async {
        wait_for_mock_state(
            &state,
            "supplemental query waits for stream overlap",
            |_| gate.waiters.load(Ordering::Relaxed) == 2,
        )
        .await;
        let mut ocm = load_json_fixture("stream/ocm_harness_open.json");
        ocm["id"] = Value::from(2);
        ocm["oc"][0]["id"] = Value::from("1.179082386");
        ocm["oc"][0]["orc"][0]["id"] = Value::from(235);
        let unmatched = &mut ocm["oc"][0]["orc"][0]["uo"][0];
        unmatched["id"] = Value::from(bet_id);
        unmatched["p"] = Value::from(2.5);
        unmatched["side"] = Value::from("L");
        unmatched["sr"] = Value::from(4.0);
        unmatched["sc"] = Value::from(6.0);
        unmatched["rfo"] = Value::from(client_order_id);
        ocm_tx.send(ocm.to_string()).unwrap();
        let event = tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .unwrap()
            .unwrap();

        let ExecutionEvent::Order(OrderEventAny::Updated(updated)) = event else {
            panic!("expected stream reduction while REST remains blocked, was {event:?}");
        };

        gate.semaphore.add_permits(1);
        updated
    };

    let command = GenerateOrderStatusReportsBuilder::default()
        .ts_init(UnixNanos::default())
        .open_only(true)
        .build()
        .unwrap();
    let (reports, updated) = tokio::join!(
        client.generate_order_status_reports(&command),
        stream_reduction
    );
    assert!(reports.unwrap().is_empty());
    assert_eq!(
        updated.client_order_id,
        ClientOrderId::from(client_order_id)
    );
    assert_eq!(updated.instrument_id, InstrumentId::from(instrument_id));
    assert_eq!(updated.quantity, Quantity::from("4"));
    assert_eq!(updated.venue_order_id, Some(VenueOrderId::from(bet_id)));
    assert!(rx.try_recv().is_err());
    assert_eq!(gate.waiters.load(Ordering::Relaxed), 2);
    drop(ocm_tx);
    client.disconnect().await.unwrap();
    server.await.unwrap();
}

#[rstest]
#[case::unfiltered(false, None, None, None, &[0, 1, 2])]
#[case::open_only(true, None, None, None, &[0])]
#[case::selection(false, Some("1.180575118-217709.BETFAIR"), None, None, &[1])]
#[case::handicap(false, Some("1.180575118-217709-1.5.BETFAIR"), None, None, &[2])]
#[case::no_match(false, Some("1.176791264-58805.BETFAIR"), None, None, &[])]
#[case::start(false, None, Some("2021-03-24T06:51:50Z"), None, &[0, 2])]
#[case::end(false, None, None, Some("2021-03-24T06:49:41Z"), &[0, 1])]
#[case::window(
    false,
    None,
    Some("2021-03-24T06:49:41Z"),
    Some("2021-03-24T06:49:41Z"),
    &[0, 1],
)]
#[case::open_after_end(false, None, None, Some("2021-03-24T06:46:00Z"), &[0])]
#[case::instrument_and_time(
    false,
    Some("1.180575118-217709.BETFAIR"),
    Some("2021-03-24T06:49:42Z"),
    None,
    &[],
)]
#[tokio::test]
async fn test_generate_order_status_reports_filters(
    #[case] open_only: bool,
    #[case] instrument_id: Option<&str>,
    #[case] start: Option<&str>,
    #[case] end: Option<&str>,
    #[case] expected_indices: &[usize],
) {
    let (addr, state) = start_mock_http().await;
    let mut response = load_json_fixture("rest/list_current_orders_execution_complete.json");
    response["result"]["currentOrders"][2]["handicap"] = Value::from(1.5);
    state.betting_overrides.lock().insert(
        METHOD_LIST_CURRENT_ORDERS.to_string(),
        response["result"].clone(),
    );
    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, _rx, _data_rx, _cache) = create_test_execution_client(addr, stream_port);
    let (server_done_tx, server_done_rx) = tokio::sync::oneshot::channel();

    let server = tokio::spawn(async move {
        let (_reader, write_half) = accept_and_activate(&listener).await;
        let _ = server_done_rx.await;
        drop(write_half);
    });

    connect_execution_ready(&mut client).await;

    let command = GenerateOrderStatusReportsBuilder::default()
        .ts_init(UnixNanos::default())
        .open_only(open_only)
        .instrument_id(instrument_id.map(InstrumentId::from))
        .start(start.map(|s| parse_betfair_timestamp(s).unwrap()))
        .end(end.map(|s| parse_betfair_timestamp(s).unwrap()))
        .build()
        .unwrap();
    let reports = client
        .generate_order_status_reports(&command)
        .await
        .unwrap();

    client.disconnect().await.unwrap();
    let _ = server_done_tx.send(());
    server.await.unwrap();

    assert_eq!(reports.len(), expected_indices.len());

    for (report, index) in reports.iter().zip(expected_indices) {
        let order =
            serde_json::from_value(response["result"]["currentOrders"][*index].clone()).unwrap();
        let mut expected =
            parse_current_order_report(&order, AccountId::from("BETFAIR-001"), report.ts_init)
                .unwrap();
        expected.report_id = report.report_id;
        assert_eq!(*report, expected);
    }
}

#[rstest]
#[case::invalid_page(false)]
#[case::cancelled_fetch(true)]
#[tokio::test]
async fn test_mass_status_failed_fetch_preserves_fills(#[case] cancel_fetch: bool) {
    let (addr, state) = start_mock_http().await;
    let snapshot =
        load_json_fixture("rest/list_current_orders_execution_complete.json")["result"].clone();
    let mut first = snapshot.clone();
    first["moreAvailable"] = Value::Bool(true);
    let mut invalid = snapshot.clone();
    invalid["currentOrders"][0]["placedDate"] = Value::from("invalid-date");
    state.betting_response_sequences.lock().insert(
        METHOD_LIST_CURRENT_ORDERS.to_string(),
        VecDeque::from([first, invalid]),
    );
    state
        .betting_overrides
        .lock()
        .insert(METHOD_LIST_CURRENT_ORDERS.to_string(), snapshot);
    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, _rx, _data_rx, _cache) = create_test_execution_client(addr, stream_port);
    let (server_done_tx, server_done_rx) = tokio::sync::oneshot::channel();

    let server = tokio::spawn(async move {
        let (_reader, write_half) = accept_and_activate(&listener).await;
        let _ = server_done_rx.await;
        drop(write_half);
    });

    connect_execution_ready(&mut client).await;

    if cancel_fetch {
        let gate = MockResponseGate {
            method: METHOD_LIST_CURRENT_ORDERS.to_string(),
            waiters: Arc::new(AtomicUsize::new(0)),
            semaphore: Arc::new(tokio::sync::Semaphore::new(1)),
        };

        *state.betting_response_gate.lock() = Some(gate.clone());
        {
            let pending = client.generate_mass_status(None);
            tokio::pin!(pending);
            tokio::select! {
                result = &mut pending => panic!("snapshot completed before cancellation: {result:?}"),
                () = wait_for_mock_state(&state, "second snapshot page blocked", |state| response_gate_waiter_count(state) == 2) => {}
            }
        }

        // Release the cancelled HTTP request before starting the retry
        gate.semaphore.add_permits(1);
        wait_for_mock_state(&state, "cancelled page consumed", |state| {
            state
                .betting_response_sequences
                .lock()
                .get(METHOD_LIST_CURRENT_ORDERS)
                .unwrap()
                .is_empty()
        })
        .await;

        *state.betting_response_gate.lock() = None;
    } else {
        assert!(client.generate_mass_status(None).await.is_err());
    }

    let recovered = client.generate_mass_status(None).await.unwrap().unwrap();
    let repeated = client.generate_mass_status(None).await.unwrap().unwrap();
    assert_eq!(recovered.order_reports().len(), 3);
    assert_eq!(recovered.fill_reports().len(), 2);

    for bet in ["228059821049", "228059869313"] {
        let fills = &recovered.fill_reports()[&VenueOrderId::from(bet)];
        assert_eq!(fills.len(), 1);
        assert_eq!(fills[0].last_qty, Quantity::from(10));
    }

    assert!(repeated.fill_reports().is_empty());
    client.disconnect().await.unwrap();
    let _ = server_done_tx.send(());
    server.await.unwrap();
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
        .insert(METHOD_LIST_CURRENT_ORDERS.to_string(), v["result"].clone());

    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, _cache) = create_test_execution_client(addr, stream_port);

    let (server_done_tx, server_done_rx) = tokio::sync::oneshot::channel();

    let server = tokio::spawn(async move {
        let (_reader, write_half) = accept_and_activate(&listener).await;
        let _ = server_done_rx.await;
        drop(write_half);
    });

    connect_execution_ready(&mut client).await;

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

    let replay_cmd = GenerateFillReportsBuilder::default()
        .ts_init(UnixNanos::default())
        .build()
        .unwrap();
    let replayed = client.generate_fill_reports(replay_cmd).await.unwrap();
    assert!(
        replayed.is_empty(),
        "unchanged cumulative order state must not replay fill reports",
    );

    client.disconnect().await.unwrap();
    let _ = server_done_tx.send(());
    server.await.unwrap();
}

#[rstest]
#[case::order(true, false, false)]
#[case::instrument(false, true, false)]
#[case::order_and_instrument(true, true, false)]
#[case::mass_status(true, true, true)]
#[tokio::test]
async fn test_generate_fill_reports_preserves_unrequested_fill_state(
    #[case] filter_order: bool,
    #[case] filter_instrument: bool,
    #[case] mass_status: bool,
) {
    let (addr, state) = start_mock_http().await;
    let mut response = load_json_fixture("rest/list_current_orders_execution_complete.json");
    response["result"]["currentOrders"][2]["selectionId"] = Value::from(39980);
    state.betting_overrides.lock().insert(
        METHOD_LIST_CURRENT_ORDERS.to_string(),
        response["result"].clone(),
    );
    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, _rx, _data_rx, _cache) = create_test_execution_client(addr, stream_port);
    let (server_done_tx, server_done_rx) = tokio::sync::oneshot::channel();

    let server = tokio::spawn(async move {
        let (_reader, write_half) = accept_and_activate(&listener).await;
        let _ = server_done_rx.await;
        drop(write_half);
    });

    connect_execution_ready(&mut client).await;

    let command = GenerateFillReports::new(
        UUID4::new(),
        UnixNanos::default(),
        filter_instrument.then_some(InstrumentId::from("1.180575118-217709.BETFAIR")),
        filter_order.then_some(VenueOrderId::from("228059821049")),
        None,
        None,
        None,
        None,
    );
    let requested = client.generate_fill_reports(command).await.unwrap();

    let remaining = if mass_status {
        client
            .generate_mass_status(None)
            .await
            .unwrap()
            .unwrap()
            .fill_reports()
            .into_values()
            .flatten()
            .collect::<Vec<_>>()
    } else {
        client
            .generate_fill_reports(
                GenerateFillReportsBuilder::default()
                    .ts_init(UnixNanos::default())
                    .build()
                    .unwrap(),
            )
            .await
            .unwrap()
    };

    client.disconnect().await.unwrap();
    let _ = server_done_tx.send(());
    server.await.unwrap();

    assert_eq!(requested.len(), 1);
    assert_eq!(
        requested[0].venue_order_id,
        VenueOrderId::from("228059821049")
    );
    assert_eq!(
        requested[0].instrument_id,
        InstrumentId::from("1.180575118-217709.BETFAIR")
    );
    assert_eq!(requested[0].last_qty, Quantity::from("10.00"));
    assert_eq!(requested[0].last_px, Price::from("1.90"));
    assert_eq!(remaining.len(), 1);
    assert_eq!(
        remaining[0].venue_order_id,
        VenueOrderId::from("228059869313")
    );
    assert_eq!(
        remaining[0].instrument_id,
        InstrumentId::from("1.180575118-39980.BETFAIR")
    );
    assert_eq!(remaining[0].last_qty, Quantity::from("10.00"));
    assert_eq!(remaining[0].last_px, Price::from("1.92"));
}

#[rstest]
#[case::start_only(
    Some(1_000_000_000_000_000_000),
    None,
    Some("2001-09-09T01:46:40+00:00"),
    None
)]
#[case::end_only(
    None,
    Some(1_500_000_000_000_000_000),
    None,
    Some("2017-07-14T02:40:00+00:00")
)]
#[case::both(
    Some(1_000_000_000_000_000_000),
    Some(1_500_000_000_000_000_000),
    Some("2001-09-09T01:46:40+00:00"),
    Some("2017-07-14T02:40:00+00:00")
)]
#[tokio::test]
async fn test_generate_fill_reports_preserves_partial_date_range(
    #[case] start_ns: Option<u64>,
    #[case] end_ns: Option<u64>,
    #[case] expected_from: Option<&str>,
    #[case] expected_to: Option<&str>,
) {
    let (addr, state) = start_mock_http().await;
    let empty = load_json_fixture("rest/list_current_orders_empty.json");
    state.betting_overrides.lock().insert(
        METHOD_LIST_CURRENT_ORDERS.to_string(),
        empty["result"].clone(),
    );

    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, _rx, _data_rx, _cache) = create_test_execution_client(addr, stream_port);
    let (server_done_tx, server_done_rx) = tokio::sync::oneshot::channel();

    let server = tokio::spawn(async move {
        let (_reader, write_half) = accept_and_activate(&listener).await;
        let _ = server_done_rx.await;
        drop(write_half);
    });

    connect_execution_ready(&mut client).await;

    let cmd = GenerateFillReports::new(
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
        start_ns.map(UnixNanos::from),
        end_ns.map(UnixNanos::from),
        None,
        None,
    );
    let reports = client.generate_fill_reports(cmd).await.unwrap();
    let params = state
        .betting_request_params
        .lock()
        .iter()
        .filter(|(method, _)| method == METHOD_LIST_CURRENT_ORDERS)
        .map(|(_, params)| params.clone())
        .collect::<Vec<_>>();

    assert!(reports.is_empty());
    assert_eq!(params.len(), 1);
    assert_eq!(
        params[0]["dateRange"].get("from").and_then(Value::as_str),
        expected_from
    );
    assert_eq!(
        params[0]["dateRange"].get("to").and_then(Value::as_str),
        expected_to
    );

    client.disconnect().await.unwrap();
    let _ = server_done_tx.send(());
    server.await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_generate_reports_batches_market_ids_and_resets_pagination() {
    let (addr, state) = start_mock_http().await;
    let executable = load_json_fixture("rest/list_current_orders_executable.json");
    let executable_orders = executable["result"]["currentOrders"]
        .as_array()
        .expect("currentOrders must be an array");
    let completed = load_json_fixture("rest/list_current_orders_execution_complete.json");
    let completed_orders = completed["result"]["currentOrders"]
        .as_array()
        .expect("currentOrders must be an array");

    let current_orders_page = |orders: Vec<Value>, more_available: bool| {
        serde_json::json!({
            "currentOrders": orders,
            "moreAvailable": more_available,
        })
    };

    state.betting_response_sequences.lock().insert(
        METHOD_LIST_CURRENT_ORDERS.to_string(),
        VecDeque::from([
            current_orders_page(vec![executable_orders[1].clone()], true),
            current_orders_page(Vec::new(), false),
            current_orders_page(vec![executable_orders[0].clone()], false),
            current_orders_page(vec![completed_orders[2].clone()], true),
            current_orders_page(Vec::new(), false),
            current_orders_page(vec![completed_orders[1].clone()], false),
        ]),
    );

    let market_ids = (0..=250)
        .map(|index| format!("1.{index}"))
        .collect::<Vec<_>>();

    let config = BetfairExecutionClientConfig {
        reconcile_market_ids_only: true,
        reconcile_market_ids: Some(market_ids.clone()),
        ..Default::default()
    };

    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, _rx, _data_rx, _cache) =
        create_test_execution_client_with_config(addr, stream_port, config);
    let (server_done_tx, server_done_rx) = tokio::sync::oneshot::channel();

    let server = tokio::spawn(async move {
        let (_reader, write_half) = accept_and_activate(&listener).await;
        let _ = server_done_rx.await;
        drop(write_half);
    });

    connect_execution_ready(&mut client).await;

    let order_cmd = GenerateOrderStatusReportsBuilder::default()
        .ts_init(UnixNanos::default())
        .open_only(true)
        .build()
        .unwrap();
    let order_reports = client
        .generate_order_status_reports(&order_cmd)
        .await
        .unwrap();
    let fill_cmd = GenerateFillReportsBuilder::default()
        .ts_init(UnixNanos::default())
        .build()
        .unwrap();
    let fill_reports = client.generate_fill_reports(fill_cmd).await.unwrap();

    let params = state
        .betting_request_params
        .lock()
        .iter()
        .filter(|(method, _)| method == METHOD_LIST_CURRENT_ORDERS)
        .map(|(_, params)| params.clone())
        .collect::<Vec<_>>();
    let first_batch = serde_json::to_value(&market_ids[..250]).unwrap();
    let second_batch = serde_json::to_value(&market_ids[250..]).unwrap();
    let expected_batches = [
        &first_batch,
        &first_batch,
        &second_batch,
        &first_batch,
        &first_batch,
        &second_batch,
    ];
    let expected_from_records = [None, Some(1), None, None, Some(1), None];

    assert_eq!(params.len(), 6);

    for ((params, expected_batch), expected_from_record) in params
        .iter()
        .zip(expected_batches)
        .zip(expected_from_records)
    {
        assert_eq!(&params["marketIds"], expected_batch);

        match expected_from_record {
            Some(from_record) => assert_eq!(params["fromRecord"], from_record),
            None => assert!(params.get("fromRecord").is_none()),
        }
    }

    assert_eq!(params[0]["orderProjection"], "EXECUTABLE");
    assert_eq!(params[3]["orderProjection"], "ALL");
    assert_eq!(params[3]["orderBy"], "BY_MATCH_TIME");
    assert_eq!(params[3]["sortDir"], "EARLIEST_TO_LATEST");
    assert_eq!(
        order_reports
            .iter()
            .map(|report| report.venue_order_id.to_string())
            .collect::<Vec<_>>(),
        vec!["228059754671", "228059760965"],
    );
    assert_eq!(
        fill_reports
            .iter()
            .map(|report| report.venue_order_id.to_string())
            .collect::<Vec<_>>(),
        vec!["228059821049", "228059869313"],
    );

    state.betting_response_sequences.lock().insert(
        METHOD_LIST_CURRENT_ORDERS.to_string(),
        VecDeque::from([
            current_orders_page(vec![executable_orders[1].clone()], true),
            current_orders_page(vec![completed_orders[1].clone()], false),
            current_orders_page(vec![executable_orders[0].clone()], false),
        ]),
    );
    let mass_status = client.generate_mass_status(None).await.unwrap().unwrap();
    let snapshot_params = state
        .betting_request_params
        .lock()
        .iter()
        .filter(|(method, _)| method == METHOD_LIST_CURRENT_ORDERS)
        .skip(params.len())
        .map(|(_, params)| params.clone())
        .collect::<Vec<_>>();
    assert_eq!(mass_status.order_reports().len(), 3);
    assert_eq!(snapshot_params.len(), 3);

    for (params, expected_batch) in
        snapshot_params
            .iter()
            .zip([&first_batch, &first_batch, &second_batch])
    {
        assert_eq!(&params["marketIds"], expected_batch);
        assert_eq!(params["orderProjection"], "ALL");
        assert!(params.get("orderBy").is_none());
        assert!(params.get("dateRange").is_none());
    }

    assert!(snapshot_params[0].get("fromRecord").is_none());
    assert_eq!(snapshot_params[1]["fromRecord"], 1);
    assert!(snapshot_params[2].get("fromRecord").is_none());

    client.disconnect().await.unwrap();
    let _ = server_done_tx.send(());
    server.await.unwrap();
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
        .insert(METHOD_LIST_CURRENT_ORDERS.to_string(), v["result"].clone());

    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, _cache) = create_test_execution_client(addr, stream_port);

    let (server_done_tx, server_done_rx) = tokio::sync::oneshot::channel();

    let server = tokio::spawn(async move {
        let (_reader, write_half) = accept_and_activate(&listener).await;
        let _ = server_done_rx.await;
        drop(write_half);
    });

    connect_execution_ready(&mut client).await;

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
    let _ = server_done_tx.send(());
    server.await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_query_order_resolves_terminal_replacement_once() {
    let (addr, state) = start_mock_http().await;
    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, cache) = create_test_execution_client(addr, stream_port);
    let (server_done_tx, server_done_rx) = tokio::sync::oneshot::channel();

    let server = tokio::spawn(async move {
        let (_reader, write_half) = accept_and_activate(&listener).await;
        let _ = server_done_rx.await;
        drop(write_half);
    });

    let instrument_id = "1.179082386-235.BETFAIR";
    let client_order_id = "O-QUERY-REPLACE-CLOSED";
    let old_bet_id = "228302937743";
    let new_bet_id = "240808766933";
    let order = make_accepted_test_order(instrument_id, client_order_id, old_bet_id, "2.58", "10");
    add_order_to_cache(&cache, order);
    connect_execution_ready(&mut client).await;

    while rx.try_recv().is_ok() {}

    state
        .betting_status_overrides
        .lock()
        .insert(METHOD_REPLACE_ORDERS.to_string(), 502);
    client
        .modify_order(make_price_modify_order_cmd(
            instrument_id,
            client_order_id,
            old_bet_id,
            "3.00",
        ))
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        while betting_method_count(&state, METHOD_REPLACE_ORDERS) < 2 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("replace did not reach its second request");

    let mut new_leg = load_json_fixture("rest/list_current_orders_harness_canceled.json")["result"]
        ["currentOrders"][0]
        .clone();
    new_leg["betId"] = Value::from(new_bet_id);
    new_leg["marketId"] = Value::from("1.179082386");
    new_leg["selectionId"] = Value::from(235);
    new_leg["priceSize"]["price"] = Value::from(3.0);
    new_leg["customerOrderRef"] = Value::from(client_order_id);
    state.betting_overrides.lock().insert(
        METHOD_LIST_CURRENT_ORDERS.to_string(),
        serde_json::json!({
            "currentOrders": [new_leg],
            "moreAvailable": false,
        }),
    );

    client
        .query_order(QueryOrder::new(
            TraderId::from("TESTER-001"),
            Some(*BETFAIR_CLIENT_ID),
            StrategyId::from("S-001"),
            InstrumentId::from(instrument_id),
            ClientOrderId::from(client_order_id),
            Some(VenueOrderId::from(old_bet_id)),
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ))
        .unwrap();

    let update = match tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("timed out waiting for replacement update")
        .expect("execution event channel closed")
    {
        ExecutionEvent::Order(OrderEventAny::Updated(update)) => update,
        other => panic!("expected replacement update before terminal report, was {other:?}"),
    };

    let report = match tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("timed out waiting for terminal replacement report")
        .expect("execution event channel closed")
    {
        ExecutionEvent::Report(ExecutionReport::Order(report)) => *report,
        other => panic!("expected terminal replacement report after update, was {other:?}"),
    };

    let reconcile = GenerateOrderStatusReportsBuilder::default()
        .ts_init(UnixNanos::default())
        .open_only(false)
        .build()
        .unwrap();
    let repeated = client
        .generate_order_status_reports(&reconcile)
        .await
        .unwrap();

    assert_eq!(update.client_order_id, ClientOrderId::from(client_order_id));
    assert_eq!(update.strategy_id, StrategyId::from("S-001"));
    assert_eq!(update.venue_order_id, Some(VenueOrderId::from(new_bet_id)));
    assert_eq!(update.quantity, Quantity::from("10"));
    assert_eq!(update.price, Some(Price::from("3.00")));
    assert!(update.reconciliation);
    assert_eq!(
        report.client_order_id,
        Some(ClientOrderId::from(client_order_id))
    );
    assert_eq!(report.venue_order_id, VenueOrderId::from(new_bet_id));
    assert_eq!(report.order_status, OrderStatus::Canceled);
    assert_eq!(report.quantity, Quantity::from("10"));
    assert_eq!(report.price, Some(Price::from("3.00")));
    assert_eq!(repeated.len(), 1);
    assert_eq!(
        repeated[0].client_order_id,
        Some(ClientOrderId::from(client_order_id))
    );
    assert_eq!(repeated[0].venue_order_id, VenueOrderId::from(new_bet_id));
    assert_eq!(repeated[0].order_status, OrderStatus::Canceled);
    assert_eq!(repeated[0].quantity, Quantity::from("10"));
    assert_eq!(repeated[0].price, Some(Price::from("3.00")));
    assert!(rx.try_recv().is_err());

    client.disconnect().await.unwrap();
    let _ = server_done_tx.send(());
    server.await.unwrap();
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
        .insert(METHOD_LIST_CURRENT_ORDERS.to_string(), v["result"].clone());

    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, _cache) = create_test_execution_client(addr, stream_port);

    let (server_done_tx, server_done_rx) = tokio::sync::oneshot::channel();

    let server = tokio::spawn(async move {
        let (_reader, write_half) = accept_and_activate(&listener).await;
        let _ = server_done_rx.await;
        drop(write_half);
    });

    connect_execution_ready(&mut client).await;

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
    let _ = server_done_tx.send(());
    server.await.unwrap();
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
        .insert(METHOD_PLACE_ORDERS.to_string(), v["result"].clone());

    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, cache) = create_test_execution_client(addr, stream_port);

    let (server_done_tx, server_done_rx) = tokio::sync::oneshot::channel();

    let server = tokio::spawn(async move {
        let (_reader, write_half) = accept_and_activate(&listener).await;
        let _ = server_done_rx.await;
        drop(write_half);
    });

    connect_execution_ready(&mut client).await;

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

    let params = state
        .betting_request_params
        .lock()
        .iter()
        .find(|(method, _)| method == METHOD_PLACE_ORDERS)
        .cloned()
        .expect("placeOrders call must be recorded")
        .1;
    assert_valid_customer_ref(&params);
    assert_eq!(params["instructions"].as_array().unwrap().len(), 2);

    client.disconnect().await.unwrap();
    let _ = server_done_tx.send(());
    server.await.unwrap();
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
        .insert(METHOD_PLACE_ORDERS.to_string(), v["result"].clone());

    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, cache) = create_test_execution_client(addr, stream_port);

    let (server_done_tx, server_done_rx) = tokio::sync::oneshot::channel();

    let server = tokio::spawn(async move {
        let (_reader, write_half) = accept_and_activate(&listener).await;
        let _ = server_done_rx.await;
        drop(write_half);
    });

    connect_execution_ready(&mut client).await;

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
    let _ = server_done_tx.send(());
    server.await.unwrap();
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
        .insert(METHOD_CANCEL_ORDERS.to_string(), v["result"].clone());

    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, _cache) = create_test_execution_client(addr, stream_port);

    let (server_done_tx, server_done_rx) = tokio::sync::oneshot::channel();

    let server = tokio::spawn(async move {
        let (_reader, write_half) = accept_and_activate(&listener).await;
        let _ = server_done_rx.await;
        drop(write_half);
    });

    connect_execution_ready(&mut client).await;

    while rx.try_recv().is_ok() {}

    let cmd = make_batch_cancel_cmd(
        "1.179082386-235-0.BETFAIR",
        vec![
            (ClientOrderId::from("O-BC-1"), Some(VenueOrderId::from("1"))),
            (ClientOrderId::from("O-BC-2"), Some(VenueOrderId::from("2"))),
        ],
    );
    client.batch_cancel_orders(cmd).unwrap();

    wait_for_mock_state(&state, "METHOD_CANCEL_ORDERS request count >= 1", |state| {
        betting_method_count(state, METHOD_CANCEL_ORDERS) >= 1
    })
    .await;

    let params = state
        .betting_request_params
        .lock()
        .iter()
        .find(|(method, _)| method == METHOD_CANCEL_ORDERS)
        .cloned()
        .expect("cancelOrders call must be recorded")
        .1;
    assert_valid_customer_ref(&params);
    assert_eq!(params["instructions"].as_array().unwrap().len(), 2);

    tokio::time::sleep(Duration::from_millis(200)).await;
    let event = rx.try_recv();
    assert!(
        event.is_err(),
        "Successful batch-cancel should not emit rejected events, found: {event:?}"
    );

    client.disconnect().await.unwrap();
    let _ = server_done_tx.send(());
    server.await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_batch_cancel_orders_splits_more_than_sixty_instructions() {
    let (addr, state) = start_mock_http().await;

    let fixture = load_fixture("rest/betting_cancel_orders_batch_success.json");
    let response: Value = serde_json::from_str(&fixture).unwrap();
    state
        .betting_overrides
        .lock()
        .insert(METHOD_CANCEL_ORDERS.to_string(), response["result"].clone());

    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, _cache) = create_test_execution_client(addr, stream_port);
    let (server_done_tx, server_done_rx) = tokio::sync::oneshot::channel();

    let server = tokio::spawn(async move {
        let (_reader, write_half) = accept_and_activate(&listener).await;
        let _ = server_done_rx.await;
        drop(write_half);
    });

    connect_execution_ready(&mut client).await;

    while rx.try_recv().is_ok() {}

    let cancels = (0..61)
        .map(|index| {
            (
                ClientOrderId::from(format!("O-BC-{index:03}")),
                Some(VenueOrderId::from(format!("{}", 1_000 + index))),
            )
        })
        .collect();

    let cmd = make_batch_cancel_cmd("1.179082386-235-0.BETFAIR", cancels);
    client.batch_cancel_orders(cmd).unwrap();

    wait_for_mock_state(&state, "METHOD_CANCEL_ORDERS request count == 2", |state| {
        betting_method_count(state, METHOD_CANCEL_ORDERS) == 2
    })
    .await;

    let params = state
        .betting_request_params
        .lock()
        .iter()
        .filter(|(method, _)| method == METHOD_CANCEL_ORDERS)
        .map(|(_, params)| params.clone())
        .collect::<Vec<_>>();
    let instruction_counts = params
        .iter()
        .map(|params| params["instructions"].as_array().unwrap().len())
        .collect::<Vec<_>>();
    let actual_bet_ids = params
        .iter()
        .flat_map(|params| params["instructions"].as_array().unwrap())
        .map(|instruction| instruction["betId"].as_str().unwrap().to_string())
        .collect::<HashSet<_>>();
    let expected_bet_ids = (1_000..1_061)
        .map(|bet_id| bet_id.to_string())
        .collect::<HashSet<_>>();
    let customer_refs = params
        .iter()
        .map(assert_valid_customer_ref)
        .collect::<HashSet<_>>();

    assert_eq!(instruction_counts, vec![60, 1]);
    assert_eq!(actual_bet_ids, expected_bet_ids);
    assert_eq!(customer_refs.len(), 2);
    assert!(rx.try_recv().is_err());

    client.disconnect().await.unwrap();
    let _ = server_done_tx.send(());
    server.await.unwrap();
}

/// A mixed per-item batch result must emit CancelRejected for the explicit
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
        .insert(METHOD_CANCEL_ORDERS.to_string(), v["result"].clone());

    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, _cache) = create_test_execution_client(addr, stream_port);

    let (server_done_tx, server_done_rx) = tokio::sync::oneshot::channel();

    let server = tokio::spawn(async move {
        let (_reader, write_half) = accept_and_activate(&listener).await;
        let _ = server_done_rx.await;
        drop(write_half);
    });

    connect_execution_ready(&mut client).await;

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
    let _ = server_done_tx.send(());
    server.await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_batch_cancel_missing_instruction_report_stays_ambiguous() {
    let (addr, state) = start_mock_http().await;
    let fixture = load_fixture("rest/betting_cancel_orders_batch_partial_failure.json");
    let mut response: Value = serde_json::from_str(&fixture).unwrap();
    let mut failure = response["result"]["instructionReports"][1].clone();
    failure["instruction"]["betId"] = Value::String("1".to_string());
    response["result"]["instructionReports"] = Value::Array(vec![failure]);
    state
        .betting_overrides
        .lock()
        .insert(METHOD_CANCEL_ORDERS.to_string(), response["result"].clone());

    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, _cache) = create_test_execution_client(addr, stream_port);
    let (server_done_tx, server_done_rx) = tokio::sync::oneshot::channel();

    let server = tokio::spawn(async move {
        let (_reader, write_half) = accept_and_activate(&listener).await;
        let _ = server_done_rx.await;
        drop(write_half);
    });

    connect_execution_ready(&mut client).await;

    while rx.try_recv().is_ok() {}

    let cmd = make_batch_cancel_cmd(
        "1.179082386-235-0.BETFAIR",
        vec![
            (
                ClientOrderId::from("O-BC-REPORTED"),
                Some(VenueOrderId::from("1")),
            ),
            (
                ClientOrderId::from("O-BC-MISSING"),
                Some(VenueOrderId::from("2")),
            ),
        ],
    );
    client.batch_cancel_orders(cmd).unwrap();

    let mut rejected_ids = Vec::new();

    while let Ok(Some(event)) = tokio::time::timeout(Duration::from_millis(500), rx.recv()).await {
        if let ExecutionEvent::Order(OrderEventAny::CancelRejected(rejected)) = event {
            rejected_ids.push(rejected.client_order_id);
        }
    }

    assert_eq!(
        rejected_ids,
        vec![ClientOrderId::from("O-BC-REPORTED")],
        "an absent positional report must stay pending",
    );

    client.disconnect().await.unwrap();
    let _ = server_done_tx.send(());
    server.await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_batch_cancel_orders_definitive_failure_rejects_each_instruction_once() {
    let (addr, state) = start_mock_http().await;

    let fixture = load_fixture("rest/betting_cancel_orders_result_failure.json");
    let v: Value = serde_json::from_str(&fixture).unwrap();
    state
        .betting_overrides
        .lock()
        .insert(METHOD_CANCEL_ORDERS.to_string(), v["result"].clone());

    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, _cache) = create_test_execution_client(addr, stream_port);

    let (server_done_tx, server_done_rx) = tokio::sync::oneshot::channel();

    let server = tokio::spawn(async move {
        let (_reader, write_half) = accept_and_activate(&listener).await;
        let _ = server_done_rx.await;
        drop(write_half);
    });

    connect_execution_ready(&mut client).await;

    while rx.try_recv().is_ok() {}

    let cmd = make_batch_cancel_cmd(
        "1.179082386-235-0.BETFAIR",
        vec![
            (
                ClientOrderId::from("O-BC-WHOLE-1"),
                Some(VenueOrderId::from("1")),
            ),
            (
                ClientOrderId::from("O-BC-WHOLE-2"),
                Some(VenueOrderId::from("2")),
            ),
        ],
    );
    client.batch_cancel_orders(cmd).unwrap();

    wait_for_mock_state(&state, "METHOD_CANCEL_ORDERS request count >= 1", |state| {
        betting_method_count(state, METHOD_CANCEL_ORDERS) >= 1
    })
    .await;

    let mut rejected_ids: Vec<ClientOrderId> = Vec::new();

    while let Ok(Some(event)) = tokio::time::timeout(Duration::from_millis(500), rx.recv()).await {
        if let ExecutionEvent::Order(OrderEventAny::CancelRejected(rejected)) = event {
            rejected_ids.push(rejected.client_order_id);
        }
    }

    assert_eq!(
        rejected_ids,
        vec![
            ClientOrderId::from("O-BC-WHOLE-1"),
            ClientOrderId::from("O-BC-WHOLE-2"),
        ],
        "definitive batch failure must reject each instruction exactly once",
    );

    client.disconnect().await.unwrap();
    let _ = server_done_tx.send(());
    server.await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_batch_cancel_orders_ambiguous_5xx_emits_no_rejections() {
    let (addr, state) = start_mock_http().await;
    state
        .betting_status_overrides
        .lock()
        .insert(METHOD_CANCEL_ORDERS.to_string(), 502);

    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, _cache) = create_test_execution_client(addr, stream_port);

    let (server_done_tx, server_done_rx) = tokio::sync::oneshot::channel();

    let server = tokio::spawn(async move {
        let (_reader, write_half) = accept_and_activate(&listener).await;
        let _ = server_done_rx.await;
        drop(write_half);
    });

    connect_execution_ready(&mut client).await;

    while rx.try_recv().is_ok() {}

    let cmd = make_batch_cancel_cmd(
        "1.179082386-235-0.BETFAIR",
        vec![
            (
                ClientOrderId::from("O-BC-5XX-1"),
                Some(VenueOrderId::from("1")),
            ),
            (
                ClientOrderId::from("O-BC-5XX-2"),
                Some(VenueOrderId::from("2")),
            ),
        ],
    );
    client.batch_cancel_orders(cmd).unwrap();

    wait_for_mock_state(&state, "METHOD_CANCEL_ORDERS request count >= 1", |state| {
        betting_method_count(state, METHOD_CANCEL_ORDERS) >= 1
    })
    .await;

    tokio::time::sleep(Duration::from_secs(1)).await;

    let mut rejected_ids: Vec<ClientOrderId> = Vec::new();

    while let Ok(Some(event)) = tokio::time::timeout(Duration::from_millis(500), rx.recv()).await {
        if let ExecutionEvent::Order(OrderEventAny::CancelRejected(rejected)) = event {
            rejected_ids.push(rejected.client_order_id);
        }
    }

    assert!(
        rejected_ids.is_empty(),
        "ambiguous batch cancel 5xx must not emit CancelRejected, found: {rejected_ids:?}",
    );

    client.disconnect().await.unwrap();
    let _ = server_done_tx.send(());
    server.await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_batch_cancel_orders_missing_venue_id_emits_no_rejected_locally() {
    let (addr, state) = start_mock_http().await;
    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, _cache) = create_test_execution_client(addr, stream_port);

    let (server_done_tx, server_done_rx) = tokio::sync::oneshot::channel();

    let server = tokio::spawn(async move {
        let (_reader, write_half) = accept_and_activate(&listener).await;
        let _ = server_done_rx.await;
        drop(write_half);
    });

    connect_execution_ready(&mut client).await;

    while rx.try_recv().is_ok() {}

    let cmd = make_batch_cancel_cmd(
        "1.179082386-235-0.BETFAIR",
        vec![(ClientOrderId::from("O-BC-NO-ID"), None)],
    );
    client.batch_cancel_orders(cmd).unwrap();

    let mut rejected_seen = false;

    while let Ok(Some(event)) = tokio::time::timeout(Duration::from_millis(500), rx.recv()).await {
        if matches!(
            event,
            ExecutionEvent::Order(OrderEventAny::CancelRejected(_))
        ) {
            rejected_seen = true;
            break;
        }
    }

    assert!(
        !rejected_seen,
        "local batch cancel validation failure must not emit CancelRejected",
    );
    assert_eq!(state.betting_request_count.load(Ordering::Relaxed), 0);

    client.disconnect().await.unwrap();
    let _ = server_done_tx.send(());
    server.await.unwrap();
}

/// A modify with quantity reduction (no price change) must succeed without
/// emitting a ModifyRejected; the venue acknowledges via the OCM stream.
#[rstest]
#[tokio::test]
async fn test_modify_order_quantity_reduction_does_not_reject() {
    let (addr, state) = start_mock_http().await;

    // Reducing 10 -> 4 cancels 6; OrderUpdated is built from the actual
    // `sizeCancelled`, not the requested target, so a raced fill is not overfilled.
    let fixture = load_fixture("rest/betting_cancel_orders_success.json");
    let mut cancel: Value = serde_json::from_str(&fixture).unwrap();
    cancel["result"]["instructionReports"][0]["sizeCancelled"] = serde_json::json!(6.0);
    state
        .betting_overrides
        .lock()
        .insert(METHOD_CANCEL_ORDERS.to_string(), cancel["result"].clone());

    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, cache) = create_test_execution_client(addr, stream_port);

    let (server_done_tx, server_done_rx) = tokio::sync::oneshot::channel();

    let server = tokio::spawn(async move {
        let (_reader, write_half) = accept_and_activate(&listener).await;
        let _ = server_done_rx.await;
        drop(write_half);
    });

    connect_execution_ready(&mut client).await;

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

    wait_for_mock_state(&state, "METHOD_CANCEL_ORDERS request count >= 1", |state| {
        betting_method_count(state, METHOD_CANCEL_ORDERS) >= 1
    })
    .await;

    let params = state
        .betting_request_params
        .lock()
        .iter()
        .find(|(method, _)| method == METHOD_CANCEL_ORDERS)
        .cloned()
        .expect("cancelOrders call must be recorded")
        .1;
    assert_valid_customer_ref(&params);
    assert_eq!(params["instructions"][0]["sizeReduction"], "6");

    tokio::time::sleep(Duration::from_millis(200)).await;

    match rx.try_recv() {
        Ok(ExecutionEvent::Order(OrderEventAny::Updated(updated))) => {
            assert_eq!(updated.quantity, Quantity::from("4"));
            assert!(
                updated.price.is_none(),
                "quantity reduction must not change price, was: {:?}",
                updated.price
            );
        }
        other => panic!("successful quantity reduction must emit OrderUpdated, was: {other:?}"),
    }

    client.disconnect().await.unwrap();
    let _ = server_done_tx.send(());
    server.await.unwrap();
}

/// Modify cannot proceed without a `venue_order_id`. The command must
/// surface a synchronous error rather than silently dropping the request.
#[rstest]
#[tokio::test]
async fn test_modify_order_without_venue_id_returns_error() {
    let (addr, _state) = start_mock_http().await;
    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, _rx, _data_rx, cache) = create_test_execution_client(addr, stream_port);

    let (server_done_tx, server_done_rx) = tokio::sync::oneshot::channel();

    let server = tokio::spawn(async move {
        let (_reader, write_half) = accept_and_activate(&listener).await;
        let _ = server_done_rx.await;
        drop(write_half);
    });

    connect_execution_ready(&mut client).await;

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
    let _ = server_done_tx.send(());
    server.await.unwrap();
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

    let (server_done_tx, server_done_rx) = tokio::sync::oneshot::channel();

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

        let _ = server_done_rx.await;
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
    let _ = server_done_tx.send(());
    server.await.unwrap();
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

    let (server_done_tx, server_done_rx) = tokio::sync::oneshot::channel();

    let server = tokio::spawn(async move {
        let (mut reader, mut write_half) = accept_and_auth(&listener).await;

        let mut line = String::new();
        tokio::io::AsyncBufReadExt::read_line(&mut reader, &mut line)
            .await
            .unwrap();

        write_lines(&mut write_half, &frames).await;

        let _ = server_done_rx.await;
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
    let _ = server_done_tx.send(());
    server.await.unwrap();
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

    let (server_done_tx, server_done_rx) = tokio::sync::oneshot::channel();

    let server = tokio::spawn(async move {
        let (mut reader, mut write_half) = accept_and_auth(&listener).await;

        let mut line = String::new();
        tokio::io::AsyncBufReadExt::read_line(&mut reader, &mut line)
            .await
            .unwrap();

        write_lines(&mut write_half, &duplicated).await;

        let _ = server_done_rx.await;
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
    let _ = server_done_tx.send(());
    server.await.unwrap();
}

/// With `ignore_external_orders=true`, an unmatched order with no `rfo`
/// (no customer order ref, e.g. placed via the venue web UI) must be
/// silently skipped: no execution report, no fill.
#[rstest]
#[tokio::test]
async fn test_ocm_ignore_external_orders_skips_orders_without_rfo() {
    let (addr, _state) = start_mock_http().await;
    let (stream_port, listener) = start_mock_stream().await;

    let config = BetfairExecutionClientConfig::builder()
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

    let (server_done_tx, server_done_rx) = tokio::sync::oneshot::channel();

    let server = tokio::spawn(async move {
        let (mut reader, mut write_half) = accept_and_auth(&listener).await;

        let mut line = String::new();
        tokio::io::AsyncBufReadExt::read_line(&mut reader, &mut line)
            .await
            .unwrap();

        write_lines(&mut write_half, &[external_line]).await;

        let _ = server_done_rx.await;
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
    let _ = server_done_tx.send(());
    server.await.unwrap();
}

/// Regression: an empty `rfo` string must be treated identically to a missing
/// `rfo`. Parsers elsewhere normalize `""` to `None`; the
/// `ignore_external_orders` skip must do the same so externally-placed orders
/// (the venue sometimes emits `"rfo": ""`) are silently ignored.
#[rstest]
#[tokio::test]
async fn test_ocm_ignore_external_orders_skips_empty_string_rfo() {
    let (addr, _state) = start_mock_http().await;
    let (stream_port, listener) = start_mock_stream().await;

    let config = BetfairExecutionClientConfig::builder()
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

    let (server_done_tx, server_done_rx) = tokio::sync::oneshot::channel();

    let server = tokio::spawn(async move {
        let (mut reader, mut write_half) = accept_and_auth(&listener).await;

        let mut line = String::new();
        tokio::io::AsyncBufReadExt::read_line(&mut reader, &mut line)
            .await
            .unwrap();

        write_lines(&mut write_half, &[external_line]).await;

        let _ = server_done_rx.await;
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
    let _ = server_done_tx.send(());
    server.await.unwrap();
}

/// `stream_market_ids_filter` must drop OCMs for markets outside the filter
/// so multi-strategy deployments can isolate per-instance order streams.
#[rstest]
#[tokio::test]
async fn test_ocm_market_ids_filter_skips_unrelated_markets() {
    let (addr, _state) = start_mock_http().await;
    let (stream_port, listener) = start_mock_stream().await;

    let config = BetfairExecutionClientConfig::builder()
        .stream_market_ids_filter(vec!["1.OTHER".to_string()])
        .build();
    let (mut client, mut rx, _data_rx, _cache) =
        create_test_execution_client_with_config(addr, stream_port, config);

    // Multi-fill fixture targets market "1.179082386"; with the filter set to
    // "1.OTHER" the handler must drop every frame.
    let frames = load_fixture_frames("stream/ocm_multiple_fills.json");

    let (server_done_tx, server_done_rx) = tokio::sync::oneshot::channel();

    let server = tokio::spawn(async move {
        let (mut reader, mut write_half) = accept_and_auth(&listener).await;

        let mut line = String::new();
        tokio::io::AsyncBufReadExt::read_line(&mut reader, &mut line)
            .await
            .unwrap();

        write_lines(&mut write_half, &frames).await;

        let _ = server_done_rx.await;
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
    let _ = server_done_tx.send(());
    server.await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_cancel_order_without_venue_id_emits_no_rejected_locally() {
    let (addr, state) = start_mock_http().await;
    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, _cache) = create_test_execution_client(addr, stream_port);

    let (server_done_tx, server_done_rx) = tokio::sync::oneshot::channel();

    let server = tokio::spawn(async move {
        let (_reader, write_half) = accept_and_activate(&listener).await;
        let _ = server_done_rx.await;
        drop(write_half);
    });

    connect_execution_ready(&mut client).await;

    while rx.try_recv().is_ok() {}

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
    assert!(
        result.is_ok(),
        "cancel without venue_order_id must log and return"
    );

    let mut rejected_seen = false;

    while let Ok(Some(event)) = tokio::time::timeout(Duration::from_millis(500), rx.recv()).await {
        if matches!(
            event,
            ExecutionEvent::Order(OrderEventAny::CancelRejected(_))
        ) {
            rejected_seen = true;
            break;
        }
    }

    assert!(
        !rejected_seen,
        "local cancel validation failure must not emit CancelRejected",
    );
    assert_eq!(state.betting_request_count.load(Ordering::Relaxed), 0);

    client.disconnect().await.unwrap();
    let _ = server_done_tx.send(());
    server.await.unwrap();
}

/// A modify with a quantity *increase* (not allowed on Betfair) must emit a
/// ModifyRejected explaining the constraint. Only reductions are valid.
#[rstest]
#[tokio::test]
async fn test_modify_order_quantity_increase_rejects() {
    let (addr, _state) = start_mock_http().await;
    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, cache) = create_test_execution_client(addr, stream_port);

    let (server_done_tx, server_done_rx) = tokio::sync::oneshot::channel();

    let server = tokio::spawn(async move {
        let (_reader, write_half) = accept_and_activate(&listener).await;
        let _ = server_done_rx.await;
        drop(write_half);
    });

    connect_execution_ready(&mut client).await;

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
                rej.reason.contains("can only reduce quantity"),
                "expected reduce-only reason, was: {}",
                rej.reason,
            );
        }
        other => panic!("Expected ModifyRejected, was {other:?}"),
    }

    client.disconnect().await.unwrap();
    let _ = server_done_tx.send(());
    server.await.unwrap();
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

    let (server_done_tx, server_done_rx) = tokio::sync::oneshot::channel();

    let server = tokio::spawn(async move {
        let (_reader, write_half) = accept_and_activate(&listener).await;
        let _ = server_done_rx.await;
        drop(write_half);
    });

    connect_execution_ready(&mut client).await;

    while rx.try_recv().is_ok() {}

    let order = make_test_order("1.181005744-86362-1.5.BETFAIR", "O-HCAP", "2.58", "10");
    add_order_to_cache(&cache, order.clone());

    client.submit_order(make_submit_order_cmd(&order)).unwrap();

    wait_for_mock_state(&state, "METHOD_PLACE_ORDERS request count >= 1", |state| {
        betting_method_count(state, METHOD_PLACE_ORDERS) >= 1
    })
    .await;

    let params = state
        .betting_request_params
        .lock()
        .iter()
        .find(|(m, _)| m == METHOD_PLACE_ORDERS)
        .cloned()
        .expect("placeOrders call must be recorded")
        .1;

    let instr = &params["instructions"][0];
    assert_eq!(instr["selectionId"], 86362);
    // Decimals serialize as JSON strings; Betfair accepts the string form.
    assert_eq!(instr["handicap"], "1.5");

    client.disconnect().await.unwrap();
    let _ = server_done_tx.send(());
    server.await.unwrap();
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

    let (server_done_tx, server_done_rx) = tokio::sync::oneshot::channel();

    let server = tokio::spawn(async move {
        let (_reader, write_half) = accept_and_activate(&listener).await;
        let _ = server_done_rx.await;
        drop(write_half);
    });

    connect_execution_ready(&mut client).await;

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

    wait_for_mock_state(
        &state,
        "METHOD_REPLACE_ORDERS request count >= 1",
        |state| betting_method_count(state, METHOD_REPLACE_ORDERS) >= 1,
    )
    .await;

    let params = state
        .betting_request_params
        .lock()
        .iter()
        .find(|(m, _)| m == "SportsAPING/v1.0/replaceOrders")
        .cloned()
        .expect("replaceOrders call must be recorded")
        .1;

    let instr = &params["instructions"][0];
    assert_valid_customer_ref(&params);
    assert_eq!(instr["betId"], "228000000111");
    // Decimals serialize as JSON strings.
    assert_eq!(instr["newPrice"], "3.50");

    let event = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("timeout waiting for replace update")
        .expect("execution event channel closed");

    let ExecutionEvent::Order(OrderEventAny::Updated(updated)) = event else {
        panic!("successful replace must emit OrderUpdated, was {event:?}");
    };

    assert_eq!(updated.client_order_id, ClientOrderId::from("O-MOD-PX"));
    assert_eq!(
        updated.venue_order_id,
        Some(VenueOrderId::from("240808766933")),
    );
    assert_eq!(updated.price, Some(Price::from("3.50")));
    assert_eq!(updated.quantity, Quantity::from("10"));
    assert!(!updated.reconciliation);

    let methods = state.betting_methods.lock().clone();
    assert!(
        !methods.iter().any(|m| m == METHOD_PLACE_ORDERS),
        "price modify must not place a new order, only replace; saw: {methods:?}"
    );

    client.disconnect().await.unwrap();
    let _ = server_done_tx.send(());
    server.await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_modify_price_cancelled_not_placed_emits_canceled_once() {
    let (addr, state) = start_mock_http().await;
    let fixture = load_fixture("rest/betting_replace_orders_cancelled_not_placed_live.json");
    let response: Value = serde_json::from_str(&fixture).unwrap();
    state.betting_overrides.lock().insert(
        METHOD_REPLACE_ORDERS.to_string(),
        response["result"].clone(),
    );

    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, cache) = create_test_execution_client(addr, stream_port);
    let (server_done_tx, server_done_rx) = tokio::sync::oneshot::channel();

    let server = tokio::spawn(async move {
        let (_reader, write_half) = accept_and_activate(&listener).await;
        let _ = server_done_rx.await;
        drop(write_half);
    });

    connect_execution_ready(&mut client).await;

    while rx.try_recv().is_ok() {}

    let order = make_test_order("1.259300416-1096-0.BETFAIR", "O-MOD-FAIL", "990", "5");
    add_order_to_cache(&cache, order);
    client
        .modify_order(ModifyOrder::new(
            TraderId::from("TESTER-001"),
            Some(*BETFAIR_CLIENT_ID),
            StrategyId::from("S-001"),
            InstrumentId::from("1.259300416-1096-0.BETFAIR"),
            ClientOrderId::from("O-MOD-FAIL"),
            Some(VenueOrderId::from("439198201832")),
            None,
            Some(Price::from("2.57")),
            None,
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ))
        .unwrap();

    let event = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("timeout waiting for modify rejection")
        .expect("channel closed");

    match event {
        ExecutionEvent::Order(OrderEventAny::Canceled(canceled)) => {
            assert_eq!(canceled.client_order_id, ClientOrderId::from("O-MOD-FAIL"));
            assert_eq!(
                canceled.venue_order_id,
                Some(VenueOrderId::from("439198201832"))
            );
        }
        other => panic!("expected Canceled, found: {other:?}"),
    }

    assert!(
        tokio::time::timeout(Duration::from_millis(300), rx.recv())
            .await
            .is_err(),
        "partial replace failure must cancel exactly once",
    );

    client.disconnect().await.unwrap();
    let _ = server_done_tx.send(());
    server.await.unwrap();
}

#[rstest]
#[case::all(false)]
#[case::open(true)]
#[tokio::test]
async fn test_startup_restored_modify_price_ambiguous_5xx_resolves_from_http_reconciliation(
    #[case] open_only: bool,
) {
    let (addr, state) = start_mock_http().await;
    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, cache) = create_test_execution_client(addr, stream_port);

    let (server_done_tx, server_done_rx) = tokio::sync::oneshot::channel();

    let server = tokio::spawn(async move {
        let (_reader, write_half) = accept_and_activate(&listener).await;
        let _ = server_done_rx.await;
        drop(write_half);
    });

    let instrument_id = "1.179082386-235-0.BETFAIR";
    let client_order_id = "O-MOD-PX-RECON";
    let old_bet_id = "228302937743";
    let new_bet_id = "240808766933";
    let unrelated_bet_id = "228059760965";
    let order = make_accepted_test_order(instrument_id, client_order_id, old_bet_id, "2.58", "10");
    add_order_to_cache(&cache, order);

    connect_execution_ready(&mut client).await;

    while rx.try_recv().is_ok() {}

    state
        .betting_status_overrides
        .lock()
        .insert(METHOD_REPLACE_ORDERS.to_string(), 502);

    client
        .modify_order(make_price_modify_order_cmd(
            instrument_id,
            client_order_id,
            old_bet_id,
            "3.00",
        ))
        .unwrap();

    wait_for_mock_state(
        &state,
        "METHOD_REPLACE_ORDERS request count >= 2",
        |state| betting_method_count(state, METHOD_REPLACE_ORDERS) >= 2,
    )
    .await;

    let quiet = drain_events(&mut rx, Duration::from_millis(300)).await;
    assert!(
        quiet.is_empty(),
        "ambiguous replace must stay in flight without events, found: {quiet:?}",
    );
    assert_eq!(
        betting_method_count(&state, METHOD_REPLACE_ORDERS),
        2,
        "a customerRef makes the replace idempotent, so one 502 retries exactly once",
    );

    state
        .betting_status_overrides
        .lock()
        .remove(METHOD_REPLACE_ORDERS);

    let mut old_leg = load_json_fixture("rest/list_current_orders_harness_canceled.json")["result"]
        ["currentOrders"][0]
        .clone();
    old_leg["betId"] = Value::from(old_bet_id);
    old_leg["marketId"] = Value::from("1.179082386");
    old_leg["selectionId"] = Value::from(235);
    old_leg["priceSize"]["price"] = Value::from(2.58);
    old_leg["customerOrderRef"] = Value::from(client_order_id);

    let executable = load_json_fixture("rest/list_current_orders_executable.json");
    let mut new_leg = executable["result"]["currentOrders"][0].clone();
    new_leg["betId"] = Value::from(new_bet_id);
    new_leg["marketId"] = Value::from("1.179082386");
    new_leg["selectionId"] = Value::from(235);
    new_leg["priceSize"]["price"] = Value::from(3.0);
    new_leg["customerOrderRef"] = Value::from(client_order_id);

    let mut unrelated_leg = executable["result"]["currentOrders"][1].clone();
    unrelated_leg["betId"] = Value::from(unrelated_bet_id);
    unrelated_leg["customerOrderRef"] = Value::from("O-SOMEONE-ELSE");

    // List the superseded leg first to exercise order-independent resolution
    *state.betting_current_orders.lock() = Some(vec![old_leg, new_leg, unrelated_leg]);

    let reconcile = GenerateOrderStatusReportsBuilder::default()
        .ts_init(UnixNanos::default())
        .open_only(open_only)
        .build()
        .unwrap();
    let reports = client
        .generate_order_status_reports(&reconcile)
        .await
        .unwrap();

    let reported_bet_ids: HashSet<String> = reports
        .iter()
        .map(|report| report.venue_order_id.to_string())
        .collect();
    assert_eq!(
        reported_bet_ids,
        HashSet::from([unrelated_bet_id.to_string()]),
        "the resolving pass must withhold both replace legs and nothing else",
    );

    let events = drain_events(&mut rx, Duration::from_millis(300)).await;
    assert_no_accept_or_modify_reject(&events);
    let updates = order_updates(&events);
    assert_eq!(
        updates.len(),
        1,
        "reconciliation must promote the replacement exactly once, found: {events:?}",
    );
    let updated = updates[0];
    assert_eq!(
        updated.client_order_id,
        ClientOrderId::from(client_order_id)
    );
    assert_eq!(updated.venue_order_id, Some(VenueOrderId::from(new_bet_id)));
    assert_eq!(updated.price, Some(Price::from("3.00")));
    assert_eq!(updated.quantity, Quantity::from("10"));
    assert_eq!(
        updated.instrument_id,
        InstrumentId::from("1.179082386-235.BETFAIR")
    );
    assert!(updated.reconciliation);

    let repeated = client
        .generate_order_status_reports(&reconcile)
        .await
        .unwrap();
    let repeated_bet_ids: HashSet<String> = repeated
        .iter()
        .map(|report| report.venue_order_id.to_string())
        .collect();
    assert_eq!(
        repeated_bet_ids,
        HashSet::from([new_bet_id.to_string(), unrelated_bet_id.to_string()]),
        "once resolved, only the superseded leg stays withheld",
    );

    let settled = drain_events(&mut rx, Duration::from_millis(300)).await;
    assert_no_accept_or_modify_reject(&settled);
    assert!(
        settled.is_empty(),
        "a resolved replace must not be promoted twice, found: {settled:?}",
    );

    client.disconnect().await.unwrap();
    let _ = server_done_tx.send(());
    server.await.unwrap();
}

#[rstest]
#[case::all(false)]
#[case::open(true)]
#[tokio::test]
async fn test_startup_restored_ambiguous_replace_rejects_when_old_bet_stays_active(
    #[case] open_only: bool,
) {
    let (addr, state) = start_mock_http().await;
    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, cache) = create_test_execution_client(addr, stream_port);
    let (server_done_tx, server_done_rx) = tokio::sync::oneshot::channel();

    let server = tokio::spawn(async move {
        let (_reader, write_half) = accept_and_activate(&listener).await;
        let _ = server_done_rx.await;
        drop(write_half);
    });

    let instrument_id = "1.179082386-235-0.BETFAIR";
    let client_order_id = "O-MOD-PX-UNCHANGED";
    let old_bet_id = "228302937743";
    let order = make_accepted_test_order(instrument_id, client_order_id, old_bet_id, "2.58", "10");
    add_order_to_cache(&cache, order);

    connect_execution_ready(&mut client).await;

    while rx.try_recv().is_ok() {}

    state
        .betting_status_overrides
        .lock()
        .insert(METHOD_REPLACE_ORDERS.to_string(), 502);
    client
        .modify_order(make_price_modify_order_cmd(
            instrument_id,
            client_order_id,
            old_bet_id,
            "3.00",
        ))
        .unwrap();
    wait_for_mock_state(
        &state,
        "METHOD_REPLACE_ORDERS request count >= 2",
        |state| betting_method_count(state, METHOD_REPLACE_ORDERS) >= 2,
    )
    .await;

    let quiet = drain_events(&mut rx, Duration::from_millis(300)).await;
    assert!(
        quiet.is_empty(),
        "ambiguous replace must await reconciliation, found: {quiet:?}",
    );

    let executable = load_json_fixture("rest/list_current_orders_executable.json");
    let mut old_leg = executable["result"]["currentOrders"][0].clone();
    old_leg["betId"] = Value::from(old_bet_id);
    old_leg["marketId"] = Value::from("1.179082386");
    old_leg["selectionId"] = Value::from(235);
    old_leg["priceSize"]["price"] = Value::from(2.58);
    old_leg["customerOrderRef"] = Value::from(client_order_id);
    *state.betting_current_orders.lock() = Some(vec![old_leg]);

    let reconcile = GenerateOrderStatusReportsBuilder::default()
        .ts_init(UnixNanos::default())
        .open_only(open_only)
        .build()
        .unwrap();
    let reports = client
        .generate_order_status_reports(&reconcile)
        .await
        .unwrap();
    let event = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("timeout waiting for reconciled modify rejection")
        .expect("execution event channel closed");

    let ExecutionEvent::Order(OrderEventAny::ModifyRejected(rejected)) = event else {
        panic!("unchanged old bet must reject the ambiguous replace, was {event:?}");
    };

    assert_eq!(reports.len(), 1);
    assert_eq!(
        reports[0].client_order_id,
        Some(ClientOrderId::from(client_order_id))
    );
    assert_eq!(reports[0].venue_order_id, VenueOrderId::from(old_bet_id));
    assert_eq!(reports[0].order_status, OrderStatus::Accepted);
    assert_eq!(
        rejected.client_order_id,
        ClientOrderId::from(client_order_id)
    );
    assert_eq!(
        rejected.venue_order_id,
        Some(VenueOrderId::from(old_bet_id))
    );
    assert_eq!(
        rejected.reason,
        "Original bet remained executable after ambiguous replace",
    );
    assert!(rejected.reconciliation);

    let repeated = client
        .generate_order_status_reports(&reconcile)
        .await
        .unwrap();
    assert_eq!(repeated.len(), 1);
    assert_eq!(repeated[0].venue_order_id, VenueOrderId::from(old_bet_id));
    assert!(
        drain_events(&mut rx, Duration::from_millis(300))
            .await
            .is_empty(),
        "resolved ambiguous replace must reject exactly once",
    );

    client.disconnect().await.unwrap();
    let _ = server_done_tx.send(());
    server.await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_modify_price_reconciliation_keeps_in_flight_request_pending() {
    let (addr, state) = start_mock_http().await;
    state
        .betting_response_delays
        .lock()
        .insert(METHOD_REPLACE_ORDERS.to_string(), Duration::from_secs(1));

    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, cache) = create_test_execution_client(addr, stream_port);

    let (server_done_tx, server_done_rx) = tokio::sync::oneshot::channel();

    let server = tokio::spawn(async move {
        let (_reader, write_half) = accept_and_activate(&listener).await;
        let _ = server_done_rx.await;
        drop(write_half);
    });

    connect_execution_ready(&mut client).await;

    while rx.try_recv().is_ok() {}

    let instrument_id = "1.179082386-235-0.BETFAIR";
    let client_order_id = "O-MOD-PX-INFLIGHT";
    let old_bet_id = "228302937743";
    let new_bet_id = "240808766933";
    let order = make_test_order(instrument_id, client_order_id, "2.58", "10");
    add_order_to_cache(&cache, order.clone());
    submit_and_await_accept(&client, &mut rx, &order, old_bet_id).await;

    let mut unchanged =
        load_json_fixture("rest/list_current_orders_executable.json")["result"]["currentOrders"][0]
            .clone();
    unchanged["betId"] = Value::from(old_bet_id);
    unchanged["marketId"] = Value::from("1.179082386");
    unchanged["selectionId"] = Value::from(235);
    unchanged["priceSize"]["price"] = Value::from(2.58);
    unchanged["customerOrderRef"] = Value::from(client_order_id);
    state.betting_overrides.lock().insert(
        METHOD_LIST_CURRENT_ORDERS.to_string(),
        serde_json::json!({
            "currentOrders": [unchanged],
            "moreAvailable": false,
        }),
    );

    client
        .modify_order(make_price_modify_order_cmd(
            instrument_id,
            client_order_id,
            old_bet_id,
            "3.00",
        ))
        .unwrap();

    wait_for_mock_state(
        &state,
        "METHOD_REPLACE_ORDERS request count >= 1",
        |state| betting_method_count(state, METHOD_REPLACE_ORDERS) >= 1,
    )
    .await;

    let reconcile = GenerateOrderStatusReportsBuilder::default()
        .ts_init(UnixNanos::default())
        .open_only(false)
        .build()
        .unwrap();
    let reports = client
        .generate_order_status_reports(&reconcile)
        .await
        .unwrap();
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].venue_order_id, VenueOrderId::from(old_bet_id));
    assert_eq!(reports[0].quantity, Quantity::from("10"));
    assert!(
        tokio::time::timeout(Duration::from_millis(300), rx.recv())
            .await
            .is_err(),
        "reconciliation must not resolve a replace whose HTTP request is still running",
    );

    let event = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("timeout waiting for the delayed replace response")
        .expect("execution event channel closed");

    let ExecutionEvent::Order(OrderEventAny::Updated(updated)) = event else {
        panic!("in-flight replace must resolve as an update: {event:?}");
    };

    assert_eq!(
        updated.client_order_id,
        ClientOrderId::from(client_order_id)
    );
    assert_eq!(updated.venue_order_id, Some(VenueOrderId::from(new_bet_id)));
    assert_eq!(updated.price, Some(Price::from("3.00")));
    assert_eq!(updated.quantity, Quantity::from("10"));

    client.disconnect().await.unwrap();
    let _ = server_done_tx.send(());
    server.await.unwrap();
}

#[rstest]
#[case::all(false)]
#[case::open(true)]
#[tokio::test]
async fn test_startup_restored_modify_quantity_ambiguous_5xx_resolves_from_http_reconciliation(
    #[case] open_only: bool,
) {
    let (addr, state) = start_mock_http().await;
    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, cache) = create_test_execution_client(addr, stream_port);

    let (server_done_tx, server_done_rx) = tokio::sync::oneshot::channel();

    let server = tokio::spawn(async move {
        let (_reader, write_half) = accept_and_activate(&listener).await;
        let _ = server_done_rx.await;
        drop(write_half);
    });

    let instrument_id = "1.179082386-235-0.BETFAIR";
    let client_order_id = "O-MOD-QTY-RECON";
    let bet_id = "228302937743";
    let order = make_accepted_test_order(instrument_id, client_order_id, bet_id, "2.58", "10");
    add_order_to_cache(&cache, order);

    connect_execution_ready(&mut client).await;

    while rx.try_recv().is_ok() {}

    state
        .betting_status_overrides
        .lock()
        .insert(METHOD_CANCEL_ORDERS.to_string(), 502);

    client
        .modify_order(make_quantity_modify_order_cmd(
            instrument_id,
            client_order_id,
            bet_id,
            "4",
        ))
        .unwrap();

    wait_for_mock_state(&state, "METHOD_CANCEL_ORDERS request count >= 2", |state| {
        betting_method_count(state, METHOD_CANCEL_ORDERS) >= 2
    })
    .await;

    let quiet = drain_events(&mut rx, Duration::from_millis(300)).await;
    assert!(
        quiet.is_empty(),
        "ambiguous quantity reduction must stay in flight without events, found: {quiet:?}",
    );

    state
        .betting_status_overrides
        .lock()
        .remove(METHOD_CANCEL_ORDERS);

    let mut reduced =
        load_json_fixture("rest/list_current_orders_executable.json")["result"]["currentOrders"][0]
            .clone();
    reduced["betId"] = Value::from(bet_id);
    reduced["marketId"] = Value::from("1.179082386");
    reduced["selectionId"] = Value::from(235);
    reduced["priceSize"]["price"] = Value::from(2.58);
    reduced["sizeCancelled"] = Value::from(6.0);
    reduced["sizeRemaining"] = Value::from(4.0);
    reduced["customerOrderRef"] = Value::from(client_order_id);
    *state.betting_current_orders.lock() = Some(vec![reduced]);

    let reconcile = GenerateOrderStatusReportsBuilder::default()
        .ts_init(UnixNanos::default())
        .open_only(open_only)
        .build()
        .unwrap();
    let mut unrelated = reconcile.clone();
    unrelated.instrument_id = Some(InstrumentId::from("1.179082386-39980.BETFAIR"));
    let unrelated_reports = client
        .generate_order_status_reports(&unrelated)
        .await
        .unwrap();

    assert!(unrelated_reports.is_empty());
    assert!(rx.try_recv().is_err());

    let reports = client
        .generate_order_status_reports(&reconcile)
        .await
        .unwrap();
    assert!(
        reports.is_empty(),
        "the resolving pass must leave the reduction to the direct event, found: {reports:?}",
    );

    let events = drain_events(&mut rx, Duration::from_millis(300)).await;
    assert_no_accept_or_modify_reject(&events);
    let updates = order_updates(&events);
    assert_eq!(
        updates.len(),
        1,
        "reconciliation must apply the reduction exactly once, found: {events:?}",
    );
    let updated = updates[0];
    assert_eq!(
        updated.client_order_id,
        ClientOrderId::from(client_order_id)
    );
    assert_eq!(updated.venue_order_id, Some(VenueOrderId::from(bet_id)));
    assert_eq!(updated.quantity, Quantity::from("4"));
    assert_eq!(updated.price, None);
    assert!(updated.reconciliation);

    let repeated = client
        .generate_order_status_reports(&reconcile)
        .await
        .unwrap();
    assert_eq!(repeated.len(), 1);
    assert_eq!(
        repeated[0].client_order_id,
        Some(ClientOrderId::from(client_order_id))
    );
    assert_eq!(repeated[0].venue_order_id, VenueOrderId::from(bet_id));
    assert_eq!(
        repeated[0].quantity,
        Quantity::from("4"),
        "the report must carry the reduced size, not the venue's original stake",
    );
    assert_eq!(repeated[0].price, Some(Price::from("2.58")));

    let settled = drain_events(&mut rx, Duration::from_millis(300)).await;
    assert_no_accept_or_modify_reject(&settled);
    assert!(
        settled.is_empty(),
        "a resolved reduction must not update twice, found: {settled:?}",
    );

    client.disconnect().await.unwrap();
    let _ = server_done_tx.send(());
    server.await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_startup_restored_modify_quantity_ambiguous_5xx_resolves_from_ocm() {
    let (addr, state) = start_mock_http().await;
    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, cache) = create_test_execution_client(addr, stream_port);
    let (ocm_tx, mut ocm_rx) = tokio::sync::mpsc::unbounded_channel::<String>();

    let server = tokio::spawn(async move {
        let (_reader, mut write_half) = accept_and_activate(&listener).await;

        while let Some(line) = ocm_rx.recv().await {
            tokio::io::AsyncWriteExt::write_all(&mut write_half, format!("{line}\r\n").as_bytes())
                .await
                .unwrap();
        }
    });

    let instrument_id = "1.179082386-235-0.BETFAIR";
    let client_order_id = "O-MOD-QTY-OCM";
    let bet_id = "228302937743";
    let order = make_accepted_test_order(instrument_id, client_order_id, bet_id, "2.58", "10");
    add_order_to_cache(&cache, order);

    connect_execution_ready(&mut client).await;

    while rx.try_recv().is_ok() {}

    state
        .betting_status_overrides
        .lock()
        .insert(METHOD_CANCEL_ORDERS.to_string(), 502);

    client
        .modify_order(make_quantity_modify_order_cmd(
            instrument_id,
            client_order_id,
            bet_id,
            "4",
        ))
        .unwrap();

    wait_for_mock_state(&state, "METHOD_CANCEL_ORDERS request count >= 2", |state| {
        betting_method_count(state, METHOD_CANCEL_ORDERS) >= 2
    })
    .await;

    let quiet = drain_events(&mut rx, Duration::from_millis(300)).await;
    assert!(
        quiet.is_empty(),
        "ambiguous quantity reduction must stay in flight without events, found: {quiet:?}",
    );

    let mut ocm = load_json_fixture("stream/ocm_harness_open.json");
    ocm["id"] = Value::from(2);
    ocm["oc"][0]["id"] = Value::from("1.179082386");
    ocm["oc"][0]["orc"][0]["id"] = Value::from(235);
    let unmatched = &mut ocm["oc"][0]["orc"][0]["uo"][0];
    unmatched["id"] = Value::from(bet_id);
    unmatched["p"] = Value::from(2.58);
    unmatched["side"] = Value::from("L");
    unmatched["sr"] = Value::from(4.0);
    unmatched["sc"] = Value::from(6.0);
    unmatched["rfo"] = Value::from(client_order_id);
    ocm_tx.send(ocm.to_string()).unwrap();

    let events = drain_events(&mut rx, Duration::from_millis(500)).await;
    assert_no_accept_or_modify_reject(&events);
    let updates = order_updates(&events);
    assert_eq!(
        updates.len(),
        1,
        "OCM must apply the reduction exactly once, found: {events:?}",
    );
    let updated = updates[0];
    assert_eq!(
        updated.client_order_id,
        ClientOrderId::from(client_order_id)
    );
    assert_eq!(updated.venue_order_id, Some(VenueOrderId::from(bet_id)));
    assert_eq!(updated.quantity, Quantity::from("4"));
    assert_eq!(updated.price, None);

    ocm["id"] = Value::from(3);
    ocm_tx.send(ocm.to_string()).unwrap();
    let settled = drain_events(&mut rx, Duration::from_millis(300)).await;
    assert_no_accept_or_modify_reject(&settled);
    assert!(
        order_updates(&settled).is_empty(),
        "repeated OCM must not apply the reduction twice, found: {settled:?}",
    );

    drop(ocm_tx);
    client.disconnect().await.unwrap();
    server.await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_modify_price_instruction_failure_rejects() {
    let (addr, state) = start_mock_http().await;
    let mut replace = load_json_fixture("rest/betting_replace_orders_success.json");
    replace["result"]["status"] = Value::from("FAILURE");
    replace["result"]["errorCode"] = Value::from("PROCESSED_WITH_ERRORS");
    let instruction = &mut replace["result"]["instructionReports"][0];
    instruction["status"] = Value::from("FAILURE");
    instruction["errorCode"] = Value::from("INVALID_ODDS");
    instruction
        .as_object_mut()
        .unwrap()
        .remove("cancelInstructionReport");
    instruction
        .as_object_mut()
        .unwrap()
        .remove("placeInstructionReport");
    state
        .betting_overrides
        .lock()
        .insert(METHOD_REPLACE_ORDERS.to_string(), replace["result"].clone());

    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, cache) = create_test_execution_client(addr, stream_port);

    let (server_done_tx, server_done_rx) = tokio::sync::oneshot::channel();

    let server = tokio::spawn(async move {
        let (_reader, write_half) = accept_and_activate(&listener).await;
        let _ = server_done_rx.await;
        drop(write_half);
    });

    connect_execution_ready(&mut client).await;

    while rx.try_recv().is_ok() {}

    let instrument_id = "1.179082386-235-0.BETFAIR";
    let client_order_id = "O-MOD-PX-REJECT";
    let venue_order_id = "123";
    add_order_to_cache(
        &cache,
        make_test_order(instrument_id, client_order_id, "2.58", "10"),
    );

    client
        .modify_order(make_price_modify_order_cmd(
            instrument_id,
            client_order_id,
            venue_order_id,
            "3.00",
        ))
        .unwrap();

    let events = drain_events(&mut rx, Duration::from_secs(1)).await;

    let rejections: Vec<_> = events
        .iter()
        .filter_map(|event| match event {
            ExecutionEvent::Order(OrderEventAny::ModifyRejected(rejected)) => Some(rejected),
            _ => None,
        })
        .collect();

    assert_eq!(
        rejections.len(),
        1,
        "a failed replace instruction must reject exactly once, found: {events:?}",
    );
    assert_eq!(
        events.len(),
        1,
        "no other event may follow, found: {events:?}"
    );
    assert_eq!(
        rejections[0].client_order_id,
        ClientOrderId::from(client_order_id)
    );
    assert_eq!(
        rejections[0].venue_order_id,
        Some(VenueOrderId::from(venue_order_id))
    );
    assert_eq!(rejections[0].reason, "InvalidOdds");
    assert_eq!(betting_method_count(&state, METHOD_REPLACE_ORDERS), 1);

    client.disconnect().await.unwrap();
    let _ = server_done_tx.send(());
    server.await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_modify_quantity_instruction_failure_rejects() {
    let (addr, state) = start_mock_http().await;
    let cancel = load_json_fixture("rest/betting_cancel_orders_error.json");
    state
        .betting_overrides
        .lock()
        .insert(METHOD_CANCEL_ORDERS.to_string(), cancel["result"].clone());

    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, cache) = create_test_execution_client(addr, stream_port);

    let (server_done_tx, server_done_rx) = tokio::sync::oneshot::channel();

    let server = tokio::spawn(async move {
        let (_reader, write_half) = accept_and_activate(&listener).await;
        let _ = server_done_rx.await;
        drop(write_half);
    });

    connect_execution_ready(&mut client).await;

    while rx.try_recv().is_ok() {}

    let instrument_id = "1.179082386-235-0.BETFAIR";
    let client_order_id = "O-MOD-QTY-REJECT";
    let venue_order_id = "1";
    add_order_to_cache(
        &cache,
        make_test_order(instrument_id, client_order_id, "2.58", "10"),
    );

    client
        .modify_order(make_quantity_modify_order_cmd(
            instrument_id,
            client_order_id,
            venue_order_id,
            "4",
        ))
        .unwrap();

    let events = drain_events(&mut rx, Duration::from_secs(1)).await;

    let rejections: Vec<_> = events
        .iter()
        .filter_map(|event| match event {
            ExecutionEvent::Order(OrderEventAny::ModifyRejected(rejected)) => Some(rejected),
            _ => None,
        })
        .collect();

    assert_eq!(
        rejections.len(),
        1,
        "a failed reduction instruction must reject exactly once, found: {events:?}",
    );
    assert_eq!(
        events.len(),
        1,
        "no other event may follow, found: {events:?}"
    );
    assert_eq!(
        rejections[0].client_order_id,
        ClientOrderId::from(client_order_id)
    );
    assert_eq!(
        rejections[0].venue_order_id,
        Some(VenueOrderId::from(venue_order_id))
    );
    assert_eq!(rejections[0].reason, "ErrorInOrder");
    assert_eq!(betting_method_count(&state, METHOD_CANCEL_ORDERS), 1);

    client.disconnect().await.unwrap();
    let _ = server_done_tx.send(());
    server.await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_reduction_success_after_a_terminal_stream_update_stays_silent() {
    let (addr, state) = start_mock_http().await;
    let mut cancel = load_json_fixture("rest/betting_cancel_orders_success.json");
    cancel["result"]["instructionReports"][0]["sizeCancelled"] = Value::from(6.0);
    state
        .betting_overrides
        .lock()
        .insert(METHOD_CANCEL_ORDERS.to_string(), cancel["result"].clone());

    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, cache) = create_test_execution_client(addr, stream_port);
    let (ocm_tx, mut ocm_rx) = tokio::sync::mpsc::unbounded_channel::<String>();

    let server = tokio::spawn(async move {
        let (_reader, mut write_half) = accept_and_activate(&listener).await;

        while let Some(line) = ocm_rx.recv().await {
            tokio::io::AsyncWriteExt::write_all(&mut write_half, format!("{line}\r\n").as_bytes())
                .await
                .unwrap();
        }
    });

    connect_execution_ready(&mut client).await;

    while rx.try_recv().is_ok() {}

    let instrument_id = "1.179082386-235-0.BETFAIR";
    let client_order_id = "O-MOD-QTY-CLOSED";
    let bet_id = "228302937743";
    let order = make_test_order(instrument_id, client_order_id, "2.58", "10");
    add_order_to_cache(&cache, order.clone());
    submit_and_await_accept(&client, &mut rx, &order, bet_id).await;

    let waiters = Arc::new(AtomicUsize::new(0));
    let semaphore = Arc::new(tokio::sync::Semaphore::new(0));
    *state.betting_response_gate.lock() = Some(MockResponseGate {
        method: METHOD_CANCEL_ORDERS.to_string(),
        waiters: Arc::clone(&waiters),
        semaphore: Arc::clone(&semaphore),
    });

    client
        .modify_order(make_quantity_modify_order_cmd(
            instrument_id,
            client_order_id,
            bet_id,
            "4",
        ))
        .unwrap();

    wait_for_mock_state(&state, "response gate waiter count 1", |state| {
        response_gate_waiter_count(state) == 1
    })
    .await;

    // Lapse more than requested to close the bet at two matched
    let mut ocm = load_json_fixture("stream/ocm_harness_cancel.json");
    ocm["oc"][0]["id"] = Value::from("1.179082386");
    ocm["oc"][0]["orc"][0]["id"] = Value::from(235);
    let unmatched = &mut ocm["oc"][0]["orc"][0]["uo"][0];
    unmatched["id"] = Value::from(bet_id);
    unmatched["p"] = Value::from(2.58);
    unmatched["side"] = Value::from("L");
    unmatched["sm"] = Value::from(2.0);
    unmatched["sr"] = Value::from(0.0);
    unmatched["sc"] = Value::from(8.0);
    unmatched["avp"] = Value::from(2.58);
    unmatched["rfo"] = Value::from(client_order_id);
    ocm_tx.send(ocm.to_string()).unwrap();

    let stream_events = drain_events(&mut rx, Duration::from_millis(500)).await;
    assert!(
        stream_events
            .iter()
            .any(|event| matches!(event, ExecutionEvent::Order(OrderEventAny::Canceled(_)))),
        "the closing stream update must end the order, found: {stream_events:?}",
    );
    assert!(
        order_updates(&stream_events).is_empty(),
        "an active size below the requested one is a lapse, not the reduction: {stream_events:?}",
    );

    semaphore.add_permits(1);
    state.betting_response_gate.lock().take();

    let late_events = drain_events(&mut rx, Duration::from_millis(500)).await;
    assert!(
        late_events.is_empty(),
        "a reduction success must not reopen a closed order: {late_events:?}",
    );

    drop(ocm_tx);
    client.disconnect().await.unwrap();
    server.await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_modify_quantity_reduction_survives_a_terminal_report() {
    let (addr, state) = start_mock_http().await;
    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, cache) = create_test_execution_client(addr, stream_port);
    let (ocm_tx, mut ocm_rx) = tokio::sync::mpsc::unbounded_channel::<String>();

    let server = tokio::spawn(async move {
        let (_reader, mut write_half) = accept_and_activate(&listener).await;

        while let Some(line) = ocm_rx.recv().await {
            tokio::io::AsyncWriteExt::write_all(&mut write_half, format!("{line}\r\n").as_bytes())
                .await
                .unwrap();
        }
    });

    connect_execution_ready(&mut client).await;

    while rx.try_recv().is_ok() {}

    let instrument_id = "1.179082386-235-0.BETFAIR";
    let client_order_id = "O-20240101-550e8400-e29b-41d4-a716-446655440000";
    let customer_order_ref = make_customer_order_ref(client_order_id);
    let bet_id = "228302937743";
    let order = make_test_order(instrument_id, client_order_id, "2.58", "10");
    add_order_to_cache(&cache, order.clone());
    submit_and_await_accept(&client, &mut rx, &order, bet_id).await;

    state
        .betting_status_overrides
        .lock()
        .insert(METHOD_CANCEL_ORDERS.to_string(), 502);

    client
        .modify_order(make_quantity_modify_order_cmd(
            instrument_id,
            client_order_id,
            bet_id,
            "4",
        ))
        .unwrap();

    wait_for_mock_state(&state, "METHOD_CANCEL_ORDERS request count >= 2", |state| {
        betting_method_count(state, METHOD_CANCEL_ORDERS) >= 2
    })
    .await;

    state
        .betting_status_overrides
        .lock()
        .remove(METHOD_CANCEL_ORDERS);

    let mut ocm = load_json_fixture("stream/ocm_harness_cancel.json");
    ocm["oc"][0]["id"] = Value::from("1.179082386");
    ocm["oc"][0]["orc"][0]["id"] = Value::from(235);
    let unmatched = &mut ocm["oc"][0]["orc"][0]["uo"][0];
    unmatched["id"] = Value::from(bet_id);
    unmatched["p"] = Value::from(2.58);
    unmatched["sm"] = Value::from(4.0);
    unmatched["sr"] = Value::from(0.0);
    unmatched["sc"] = Value::from(6.0);
    unmatched["avp"] = Value::from(2.58);
    unmatched["rfo"] = Value::from(customer_order_ref.as_str());
    ocm_tx.send(ocm.to_string()).unwrap();

    let events = drain_events(&mut rx, Duration::from_millis(500)).await;
    let updates = order_updates(&events);
    assert_eq!(updates.len(), 1, "found: {events:?}");
    assert_eq!(
        updates[0].client_order_id,
        ClientOrderId::from(client_order_id),
    );
    assert_eq!(updates[0].quantity, Quantity::from("4"));

    // Cancel the unmatched six while `priceSize.size` retains the original ten
    let mut closed =
        load_json_fixture("rest/list_current_orders_executable.json")["result"]["currentOrders"][0]
            .clone();
    closed["betId"] = Value::from(bet_id);
    closed["marketId"] = Value::from("1.179082386");
    closed["selectionId"] = Value::from(235);
    closed["priceSize"]["price"] = Value::from(2.58);
    closed["status"] = Value::from("EXECUTION_COMPLETE");
    closed["sizeMatched"] = Value::from(4.0);
    closed["averagePriceMatched"] = Value::from(2.58);
    closed["sizeRemaining"] = Value::from(0.0);
    closed["sizeCancelled"] = Value::from(6.0);
    closed["customerOrderRef"] = Value::from(customer_order_ref);
    state.betting_overrides.lock().insert(
        METHOD_LIST_CURRENT_ORDERS.to_string(),
        serde_json::json!({
            "currentOrders": [closed],
            "moreAvailable": false,
        }),
    );

    let reconcile = GenerateOrderStatusReportsBuilder::default()
        .ts_init(UnixNanos::default())
        .open_only(false)
        .build()
        .unwrap();
    let reports = client
        .generate_order_status_reports(&reconcile)
        .await
        .unwrap();
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].venue_order_id, VenueOrderId::from(bet_id));
    assert_eq!(reports[0].order_status, OrderStatus::Canceled);
    assert_eq!(
        reports[0].quantity,
        Quantity::from("4"),
        "a terminal report must not restore the venue's original stake",
    );
    assert_eq!(reports[0].filled_qty, Quantity::from("4"));

    let settled = drain_events(&mut rx, Duration::from_millis(300)).await;
    assert!(settled.is_empty(), "found: {settled:?}");

    drop(ocm_tx);
    client.disconnect().await.unwrap();
    server.await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_reduction_stream_before_rest_emits_updated_once() {
    let (addr, state) = start_mock_http().await;
    let mut cancel = load_json_fixture("rest/betting_cancel_orders_success.json");
    cancel["result"]["instructionReports"][0]["sizeCancelled"] = Value::from(6.0);
    state
        .betting_overrides
        .lock()
        .insert(METHOD_CANCEL_ORDERS.to_string(), cancel["result"].clone());

    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, cache) = create_test_execution_client(addr, stream_port);
    let (ocm_tx, mut ocm_rx) = tokio::sync::mpsc::unbounded_channel::<String>();

    let server = tokio::spawn(async move {
        let (_reader, mut write_half) = accept_and_activate(&listener).await;

        while let Some(line) = ocm_rx.recv().await {
            tokio::io::AsyncWriteExt::write_all(&mut write_half, format!("{line}\r\n").as_bytes())
                .await
                .unwrap();
        }
    });

    connect_execution_ready(&mut client).await;

    while rx.try_recv().is_ok() {}

    let instrument_id = "1.179082386-235-0.BETFAIR";
    let client_order_id = "O-MOD-QTY-RACE";
    let bet_id = "228302937743";
    let order = make_test_order(instrument_id, client_order_id, "2.58", "10");
    add_order_to_cache(&cache, order.clone());
    submit_and_await_accept(&client, &mut rx, &order, bet_id).await;

    // Hold the REST response so OCM resolves first
    let waiters = Arc::new(AtomicUsize::new(0));
    let semaphore = Arc::new(tokio::sync::Semaphore::new(0));
    *state.betting_response_gate.lock() = Some(MockResponseGate {
        method: METHOD_CANCEL_ORDERS.to_string(),
        waiters: Arc::clone(&waiters),
        semaphore: Arc::clone(&semaphore),
    });

    client
        .modify_order(make_quantity_modify_order_cmd(
            instrument_id,
            client_order_id,
            bet_id,
            "4",
        ))
        .unwrap();

    wait_for_mock_state(&state, "response gate waiter count 1", |state| {
        response_gate_waiter_count(state) == 1
    })
    .await;

    let mut ocm = load_json_fixture("stream/ocm_harness_open.json");
    ocm["id"] = Value::from(2);
    ocm["oc"][0]["id"] = Value::from("1.179082386");
    ocm["oc"][0]["orc"][0]["id"] = Value::from(235);
    let unmatched = &mut ocm["oc"][0]["orc"][0]["uo"][0];
    unmatched["id"] = Value::from(bet_id);
    unmatched["p"] = Value::from(2.58);
    unmatched["side"] = Value::from("L");
    unmatched["sr"] = Value::from(4.0);
    unmatched["sc"] = Value::from(6.0);
    unmatched["rfo"] = Value::from(client_order_id);
    ocm_tx.send(ocm.to_string()).unwrap();

    let stream_events = drain_events(&mut rx, Duration::from_millis(500)).await;
    let stream_updates = order_updates(&stream_events);
    assert_eq!(
        stream_updates.len(),
        1,
        "the stream must resolve the reduction once, found: {stream_events:?}",
    );
    assert_eq!(stream_updates[0].quantity, Quantity::from("4"));

    semaphore.add_permits(1);
    state.betting_response_gate.lock().take();

    let late_events = drain_events(&mut rx, Duration::from_millis(500)).await;
    assert!(
        late_events.is_empty(),
        "the REST success duplicated the stream-resolved reduction: {late_events:?}",
    );

    drop(ocm_tx);
    client.disconnect().await.unwrap();
    server.await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_modify_quantity_rejection_discards_the_pending_reduction() {
    let (addr, state) = start_mock_http().await;
    let cancel = load_json_fixture("rest/betting_cancel_orders_error.json");
    state
        .betting_overrides
        .lock()
        .insert(METHOD_CANCEL_ORDERS.to_string(), cancel["result"].clone());

    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, cache) = create_test_execution_client(addr, stream_port);

    let (server_done_tx, server_done_rx) = tokio::sync::oneshot::channel();

    let server = tokio::spawn(async move {
        let (_reader, write_half) = accept_and_activate(&listener).await;
        let _ = server_done_rx.await;
        drop(write_half);
    });

    connect_execution_ready(&mut client).await;

    while rx.try_recv().is_ok() {}

    let instrument_id = "1.179082386-235-0.BETFAIR";
    let client_order_id = "O-MOD-QTY-DISCARD";
    let bet_id = "228302937743";
    let order = make_test_order(instrument_id, client_order_id, "2.58", "10");
    add_order_to_cache(&cache, order.clone());
    submit_and_await_accept(&client, &mut rx, &order, bet_id).await;

    client
        .modify_order(make_quantity_modify_order_cmd(
            instrument_id,
            client_order_id,
            bet_id,
            "4",
        ))
        .unwrap();

    let events = drain_events(&mut rx, Duration::from_secs(1)).await;
    assert!(
        matches!(
            events.as_slice(),
            [ExecutionEvent::Order(OrderEventAny::ModifyRejected(_))]
        ),
        "the reduction must be rejected once, found: {events:?}",
    );

    // Leave the same active size as the rejected reduction
    let mut lapsed =
        load_json_fixture("rest/list_current_orders_executable.json")["result"]["currentOrders"][0]
            .clone();
    lapsed["betId"] = Value::from(bet_id);
    lapsed["marketId"] = Value::from("1.179082386");
    lapsed["selectionId"] = Value::from(235);
    lapsed["priceSize"]["price"] = Value::from(2.58);
    lapsed["sizeRemaining"] = Value::from(5.0);
    lapsed["sizeLapsed"] = Value::from(5.0);
    lapsed["customerOrderRef"] = Value::from(client_order_id);
    state.betting_overrides.lock().insert(
        METHOD_LIST_CURRENT_ORDERS.to_string(),
        serde_json::json!({
            "currentOrders": [lapsed],
            "moreAvailable": false,
        }),
    );

    let reconcile = GenerateOrderStatusReportsBuilder::default()
        .ts_init(UnixNanos::default())
        .open_only(false)
        .build()
        .unwrap();
    let reports = client
        .generate_order_status_reports(&reconcile)
        .await
        .unwrap();
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].venue_order_id, VenueOrderId::from(bet_id));
    assert_eq!(reports[0].quantity, Quantity::from("10"));

    let settled = drain_events(&mut rx, Duration::from_millis(300)).await;
    assert!(
        settled.is_empty(),
        "a rejected reduction must not resolve from a later lapse, found: {settled:?}",
    );

    client.disconnect().await.unwrap();
    let _ = server_done_tx.send(());
    server.await.unwrap();
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
    state.betting_error_one_shot_overrides.lock().insert(
        METHOD_LIST_CURRENT_ORDERS.to_string(),
        betting_api_error("NO_SESSION"),
    );
    let fixture = load_fixture("rest/list_current_orders_executable.json");
    let v: Value = serde_json::from_str(&fixture).unwrap();
    state
        .betting_overrides
        .lock()
        .insert(METHOD_LIST_CURRENT_ORDERS.to_string(), v["result"].clone());
    state.betting_response_delays.lock().insert(
        METHOD_LIST_CURRENT_ORDERS.to_string(),
        Duration::from_secs(1),
    );

    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, _cache) = create_test_execution_client(addr, stream_port);

    let (server_done_tx, server_done_rx) = tokio::sync::oneshot::channel();
    let server_state = state.clone();

    let server = tokio::spawn(async move {
        let (mut reader, write_half) = accept_and_auth(&listener).await;
        let mut initial_sub = String::new();
        tokio::io::AsyncBufReadExt::read_line(&mut reader, &mut initial_sub)
            .await
            .unwrap();
        let initial_sub_json: Value = serde_json::from_str(&initial_sub).unwrap();
        assert_eq!(initial_sub_json["op"], "orderSubscription");

        wait_for_mock_state(&server_state, "login count 2", |state| {
            state.login_count.load(Ordering::Relaxed) == 2
        })
        .await;

        drop(reader);
        drop(write_half);

        let (socket, _) = tokio::time::timeout(Duration::from_secs(5), listener.accept())
            .await
            .expect("stream must reconnect while the retried report is still pending")
            .unwrap();
        let (read_half, replacement_write_half) = socket.into_split();
        let mut replacement_reader = tokio::io::BufReader::new(read_half);

        let mut auth = String::new();
        tokio::io::AsyncBufReadExt::read_line(&mut replacement_reader, &mut auth)
            .await
            .unwrap();
        let auth_json: Value = serde_json::from_str(&auth).unwrap();
        assert_eq!(auth_json["op"], "authentication");
        assert_eq!(auth_json["session"], "REFRESHED_SESSION_TOKEN");

        let mut replayed_sub = String::new();
        tokio::io::AsyncBufReadExt::read_line(&mut replacement_reader, &mut replayed_sub)
            .await
            .unwrap();
        let replayed_sub_json: Value = serde_json::from_str(&replayed_sub).unwrap();
        assert_eq!(replayed_sub_json, initial_sub_json);

        let _ = server_done_rx.await;
        drop(replacement_write_half);
    });

    client.connect().await.unwrap();

    let mut login_response: Value =
        serde_json::from_str(&load_fixture("rest/login_success.json")).unwrap();
    login_response["token"] = Value::String("REFRESHED_SESSION_TOKEN".to_string());
    *state.login_response_override.lock() = Some(serde_json::to_string(&login_response).unwrap());
    *state.keep_alive_response_override.lock() = Some(load_fixture("rest/login_failure.json"));

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
    assert_eq!(
        state.login_count.load(std::sync::atomic::Ordering::Relaxed),
        2,
        "failed keep-alive must cause exactly one full re-login"
    );

    let _ = server_done_tx.send(());
    server.await.unwrap();
    client.disconnect().await.unwrap();
}

/// `generate_fill_reports` has its own copy of the NO_SESSION recovery
/// branch (`execution.rs:1311`). The duplicated logic means a regression in
/// only the fill-reports path could pass while the order-status-reports test
/// still goes green; cover it with the same one-shot setup.
#[rstest]
#[tokio::test]
async fn test_generate_fill_reports_recovers_from_no_session() {
    let (addr, state) = start_mock_http().await;

    state.betting_error_one_shot_overrides.lock().insert(
        METHOD_LIST_CURRENT_ORDERS.to_string(),
        betting_api_error("NO_SESSION"),
    );
    let fixture = load_fixture("rest/list_current_orders_execution_complete.json");
    let v: Value = serde_json::from_str(&fixture).unwrap();
    state
        .betting_overrides
        .lock()
        .insert(METHOD_LIST_CURRENT_ORDERS.to_string(), v["result"].clone());

    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, _cache) = create_test_execution_client(addr, stream_port);

    let (server_done_tx, server_done_rx) = tokio::sync::oneshot::channel();

    let server = tokio::spawn(async move {
        let (_reader, write_half) = accept_and_activate(&listener).await;
        let _ = server_done_rx.await;
        drop(write_half);
    });

    connect_execution_ready(&mut client).await;

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
    let _ = server_done_tx.send(());
    server.await.unwrap();
}

/// `query_order` runs through `list_current_orders_with_retry`
/// (`execution.rs:2522`), which is the third copy of the NO_SESSION recovery
/// branch. Verify that path also recovers transparently.
#[rstest]
#[tokio::test]
async fn test_query_order_recovers_from_no_session() {
    let (addr, state) = start_mock_http().await;

    state.betting_error_one_shot_overrides.lock().insert(
        METHOD_LIST_CURRENT_ORDERS.to_string(),
        betting_api_error("NO_SESSION"),
    );
    let fixture = load_fixture("rest/list_current_orders_executable.json");
    let v: Value = serde_json::from_str(&fixture).unwrap();
    state
        .betting_overrides
        .lock()
        .insert(METHOD_LIST_CURRENT_ORDERS.to_string(), v["result"].clone());

    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, _cache) = create_test_execution_client(addr, stream_port);

    let (server_done_tx, server_done_rx) = tokio::sync::oneshot::channel();

    let server = tokio::spawn(async move {
        let (_reader, write_half) = accept_and_activate(&listener).await;
        let _ = server_done_rx.await;
        drop(write_half);
    });

    connect_execution_ready(&mut client).await;

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
    let _ = server_done_tx.send(());
    server.await.unwrap();
}

/// Replace-flow reconciliation: after a successful `replaceOrders`, the OCM
/// will publish a cancel for the *old* bet id (Betfair models a price modify
/// as cancel-old + place-new). The handler must recognize that cancel as part
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
        let (_reader, mut write_half) = accept_and_activate(&listener).await;

        // Forward OCM frames pushed by the test until the test drops `ocm_tx`.
        while let Some(line) = ocm_rx.recv().await {
            tokio::io::AsyncWriteExt::write_all(&mut write_half, format!("{line}\r\n").as_bytes())
                .await
                .unwrap();
        }
    });

    connect_execution_ready(&mut client).await;

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

    wait_for_mock_state(
        &state,
        "METHOD_REPLACE_ORDERS request count >= 1",
        |state| betting_method_count(state, METHOD_REPLACE_ORDERS) >= 1,
    )
    .await;

    // The replace success emits OrderUpdated directly, promoting the order to the
    // new bet id (from the replace fixture) at the requested price.
    let mut updated_seen = false;

    for _ in 0..4 {
        match tokio::time::timeout(Duration::from_secs(5), rx.recv()).await {
            Ok(Some(ExecutionEvent::Order(OrderEventAny::Updated(updated)))) => {
                assert_eq!(
                    updated.venue_order_id,
                    Some(VenueOrderId::from("240808766933"))
                );
                assert_eq!(updated.price, Some(Price::from("3.00")));
                updated_seen = true;
                break;
            }
            Ok(Some(_)) => {}
            _ => break,
        }
    }

    assert!(
        updated_seen,
        "successful price replace must emit OrderUpdated promoting the new bet id"
    );

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
    server.await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_startup_restored_replace_stream_before_rest_emits_updated_once() {
    let (addr, state) = start_mock_http().await;
    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, cache) = create_test_execution_client(addr, stream_port);
    let (ocm_tx, mut ocm_rx) = tokio::sync::mpsc::unbounded_channel::<String>();

    let server = tokio::spawn(async move {
        let (_reader, mut write_half) = accept_and_activate(&listener).await;

        while let Some(line) = ocm_rx.recv().await {
            tokio::io::AsyncWriteExt::write_all(&mut write_half, format!("{line}\r\n").as_bytes())
                .await
                .unwrap();
        }
    });

    let order = make_accepted_test_order(
        "1.181005744-86362-0.BETFAIR",
        "O-RPL-RACE",
        "228302937743",
        "2.58",
        "10",
    );
    add_order_to_cache(&cache, order);

    connect_execution_ready(&mut client).await;

    while rx.try_recv().is_ok() {}

    let waiters = Arc::new(AtomicUsize::new(0));
    let semaphore = Arc::new(tokio::sync::Semaphore::new(0));
    *state.betting_response_gate.lock() = Some(MockResponseGate {
        method: METHOD_REPLACE_ORDERS.to_string(),
        waiters: Arc::clone(&waiters),
        semaphore: Arc::clone(&semaphore),
    });

    client
        .modify_order(ModifyOrder::new(
            TraderId::from("TESTER-001"),
            Some(*BETFAIR_CLIENT_ID),
            StrategyId::from("S-001"),
            InstrumentId::from("1.181005744-86362-0.BETFAIR"),
            ClientOrderId::from("O-RPL-RACE"),
            Some(VenueOrderId::from("228302937743")),
            None,
            Some(Price::from("3.00")),
            None,
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ))
        .unwrap();

    wait_for_mock_state(&state, "response gate waiter count 1", |state| {
        response_gate_waiter_count(state) == 1
    })
    .await;

    assert_eq!(waiters.load(Ordering::Relaxed), 1);

    let mut replace_open = load_json_fixture("stream/ocm_harness_replace_open.json");
    replace_open["id"] = Value::from(2);
    replace_open["oc"][0]["id"] = Value::from("1.181005744");
    replace_open["oc"][0]["orc"][0]["id"] = Value::from(86362);
    replace_open["oc"][0]["orc"][0]["uo"][0]["p"] = Value::from(3.0);
    replace_open["oc"][0]["orc"][0]["uo"][0]["side"] = Value::from("L");
    replace_open["oc"][0]["orc"][0]["uo"][0]["rfo"] = Value::from("O-RPL-RACE");
    ocm_tx.send(replace_open.to_string()).unwrap();

    let updated = loop {
        match tokio::time::timeout(Duration::from_secs(5), rx.recv()).await {
            Ok(Some(ExecutionEvent::Order(OrderEventAny::Updated(updated)))) => break updated,
            Ok(Some(ExecutionEvent::Order(
                OrderEventAny::Accepted(_) | OrderEventAny::ModifyRejected(_),
            ))) => panic!("startup-restored replace emitted acceptance or rejection"),
            Ok(Some(_)) => {}
            other => panic!("replacement OCM did not emit OrderUpdated: {other:?}"),
        }
    };

    assert_eq!(
        updated.venue_order_id,
        Some(VenueOrderId::from("240808766933"))
    );
    assert_eq!(updated.price, Some(Price::from("3.00")));
    assert_eq!(updated.quantity, Quantity::from("10"));

    semaphore.add_permits(1);
    state.betting_response_gate.lock().take();
    let settle = tokio::time::sleep(Duration::from_millis(500));
    tokio::pin!(settle);
    let mut duplicate_update = None;
    let mut unexpected_event = false;

    loop {
        tokio::select! {
            () = &mut settle => break,
            event = rx.recv() => {
                match event {
                    Some(ExecutionEvent::Order(OrderEventAny::Updated(updated))) => {
                        duplicate_update = Some(updated);
                        break;
                    }
                    Some(ExecutionEvent::Order(
                        OrderEventAny::Accepted(_) | OrderEventAny::ModifyRejected(_),
                    )) => {
                        unexpected_event = true;
                        break;
                    }
                    _ => {}
                }
            }
        }
    }

    assert!(
        duplicate_update.is_none(),
        "REST success duplicated the stream-first OrderUpdated: {duplicate_update:?}"
    );
    assert!(
        !unexpected_event,
        "startup-restored replace emitted acceptance or rejection: {unexpected_event:?}",
    );

    drop(ocm_tx);
    client.disconnect().await.unwrap();
    server.await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_startup_restored_replace_rest_before_stream_emits_updated_once() {
    let (addr, state) = start_mock_http().await;
    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, cache) = create_test_execution_client(addr, stream_port);
    let (ocm_tx, mut ocm_rx) = tokio::sync::mpsc::unbounded_channel::<String>();

    let server = tokio::spawn(async move {
        let (_reader, mut write_half) = accept_and_activate(&listener).await;

        while let Some(line) = ocm_rx.recv().await {
            tokio::io::AsyncWriteExt::write_all(&mut write_half, format!("{line}\r\n").as_bytes())
                .await
                .unwrap();
        }
    });

    let order = make_accepted_test_order(
        "1.181005744-86362-0.BETFAIR",
        "O-RPL-REST-FIRST",
        "228302937743",
        "2.58",
        "10",
    );
    add_order_to_cache(&cache, order);

    connect_execution_ready(&mut client).await;

    while rx.try_recv().is_ok() {}

    let waiters = Arc::new(AtomicUsize::new(0));
    let semaphore = Arc::new(tokio::sync::Semaphore::new(0));
    *state.betting_response_gate.lock() = Some(MockResponseGate {
        method: METHOD_REPLACE_ORDERS.to_string(),
        waiters: Arc::clone(&waiters),
        semaphore: Arc::clone(&semaphore),
    });

    client
        .modify_order(ModifyOrder::new(
            TraderId::from("TESTER-001"),
            Some(*BETFAIR_CLIENT_ID),
            StrategyId::from("S-001"),
            InstrumentId::from("1.181005744-86362-0.BETFAIR"),
            ClientOrderId::from("O-RPL-REST-FIRST"),
            Some(VenueOrderId::from("228302937743")),
            None,
            Some(Price::from("3.00")),
            None,
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ))
        .unwrap();

    wait_for_mock_state(&state, "response gate waiter count 1", |state| {
        response_gate_waiter_count(state) == 1
    })
    .await;

    assert_eq!(waiters.load(Ordering::Relaxed), 1);

    semaphore.add_permits(1);

    let updated = loop {
        match tokio::time::timeout(Duration::from_secs(5), rx.recv()).await {
            Ok(Some(ExecutionEvent::Order(OrderEventAny::Updated(updated)))) => break updated,
            Ok(Some(ExecutionEvent::Order(
                OrderEventAny::Accepted(_) | OrderEventAny::ModifyRejected(_),
            ))) => panic!("REST-first replace emitted acceptance or rejection"),
            Ok(Some(_)) => {}
            other => panic!("REST replace did not emit OrderUpdated before the stream: {other:?}"),
        }
    };

    assert_eq!(
        updated.venue_order_id,
        Some(VenueOrderId::from("240808766933"))
    );
    assert_eq!(updated.price, Some(Price::from("3.00")));
    assert_eq!(updated.quantity, Quantity::from("10"));

    let mut replace_open = load_json_fixture("stream/ocm_harness_replace_open.json");
    replace_open["id"] = Value::from(2);
    replace_open["oc"][0]["id"] = Value::from("1.181005744");
    replace_open["oc"][0]["orc"][0]["id"] = Value::from(86362);
    replace_open["oc"][0]["orc"][0]["uo"][0]["p"] = Value::from(3.0);
    replace_open["oc"][0]["orc"][0]["uo"][0]["side"] = Value::from("L");
    replace_open["oc"][0]["orc"][0]["uo"][0]["rfo"] = Value::from("O-RPL-REST-FIRST");
    ocm_tx.send(replace_open.to_string()).unwrap();

    let settle = tokio::time::sleep(Duration::from_millis(500));
    tokio::pin!(settle);
    let mut duplicate_update = None;
    let mut unexpected_event = false;

    loop {
        tokio::select! {
            () = &mut settle => break,
            event = rx.recv() => {
                match event {
                    Some(ExecutionEvent::Order(OrderEventAny::Updated(updated))) => {
                        duplicate_update = Some(updated);
                        break;
                    }
                    Some(ExecutionEvent::Order(
                        OrderEventAny::Accepted(_) | OrderEventAny::ModifyRejected(_),
                    )) => {
                        unexpected_event = true;
                        break;
                    }
                    _ => {}
                }
            }
        }
    }

    assert!(
        duplicate_update.is_none(),
        "stream duplicated the REST-first OrderUpdated: {duplicate_update:?}"
    );
    assert!(
        !unexpected_event,
        "REST-first replace emitted acceptance or rejection after the stream: {unexpected_event:?}",
    );

    drop(ocm_tx);
    client.disconnect().await.unwrap();
    server.await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_open_only_supplemental_snapshot_includes_successor_bound_during_fetch() {
    let (addr, state) = start_mock_http().await;
    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, cache) = create_test_execution_client(addr, stream_port);
    let (ocm_tx, mut ocm_rx) = tokio::sync::mpsc::unbounded_channel::<String>();

    let server = tokio::spawn(async move {
        let (_reader, mut write_half) = accept_and_activate(&listener).await;

        while let Some(line) = ocm_rx.recv().await {
            tokio::io::AsyncWriteExt::write_all(&mut write_half, format!("{line}\r\n").as_bytes())
                .await
                .unwrap();
        }
    });

    let instrument_id = "1.179082386-235.BETFAIR";
    let client_order_id = "O-SNAP-RACE";
    let mut order =
        make_accepted_test_order(instrument_id, client_order_id, "hist-old", "2.50", "10");
    order
        .apply(OrderEventAny::Updated(OrderUpdated::new(
            TraderId::from("TESTER-001"),
            StrategyId::from("S-001"),
            InstrumentId::from(instrument_id),
            ClientOrderId::from(client_order_id),
            Quantity::from("10"),
            UUID4::new(),
            UnixNanos::from(2),
            UnixNanos::from(2),
            true,
            Some(VenueOrderId::from("hist-live")),
            Some(AccountId::from("BETFAIR-001")),
            Some(Price::from("3.00")),
            None,
            None,
            false,
        )))
        .unwrap();
    add_order_to_cache(&cache, order);
    connect_execution_ready(&mut client).await;

    while rx.try_recv().is_ok() {}

    *state.betting_current_orders.lock() = Some(vec![
        current_order_leg("hist-old", client_order_id, 10.0, 4.0, 0.0, 2.5),
        current_order_leg("hist-live", client_order_id, 6.0, 2.0, 4.0, 3.0),
    ]);
    state
        .betting_status_overrides
        .lock()
        .insert(METHOD_REPLACE_ORDERS.to_string(), 500);

    let gate = MockResponseGate {
        method: METHOD_LIST_CURRENT_ORDERS.to_string(),
        waiters: Arc::new(AtomicUsize::new(0)),
        semaphore: Arc::new(tokio::sync::Semaphore::new(1)),
    };

    *state.betting_response_gate.lock() = Some(gate.clone());
    let command = GenerateOrderStatusReportsBuilder::default()
        .ts_init(UnixNanos::default())
        .open_only(true)
        .build()
        .unwrap();

    let _first = {
        let pending = client.generate_order_status_reports(&command);
        tokio::pin!(pending);
        tokio::select! {
            result = &mut pending => panic!("supplemental history query was not gated: {result:?}"),
            () = wait_for_mock_state(&state, "supplemental history query blocked", |_| {
                gate.waiters.load(Ordering::Relaxed) == 2
            }) => {}
        }

        client
            .modify_order(make_price_modify_order_cmd(
                instrument_id,
                client_order_id,
                "hist-live",
                "4.00",
            ))
            .unwrap();
        wait_for_mock_state(
            &state,
            "replacement registered while history query waits",
            |state| betting_method_count(state, METHOD_REPLACE_ORDERS) == 1,
        )
        .await;

        let mut replacement = load_json_fixture("stream/ocm_harness_replace_open.json");
        replacement["id"] = Value::from(2);
        replacement["oc"][0]["id"] = Value::from("1.179082386");
        replacement["oc"][0]["orc"][0]["id"] = Value::from(235);
        let unmatched = &mut replacement["oc"][0]["orc"][0]["uo"][0];
        unmatched["id"] = Value::from("hist-new");
        unmatched["p"] = Value::from(4.0);
        unmatched["s"] = Value::from(4);
        unmatched["sm"] = Value::from(1);
        unmatched["sr"] = Value::from(3);
        unmatched["side"] = Value::from("L");
        unmatched["rfo"] = Value::from(client_order_id);
        ocm_tx.send(replacement.to_string()).unwrap();

        let updated = loop {
            match tokio::time::timeout(Duration::from_secs(5), rx.recv()).await {
                Ok(Some(ExecutionEvent::Order(OrderEventAny::Updated(updated)))) => break updated,
                Ok(Some(_)) => {}
                other => {
                    panic!("successor was not bound while the history query waited: {other:?}")
                }
            }
        };

        assert_eq!(updated.venue_order_id, Some(VenueOrderId::from("hist-new")));

        state
            .betting_current_orders
            .lock()
            .as_mut()
            .unwrap()
            .push(current_order_leg(
                "hist-new",
                client_order_id,
                4.0,
                1.0,
                3.0,
                4.0,
            ));
        gate.semaphore.add_permits(1);
        state.betting_response_gate.lock().take();
        let first = pending.as_mut().await.unwrap();
        assert!(
            first
                .iter()
                .any(|report| report.venue_order_id.as_str() == "hist-new"),
            "gated scan failed or omitted the successor bound during the fetch: {first:?}"
        );
        first
    };

    let second = client
        .generate_order_status_reports(&command)
        .await
        .unwrap();
    let report = second
        .iter()
        .find(|report| report.venue_order_id.as_str() == "hist-new")
        .expect("subsequent scan omitted the successor");
    assert_eq!(report.quantity, Quantity::from("10"));
    assert_eq!(report.filled_qty, Quantity::from("7"));
    assert_eq!(report.avg_px, Some(Decimal::from(20) / Decimal::from(7)));
    assert!(
        drain_events(&mut rx, Duration::from_millis(100))
            .await
            .iter()
            .all(|event| { !matches!(event, ExecutionEvent::Order(OrderEventAny::Updated(_))) })
    );

    drop(ocm_tx);
    client.disconnect().await.unwrap();
    server.await.unwrap();
}

#[rstest]
#[case::account(None, true)]
#[case::settled(Some("1.179082386"), false)]
#[case::live(Some("1.999999999"), true)]
#[tokio::test]
async fn test_open_only_regression_absent_cached_order_does_not_require_history(
    #[case] market_id: Option<&str>,
    #[case] expects_report: bool,
) {
    let (addr, state) = start_mock_http().await;
    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, cache) = create_test_execution_client(addr, stream_port);
    let (server_done_tx, server_done_rx) = tokio::sync::oneshot::channel();

    let server = tokio::spawn(async move {
        let (_reader, write_half) = accept_and_activate(&listener).await;
        let _ = server_done_rx.await;
        drop(write_half);
    });

    let settled_instrument = InstrumentId::from("1.179082386-235.BETFAIR");
    let live_instrument = InstrumentId::from("1.999999999-235.BETFAIR");
    add_order_to_cache(
        &cache,
        make_accepted_test_order(
            &settled_instrument.to_string(),
            "O-SETTLED-ABSENT",
            "settled-absent",
            "2.50",
            "10",
        ),
    );
    connect_execution_ready(&mut client).await;

    while rx.try_recv().is_ok() {}

    let mut live = current_order_leg("ordinary-live", "foreign-live", 7.0, 2.0, 5.0, 3.75);
    live["marketId"] = Value::from("1.999999999");
    *state.betting_current_orders.lock() = Some(vec![live.clone()]);
    let live_order = serde_json::from_value(live).unwrap();

    let instrument_id =
        market_id.map(|market_id| InstrumentId::from(format!("{market_id}-235.BETFAIR")));
    let command = GenerateOrderStatusReportsBuilder::default()
        .ts_init(get_atomic_clock_realtime().get_time_ns())
        .instrument_id(instrument_id)
        .open_only(true)
        .build()
        .unwrap();

    for _ in 0..2 {
        state.betting_request_params.lock().clear();
        let reports = client
            .generate_order_status_reports(&command)
            .await
            .unwrap();

        if expects_report {
            assert_eq!(reports.len(), 1);
            let report = &reports[0];
            let mut expected = parse_current_order_report(
                &live_order,
                AccountId::from("BETFAIR-001"),
                report.ts_init,
            )
            .unwrap();
            expected.report_id = report.report_id;
            assert_eq!(report, &expected);
            assert_eq!(report.instrument_id, live_instrument);
            assert_eq!(report.venue_order_id, VenueOrderId::from("ordinary-live"));
            assert_eq!(report.order_status, OrderStatus::PartiallyFilled);
            assert_eq!(report.quantity, Quantity::from("7"));
            assert_eq!(report.filled_qty, Quantity::from("2"));
            assert_eq!(report.avg_px, Some(Decimal::new(375, 2)));
            assert!(report.ts_init >= command.ts_init);
            assert!(report.ts_init <= get_atomic_clock_realtime().get_time_ns());
        } else {
            assert!(reports.is_empty());
        }

        let requests = state.betting_request_params.lock().clone();
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0].0, METHOD_LIST_CURRENT_ORDERS);
        let mut expected = serde_json::json!({"orderProjection": "EXECUTABLE"});

        if let Some(market_id) = market_id {
            expected["marketIds"] = serde_json::json!([market_id]);
            assert_eq!(requests[1].0, METHOD_LIST_MARKET_CATALOGUE);
            assert_eq!(
                requests[1].1["filter"],
                serde_json::json!({"bspOnly": true, "withOrders": ["EXECUTION_COMPLETE"], "marketIds": [market_id]})
            );
            assert_eq!(requests[1].1["maxResults"], 1000);
        } else {
            assert_eq!(requests[1].0, METHOD_LIST_EVENTS);
            assert_eq!(
                requests[1].1,
                serde_json::json!({"filter": {"bspOnly": true, "withOrders": ["EXECUTION_COMPLETE"]}})
            );
        }

        assert_eq!(requests[0].1, expected);
        assert!(rx.try_recv().is_err());
    }

    client.disconnect().await.unwrap();
    let _ = server_done_tx.send(());
    server.await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_open_only_regression_refreshes_pending_reference_during_history_fetch() {
    let (addr, state) = start_mock_http().await;
    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, cache) = create_test_execution_client(addr, stream_port);
    let (server_done_tx, server_done_rx) = tokio::sync::oneshot::channel();

    let server = tokio::spawn(async move {
        let (_reader, write_half) = accept_and_activate(&listener).await;
        let _ = server_done_rx.await;
        drop(write_half);
    });

    let instrument_id = "1.179082386-235.BETFAIR";
    let client_order_id = "O-FRESH-REFERENCE";
    add_order_to_cache(
        &cache,
        make_accepted_test_order(instrument_id, client_order_id, "fresh-a", "2.50", "10"),
    );
    connect_execution_ready(&mut client).await;

    while rx.try_recv().is_ok() {}
    replace_order_for_reports(
        &client,
        &mut rx,
        &state,
        make_price_modify_order_cmd(instrument_id, client_order_id, "fresh-a", "3.00"),
        "fresh-b",
        false,
    )
    .await;

    let predecessor = current_order_leg("fresh-a", client_order_id, 10.0, 4.0, 0.0, 2.5);
    let current = current_order_leg("fresh-b", client_order_id, 6.0, 2.0, 4.0, 3.0);
    *state.betting_current_orders.lock() = Some(vec![predecessor.clone(), current]);
    state.betting_request_params.lock().clear();

    let gate = MockResponseGate {
        method: METHOD_LIST_CURRENT_ORDERS.to_string(),
        waiters: Arc::new(AtomicUsize::new(0)),
        semaphore: Arc::new(tokio::sync::Semaphore::new(1)),
    };

    *state.betting_response_gate.lock() = Some(gate.clone());
    let command = GenerateOrderStatusReportsBuilder::default()
        .ts_init(get_atomic_clock_realtime().get_time_ns())
        .open_only(true)
        .build()
        .unwrap();
    let previous = current_order_leg("fresh-b", client_order_id, 6.0, 2.0, 0.0, 3.0);
    let successor = current_order_leg("fresh-c", client_order_id, 4.0, 1.0, 0.0, 4.0);

    let resolving = {
        let pending = client.generate_order_status_reports(&command);
        tokio::pin!(pending);
        tokio::select! {
            result = &mut pending => panic!("predecessor history query was not gated: {result:?}"),
            () = wait_for_mock_state(&state, "predecessor history query blocked", |_| {
                gate.waiters.load(Ordering::Relaxed) == 2
            }) => {}
        }
        let requests = state.betting_request_params.lock().clone();
        assert_eq!(requests.last().unwrap().0, METHOD_LIST_CURRENT_ORDERS);
        assert_eq!(
            requests.last().unwrap().1,
            serde_json::json!({"orderProjection": "ALL", "betIds": ["fresh-a"]})
        );
        replace_order_for_reports(
            &client,
            &mut rx,
            &state,
            make_price_modify_order_cmd(instrument_id, client_order_id, "fresh-b", "4.00"),
            "fresh-c",
            true,
        )
        .await;
        *state.betting_current_orders.lock() = Some(vec![
            predecessor.clone(),
            previous.clone(),
            successor.clone(),
        ]);
        assert!(rx.try_recv().is_err());
        state.betting_response_gate.lock().take();
        gate.semaphore.add_permits(1);
        pending.await.unwrap()
    };

    let events = drain_events(&mut rx, Duration::from_millis(100)).await;
    let requests: Vec<_> = state
        .betting_request_params
        .lock()
        .iter()
        .filter(|(method, _)| method == METHOD_LIST_CURRENT_ORDERS)
        .cloned()
        .collect();

    assert!(resolving.is_empty());
    assert_eq!(gate.waiters.load(Ordering::Relaxed), 2);
    assert_eq!(requests.len(), 3);
    assert_eq!(
        requests[0].1,
        serde_json::json!({"orderProjection": "EXECUTABLE"})
    );
    assert_eq!(
        requests[1].1,
        serde_json::json!({"orderProjection": "ALL", "betIds": ["fresh-a"]})
    );
    assert_eq!(
        requests[2].1,
        serde_json::json!({"orderProjection": "ALL", "customerOrderRefs": [make_customer_order_ref(client_order_id)]})
    );
    assert_eq!(events.len(), 1);
    let updates = order_updates(&events);
    assert_eq!(updates.len(), 1);
    assert_eq!(updates[0].instrument_id, InstrumentId::from(instrument_id));
    assert_eq!(
        updates[0].client_order_id,
        ClientOrderId::from(client_order_id)
    );
    assert_eq!(
        updates[0].venue_order_id,
        Some(VenueOrderId::from("fresh-c"))
    );
    assert_eq!(updates[0].quantity, Quantity::from("10"));
    assert_eq!(updates[0].price, Some(Price::from("4.00")));
    assert_no_accept_or_modify_reject(&events);

    let repeated = client
        .generate_order_status_reports(&command)
        .await
        .unwrap();
    let mut full = command;
    full.open_only = false;
    let reports = client.generate_order_status_reports(&full).await.unwrap();
    assert!(repeated.is_empty());
    assert_eq!(
        reports
            .iter()
            .map(|report| report.venue_order_id.as_str())
            .collect::<HashSet<_>>(),
        HashSet::from(["fresh-a", "fresh-b", "fresh-c"])
    );

    for leg in [predecessor, previous] {
        let report = reports
            .iter()
            .find(|report| report.venue_order_id.as_str() == leg["betId"].as_str().unwrap())
            .unwrap();
        let leg = serde_json::from_value(leg).unwrap();
        let mut expected =
            parse_current_order_report(&leg, AccountId::from("BETFAIR-001"), report.ts_init)
                .unwrap();
        expected.report_id = report.report_id;
        assert_eq!(report, &expected);
    }

    assert_replacement_report(
        reports
            .iter()
            .find(|report| report.venue_order_id.as_str() == "fresh-c")
            .unwrap(),
        successor,
        client_order_id,
        "7",
        Decimal::from(20) / Decimal::from(7),
    );
    assert!(
        drain_events(&mut rx, Duration::from_millis(100))
            .await
            .is_empty()
    );
    client.disconnect().await.unwrap();
    let _ = server_done_tx.send(());
    server.await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_open_only_refreshes_same_reference_for_successive_pending_replacements() {
    let (addr, state) = start_mock_http().await;
    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, cache) = create_test_execution_client(addr, stream_port);
    let (ocm_tx, mut ocm_rx) = tokio::sync::mpsc::unbounded_channel::<String>();

    let server = tokio::spawn(async move {
        let (_reader, mut write_half) = accept_and_activate(&listener).await;

        while let Some(line) = ocm_rx.recv().await {
            write_half
                .write_all(format!("{line}\r\n").as_bytes())
                .await
                .unwrap();
        }
    });

    let instrument_id = "1.179082386-235.BETFAIR";
    let client_order_id = "O-20261004-550e8400-e29b-41d4-a716-446655440000";
    let mut references = [
        make_customer_order_ref(client_order_id),
        make_customer_order_ref_legacy(client_order_id),
    ];
    references.sort();
    assert_ne!(references[0], references[1]);
    add_order_to_cache(
        &cache,
        make_accepted_test_order(instrument_id, client_order_id, "same-ref-a", "2.50", "10"),
    );
    connect_execution_ready(&mut client).await;

    while rx.try_recv().is_ok() {}
    replace_order_for_reports(
        &client,
        &mut rx,
        &state,
        make_price_modify_order_cmd(instrument_id, client_order_id, "same-ref-a", "3.00"),
        "same-ref-b",
        true,
    )
    .await;

    let predecessor = current_order_leg("same-ref-a", &references[0], 10.0, 4.0, 0.0, 2.5);
    let current = current_order_leg("same-ref-b", &references[0], 6.0, 2.0, 4.0, 3.0);
    state.betting_response_sequences.lock().insert(
        METHOD_LIST_CURRENT_ORDERS.to_string(),
        VecDeque::from([
            serde_json::json!({"currentOrders": [current.clone()], "moreAvailable": false}),
            serde_json::json!({"currentOrders": [predecessor.clone()], "moreAvailable": false}),
            serde_json::json!({"currentOrders": [current], "moreAvailable": false}),
            serde_json::json!({"currentOrders": [], "moreAvailable": false}),
        ]),
    );
    *state.betting_current_orders.lock() = Some(vec![predecessor.clone()]);
    state.betting_request_params.lock().clear();

    let gate = MockResponseGate {
        method: METHOD_LIST_CURRENT_ORDERS.to_string(),
        waiters: Arc::new(AtomicUsize::new(0)),
        semaphore: Arc::new(tokio::sync::Semaphore::new(3)),
    };

    *state.betting_response_gate.lock() = Some(gate.clone());
    let command = GenerateOrderStatusReportsBuilder::default()
        .ts_init(get_atomic_clock_realtime().get_time_ns())
        .instrument_id(Some(InstrumentId::from(instrument_id)))
        .open_only(true)
        .build()
        .unwrap();
    let previous = current_order_leg("same-ref-b", &references[0], 6.0, 2.0, 0.0, 3.0);
    let successor = current_order_leg("same-ref-c", &references[0], 4.0, 1.0, 0.0, 4.0);

    let resolving = {
        let pending = client.generate_order_status_reports(&command);
        tokio::pin!(pending);
        tokio::select! {
            result = &mut pending => panic!("second reference request was not gated: {result:?}"),
            () = wait_for_mock_state(&state, "second reference query blocked after stale successor", |_| {
                gate.waiters.load(Ordering::Relaxed) == 4
            }) => {}
        }
        let requests: Vec<_> = state
            .betting_request_params
            .lock()
            .iter()
            .filter(|(method, _)| method == METHOD_LIST_CURRENT_ORDERS)
            .map(|(_, params)| params.clone())
            .collect();
        assert_eq!(
            requests,
            vec![
                serde_json::json!({"orderProjection": "EXECUTABLE", "marketIds": ["1.179082386"]}),
                serde_json::json!({"orderProjection": "ALL", "betIds": ["same-ref-a"]}),
                serde_json::json!({"orderProjection": "ALL", "customerOrderRefs": [references[0]]}),
                serde_json::json!({"orderProjection": "ALL", "customerOrderRefs": [references[1]]}),
            ]
        );

        let mut replacement = load_json_fixture("stream/ocm_harness_replace_open.json");
        replacement["id"] = Value::from(2);
        replacement["oc"][0]["id"] = Value::from("1.179082386");
        replacement["oc"][0]["orc"][0]["id"] = Value::from(235);
        let unmatched = &mut replacement["oc"][0]["orc"][0]["uo"][0];
        unmatched["id"] = Value::from("same-ref-b");
        unmatched["p"] = Value::from(3.0);
        unmatched["s"] = Value::from(6);
        unmatched["sm"] = Value::from(2);
        unmatched["sr"] = Value::from(4);
        unmatched["avp"] = Value::from(3.0);
        unmatched["side"] = Value::from("L");
        unmatched["rfo"] = Value::from(references[0].clone());
        ocm_tx.send(replacement.to_string()).unwrap();
        let event = tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .unwrap()
            .unwrap();

        let ExecutionEvent::Order(OrderEventAny::Updated(updated)) = event else {
            panic!("stream must resolve the first pending replacement: {event:?}");
        };

        assert_eq!(
            updated.client_order_id,
            ClientOrderId::from(client_order_id)
        );
        assert_eq!(
            updated.venue_order_id,
            Some(VenueOrderId::from("same-ref-b"))
        );
        assert_eq!(updated.quantity, Quantity::from("10"));
        assert_eq!(updated.price, Some(Price::from("3.00")));
        assert_no_accept_or_modify_reject(&drain_events(&mut rx, Duration::from_millis(100)).await);

        replace_order_for_reports(
            &client,
            &mut rx,
            &state,
            make_price_modify_order_cmd(instrument_id, client_order_id, "same-ref-b", "4.00"),
            "same-ref-c",
            true,
        )
        .await;
        *state.betting_current_orders.lock() = Some(vec![predecessor, previous, successor.clone()]);
        assert!(rx.try_recv().is_err());
        state.betting_response_gate.lock().take();
        gate.semaphore.add_permits(1);
        pending.await.unwrap()
    };

    let events = drain_events(&mut rx, Duration::from_millis(100)).await;
    assert_no_accept_or_modify_reject(&events);
    assert!(resolving.is_empty());
    assert_eq!(events.len(), 1);
    let updates = order_updates(&events);
    assert_eq!(updates.len(), 1);
    assert_eq!(updates[0].instrument_id, InstrumentId::from(instrument_id));
    assert_eq!(
        updates[0].client_order_id,
        ClientOrderId::from(client_order_id)
    );
    assert_eq!(
        updates[0].venue_order_id,
        Some(VenueOrderId::from("same-ref-c"))
    );
    assert_eq!(updates[0].quantity, Quantity::from("10"));
    assert_eq!(updates[0].price, Some(Price::from("4.00")));
    let requests: Vec<_> = state
        .betting_request_params
        .lock()
        .iter()
        .filter(|(method, _)| method == METHOD_LIST_CURRENT_ORDERS)
        .map(|(_, params)| params.clone())
        .collect();
    assert_eq!(requests.len(), 6);
    assert_eq!(requests[4], requests[2]);
    assert_eq!(requests[5], requests[3]);
    assert_eq!(gate.waiters.load(Ordering::Relaxed), 4);

    let repeated = client
        .generate_order_status_reports(&command)
        .await
        .unwrap();
    let mut full = command;
    full.open_only = false;
    let reports = client.generate_order_status_reports(&full).await.unwrap();
    assert!(repeated.is_empty());
    assert_eq!(reports.len(), 3);
    assert_eq!(
        reports
            .iter()
            .map(|report| report.venue_order_id.as_str())
            .collect::<HashSet<_>>(),
        HashSet::from(["same-ref-a", "same-ref-b", "same-ref-c"])
    );
    assert_replacement_report(
        reports
            .iter()
            .find(|report| report.venue_order_id.as_str() == "same-ref-c")
            .unwrap(),
        successor,
        client_order_id,
        "7",
        Decimal::from(20) / Decimal::from(7),
    );
    assert!(
        drain_events(&mut rx, Duration::from_millis(100))
            .await
            .is_empty()
    );
    drop(ocm_tx);
    client.disconnect().await.unwrap();
    server.await.unwrap();
}

#[rstest]
#[case::market_on_close("MARKET_ON_CLOSE", OrderType::Market)]
#[case::limit_on_close("LIMIT_ON_CLOSE", OrderType::Limit)]
#[tokio::test]
async fn test_open_only_bsp_discovery_recovers_tracked_and_untracked_orders(
    #[case] venue_order_type: &str,
    #[case] order_type: OrderType,
    #[values(OrderSide::Buy, OrderSide::Sell)] side: OrderSide,
) {
    let (addr, state) = start_mock_http().await;
    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, cache) = create_test_execution_client(addr, stream_port);
    let (server_done_tx, server_done_rx) = tokio::sync::oneshot::channel();

    let server = tokio::spawn(async move {
        let (_reader, write_half) = accept_and_activate(&listener).await;
        let _ = server_done_rx.await;
        drop(write_half);
    });

    let instrument_id = "1.179082386-235.BETFAIR";
    let client_order_id = "O-BSP-RESTING";
    let mut order = OrderTestBuilder::new(order_type)
        .trader_id(TraderId::from("TESTER-001"))
        .strategy_id(StrategyId::from("S-001"))
        .instrument_id(InstrumentId::from(instrument_id))
        .client_order_id(ClientOrderId::from(client_order_id))
        .side(side)
        .price(Price::from("1.01"))
        .quantity(Quantity::from("5"))
        .time_in_force(TimeInForce::AtTheClose)
        .build();
    order
        .apply(OrderEventAny::Accepted(OrderAccepted::new(
            TraderId::from("TESTER-001"),
            StrategyId::from("S-001"),
            InstrumentId::from(instrument_id),
            ClientOrderId::from(client_order_id),
            VenueOrderId::from("bsp-resting"),
            AccountId::from("BETFAIR-001"),
            UUID4::new(),
            UnixNanos::from(1),
            UnixNanos::from(1),
            false,
        )))
        .unwrap();
    add_order_to_cache(&cache, order);
    connect_execution_ready(&mut client).await;

    while rx.try_recv().is_ok() {}
    let mut resting = resting_bsp_order("bsp-resting", client_order_id);
    resting["orderType"] = Value::from(venue_order_type);
    resting["side"] = Value::from(if side == OrderSide::Buy {
        "LAY"
    } else {
        "BACK"
    });

    let mut foreign = resting.clone();
    foreign["betId"] = Value::from("bsp-foreign");
    foreign["customerOrderRef"] = Value::from("foreign-bsp");
    let mut orders = vec![
        resting.clone(),
        foreign,
        current_order_leg("ordinary-open", "ordinary-ref", 7.0, 2.0, 5.0, 2.5),
    ];

    for (bet_id, field) in [
        ("bsp-matched", "sizeMatched"),
        ("bsp-lapsed", "sizeLapsed"),
        ("bsp-voided", "sizeVoided"),
    ] {
        let mut terminal = resting.clone();
        terminal["betId"] = Value::from(bet_id);
        terminal["customerOrderRef"] = Value::from(bet_id);
        terminal[field] = Value::from(5.0);
        orders.push(terminal);
    }

    *state.betting_current_orders.lock() = Some(orders);
    state.betting_request_params.lock().clear();

    let command = GenerateOrderStatusReportsBuilder::default()
        .ts_init(get_atomic_clock_realtime().get_time_ns())
        .open_only(true)
        .build()
        .unwrap();

    for _ in 0..2 {
        let reports = client
            .generate_order_status_reports(&command)
            .await
            .unwrap();
        assert_eq!(reports.len(), 3);
        assert_eq!(
            reports
                .iter()
                .map(|report| report.venue_order_id.as_str())
                .collect::<HashSet<_>>(),
            HashSet::from(["bsp-resting", "bsp-foreign", "ordinary-open"]),
        );

        for (bet_id, reference) in [
            ("bsp-resting", client_order_id),
            ("bsp-foreign", "foreign-bsp"),
        ] {
            let report = reports
                .iter()
                .find(|report| report.venue_order_id.as_str() == bet_id)
                .unwrap();
            assert_resting_bsp_report(
                report,
                instrument_id,
                bet_id,
                reference,
                side,
                order_type,
                command.ts_init,
            );
        }

        let ordinary = reports
            .iter()
            .find(|report| report.venue_order_id.as_str() == "ordinary-open")
            .unwrap();
        let source = serde_json::from_value(current_order_leg(
            "ordinary-open",
            "ordinary-ref",
            7.0,
            2.0,
            5.0,
            2.5,
        ))
        .unwrap();
        let mut expected =
            parse_current_order_report(&source, AccountId::from("BETFAIR-001"), ordinary.ts_init)
                .unwrap();
        expected.report_id = ordinary.report_id;
        assert_eq!(ordinary, &expected);
        assert_eq!(ordinary.order_status, OrderStatus::PartiallyFilled);
        assert_eq!(ordinary.quantity, Quantity::from("7"));
        assert_eq!(ordinary.filled_qty, Quantity::from("2"));
        assert!(rx.try_recv().is_err());
    }

    let requests = state.betting_request_params.lock().clone();
    assert_eq!(requests.len(), 8);

    for scan in requests.as_chunks::<4>().0 {
        assert_eq!(scan[0].0, METHOD_LIST_CURRENT_ORDERS);
        assert_eq!(scan[0].1["orderProjection"], "EXECUTABLE");
        assert_eq!(scan[1].0, METHOD_LIST_EVENTS);
        assert_eq!(
            scan[1].1["filter"],
            serde_json::json!({"bspOnly": true, "withOrders": ["EXECUTION_COMPLETE"]})
        );
        assert_eq!(scan[2].0, METHOD_LIST_MARKET_CATALOGUE);
        assert_eq!(
            scan[2].1["filter"],
            serde_json::json!({"bspOnly": true, "withOrders": ["EXECUTION_COMPLETE"], "eventIds": ["bsp-event"]})
        );
        assert_eq!(scan[2].1["maxResults"], 1000);
        assert_eq!(scan[3].0, METHOD_LIST_CURRENT_ORDERS);
        assert_eq!(scan[3].1["orderProjection"], "ALL");
        assert_eq!(scan[3].1["marketIds"], serde_json::json!(["1.179082386"]));
        assert!(scan[3].1.get("betIds").is_none());
        assert!(scan[3].1.get("customerOrderRefs").is_none());
    }

    client.disconnect().await.unwrap();
    let _ = server_done_tx.send(());
    server.await.unwrap();
}

#[rstest]
#[case::instrument(true)]
#[case::reconcile_market(false)]
#[tokio::test]
async fn test_open_only_bsp_discovery_preserves_scope(#[case] instrument_scoped: bool) {
    let (addr, state) = start_mock_http().await;
    let (stream_port, listener) = start_mock_stream().await;

    let config = if instrument_scoped {
        BetfairExecutionClientConfig::default()
    } else {
        BetfairExecutionClientConfig {
            reconcile_market_ids_only: true,
            reconcile_market_ids: Some(vec!["1.179082386".to_string()]),
            ..Default::default()
        }
    };

    let (mut client, mut rx, _data_rx, _cache) =
        create_test_execution_client_with_config(addr, stream_port, config);
    let (server_done_tx, server_done_rx) = tokio::sync::oneshot::channel();

    let server = tokio::spawn(async move {
        let (_reader, write_half) = accept_and_activate(&listener).await;
        let _ = server_done_rx.await;
        drop(write_half);
    });

    connect_execution_ready(&mut client).await;

    while rx.try_recv().is_ok() {}
    let mut other_selection = resting_bsp_order("bsp-other-selection", "other-selection-ref");
    other_selection["selectionId"] = Value::from(236);
    let mut other_market = resting_bsp_order("bsp-other-market", "other-market-ref");
    other_market["marketId"] = Value::from("1.999999999");
    *state.betting_current_orders.lock() = Some(vec![
        resting_bsp_order("bsp-resting", "foreign-bsp"),
        other_selection,
        other_market,
    ]);
    state.betting_request_params.lock().clear();

    let command = GenerateOrderStatusReportsBuilder::default()
        .ts_init(get_atomic_clock_realtime().get_time_ns())
        .open_only(true)
        .instrument_id(instrument_scoped.then(|| InstrumentId::from("1.179082386-235.BETFAIR")))
        .build()
        .unwrap();
    let reports = client
        .generate_order_status_reports(&command)
        .await
        .unwrap();
    assert_eq!(reports.len(), if instrument_scoped { 1 } else { 2 });
    assert_eq!(
        reports
            .iter()
            .map(|report| report.venue_order_id.as_str())
            .collect::<HashSet<_>>(),
        if instrument_scoped {
            HashSet::from(["bsp-resting"])
        } else {
            HashSet::from(["bsp-resting", "bsp-other-selection"])
        }
    );

    for report in &reports {
        let (instrument_id, reference) = match report.venue_order_id.as_str() {
            "bsp-resting" => ("1.179082386-235.BETFAIR", "foreign-bsp"),
            "bsp-other-selection" if !instrument_scoped => {
                ("1.179082386-236.BETFAIR", "other-selection-ref")
            }
            other => panic!("out-of-scope BSP report: {other}"),
        };

        assert_resting_bsp_report(
            report,
            instrument_id,
            report.venue_order_id.as_str(),
            reference,
            OrderSide::Buy,
            OrderType::Market,
            command.ts_init,
        );
    }

    let requests = state.betting_request_params.lock().clone();
    assert_eq!(requests.len(), 3);
    assert_eq!(requests[0].0, METHOD_LIST_CURRENT_ORDERS);
    assert_eq!(requests[0].1["orderProjection"], "EXECUTABLE");
    assert_eq!(requests[1].0, METHOD_LIST_MARKET_CATALOGUE);
    assert_eq!(
        requests[1].1["filter"],
        serde_json::json!({"bspOnly": true, "withOrders": ["EXECUTION_COMPLETE"], "marketIds": ["1.179082386"]})
    );
    assert_eq!(requests[2].0, METHOD_LIST_CURRENT_ORDERS);
    assert_eq!(requests[2].1["orderProjection"], "ALL");

    for (_, params) in requests
        .iter()
        .filter(|(method, _)| method == METHOD_LIST_CURRENT_ORDERS)
    {
        assert_eq!(params["marketIds"], serde_json::json!(["1.179082386"]));
        assert!(params.get("betIds").is_none());
        assert!(params.get("customerOrderRefs").is_none());
    }

    assert!(rx.try_recv().is_err());

    client.disconnect().await.unwrap();
    let _ = server_done_tx.send(());
    server.await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_open_only_bsp_discovery_excluded_instrument_preserves_pending_replace() {
    let (addr, state) = start_mock_http().await;
    let (stream_port, listener) = start_mock_stream().await;

    let config = BetfairExecutionClientConfig {
        reconcile_market_ids_only: true,
        reconcile_market_ids: Some(vec!["1.179082386".to_string()]),
        ..Default::default()
    };

    let (mut client, mut rx, _data_rx, cache) =
        create_test_execution_client_with_config(addr, stream_port, config);
    let (server_done_tx, server_done_rx) = tokio::sync::oneshot::channel();

    let server = tokio::spawn(async move {
        let (_reader, write_half) = accept_and_activate(&listener).await;
        let _ = server_done_rx.await;
        drop(write_half);
    });

    let instrument_id = "1.179082386-235.BETFAIR";
    let reference = "O-BSP-EXCLUDED-REPLACE";
    add_order_to_cache(
        &cache,
        make_accepted_test_order(instrument_id, reference, "excluded-old", "2.50", "10"),
    );
    connect_execution_ready(&mut client).await;

    while rx.try_recv().is_ok() {}
    replace_order_for_reports(
        &client,
        &mut rx,
        &state,
        make_price_modify_order_cmd(instrument_id, reference, "excluded-old", "3.00"),
        "excluded-new",
        true,
    )
    .await;
    let successor = current_order_leg("excluded-new", reference, 6.0, 2.0, 4.0, 3.0);
    let mut outside = resting_bsp_order("bsp-outside", "outside-ref");
    outside["marketId"] = Value::from("1.999999999");
    *state.betting_current_orders.lock() = Some(vec![
        current_order_leg("excluded-old", reference, 10.0, 4.0, 0.0, 2.5),
        successor.clone(),
        resting_bsp_order("bsp-resting", "foreign-bsp"),
        outside,
    ]);
    state.betting_request_params.lock().clear();
    state
        .betting_current_orders_returned
        .store(0, Ordering::Relaxed);
    let requests_before = state.betting_request_count.load(Ordering::Relaxed);
    let mut command = GenerateOrderStatusReportsBuilder::default()
        .ts_init(get_atomic_clock_realtime().get_time_ns())
        .open_only(true)
        .instrument_id(Some(InstrumentId::from("1.999999999-235.BETFAIR")))
        .build()
        .unwrap();

    for _ in 0..2 {
        let reports = client
            .generate_order_status_reports(&command)
            .await
            .unwrap();
        assert!(reports.is_empty());
        assert!(state.betting_request_params.lock().is_empty());
        assert_eq!(
            state.betting_request_count.load(Ordering::Relaxed),
            requests_before
        );
        assert_eq!(
            state
                .betting_current_orders_returned
                .load(Ordering::Relaxed),
            0
        );
        assert!(rx.try_recv().is_err());
    }

    command.instrument_id = Some(InstrumentId::from(instrument_id));
    let resolving = client
        .generate_order_status_reports(&command)
        .await
        .unwrap();
    let events = drain_events(&mut rx, Duration::from_millis(100)).await;
    let reports = client
        .generate_order_status_reports(&command)
        .await
        .unwrap();
    assert_eq!(resolving.len(), 1);
    assert_resting_bsp_report(
        &resolving[0],
        instrument_id,
        "bsp-resting",
        "foreign-bsp",
        OrderSide::Buy,
        OrderType::Market,
        command.ts_init,
    );
    assert_eq!(events.len(), 1);
    let updates = order_updates(&events);
    assert_eq!(updates.len(), 1);
    assert_eq!(updates[0].client_order_id, ClientOrderId::from(reference));
    assert_eq!(
        updates[0].venue_order_id,
        Some(VenueOrderId::from("excluded-new"))
    );
    assert_eq!(updates[0].quantity, Quantity::from("10"));
    assert_eq!(updates[0].price, Some(Price::from("3.00")));
    assert_eq!(reports.len(), 2);
    assert_replacement_report(
        reports
            .iter()
            .find(|report| report.venue_order_id.as_str() == "excluded-new")
            .unwrap(),
        successor,
        reference,
        "6",
        Decimal::from(8) / Decimal::from(3),
    );
    assert_resting_bsp_report(
        reports
            .iter()
            .find(|report| report.venue_order_id.as_str() == "bsp-resting")
            .unwrap(),
        instrument_id,
        "bsp-resting",
        "foreign-bsp",
        OrderSide::Buy,
        OrderType::Market,
        command.ts_init,
    );
    let requests = state.betting_request_params.lock().clone();
    assert_eq!(requests.len(), 7);
    assert_eq!(requests[3].0, METHOD_LIST_CURRENT_ORDERS);
    assert_eq!(
        requests[3].1,
        serde_json::json!({"orderProjection": "ALL", "customerOrderRefs": [make_customer_order_ref(reference)]})
    );

    for scan in [&requests[..3], &requests[4..]] {
        assert_eq!(scan[0].0, METHOD_LIST_CURRENT_ORDERS);
        assert_eq!(scan[0].1["orderProjection"], "EXECUTABLE");
        assert_eq!(scan[0].1["marketIds"], serde_json::json!(["1.179082386"]));
        assert_eq!(scan[1].0, METHOD_LIST_MARKET_CATALOGUE);
        assert_eq!(
            scan[1].1["filter"],
            serde_json::json!({"bspOnly": true, "withOrders": ["EXECUTION_COMPLETE"], "marketIds": ["1.179082386"]})
        );
        assert_eq!(scan[2].0, METHOD_LIST_CURRENT_ORDERS);
        assert_eq!(scan[2].1["orderProjection"], "ALL");
        assert_eq!(scan[2].1["marketIds"], serde_json::json!(["1.179082386"]));
    }

    assert!(rx.try_recv().is_err());
    client.disconnect().await.unwrap();
    let _ = server_done_tx.send(());
    server.await.unwrap();
}

#[rstest]
#[case::no_history(0)]
#[case::unrelated_history(30_000)]
#[tokio::test]
async fn test_open_only_bsp_discovery_paginates_without_unrelated_history(
    #[case] history_count: usize,
) {
    let (addr, state) = start_mock_http().await;
    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, _cache) = create_test_execution_client(addr, stream_port);
    let (server_done_tx, server_done_rx) = tokio::sync::oneshot::channel();

    let server = tokio::spawn(async move {
        let (_reader, write_half) = accept_and_activate(&listener).await;
        let _ = server_done_rx.await;
        drop(write_half);
    });

    connect_execution_ready(&mut client).await;

    while rx.try_recv().is_ok() {}

    let mut ordinary = current_order_leg("ordinary-open", "ordinary-ref", 7.0, 0.0, 7.0, 2.5);
    ordinary["marketId"] = Value::from("1.888888888");
    let mut history = current_order_leg("history", "history-ref", 9.0, 9.0, 0.0, 3.5);
    history["marketId"] = Value::from("1.999999999");
    let mut orders = vec![ordinary];
    orders.extend((0..history_count).map(|index| {
        let mut order = history.clone();
        order["betId"] = Value::from(format!("history-{index}"));
        order
    }));

    let resting = resting_bsp_order("bsp", "foreign-bsp");
    orders.extend((0..1001).map(|index| {
        let mut order = resting.clone();
        order["betId"] = Value::from(format!("bsp-{index}"));
        order["customerOrderRef"] = Value::from(format!("bsp-ref-{index}"));
        order
    }));

    *state.betting_current_orders.lock() = Some(orders);
    state.betting_request_params.lock().clear();
    state
        .betting_current_orders_returned
        .store(0, Ordering::Relaxed);
    let command = GenerateOrderStatusReportsBuilder::default()
        .ts_init(get_atomic_clock_realtime().get_time_ns())
        .open_only(true)
        .build()
        .unwrap();
    let reports = client
        .generate_order_status_reports(&command)
        .await
        .unwrap();
    assert_eq!(reports.len(), 1002);
    let by_bet_id: HashMap<_, _> = reports
        .iter()
        .map(|report| (report.venue_order_id.as_str(), report))
        .collect();
    assert_eq!(by_bet_id.len(), 1002);

    for index in 0..1001 {
        let bet_id = format!("bsp-{index}");
        assert_resting_bsp_report(
            by_bet_id[bet_id.as_str()],
            "1.179082386-235.BETFAIR",
            &bet_id,
            &format!("bsp-ref-{index}"),
            OrderSide::Buy,
            OrderType::Market,
            command.ts_init,
        );
    }

    let ordinary = by_bet_id["ordinary-open"];
    assert_eq!(
        ordinary.instrument_id,
        InstrumentId::from("1.888888888-235.BETFAIR")
    );
    assert_eq!(
        ordinary.client_order_id,
        Some(ClientOrderId::from("ordinary-ref"))
    );
    assert_eq!(ordinary.order_status, OrderStatus::Accepted);
    assert_eq!(ordinary.quantity, Quantity::from("7"));
    assert_eq!(ordinary.filled_qty, Quantity::from("0"));
    let mut source = current_order_leg("ordinary-open", "ordinary-ref", 7.0, 0.0, 7.0, 2.5);
    source["marketId"] = Value::from("1.888888888");
    let source = serde_json::from_value(source).unwrap();
    let mut expected =
        parse_current_order_report(&source, AccountId::from("BETFAIR-001"), ordinary.ts_init)
            .unwrap();
    expected.report_id = ordinary.report_id;
    assert_eq!(ordinary, &expected);
    let requests = state.betting_request_params.lock().clone();
    assert_eq!(requests.len(), 5);
    assert_eq!(requests[0].0, METHOD_LIST_CURRENT_ORDERS);
    assert_eq!(requests[0].1["orderProjection"], "EXECUTABLE");
    assert_eq!(requests[1].0, METHOD_LIST_EVENTS);
    assert_eq!(
        requests[1].1["filter"],
        serde_json::json!({"bspOnly": true, "withOrders": ["EXECUTION_COMPLETE"]})
    );
    assert_eq!(requests[2].0, METHOD_LIST_MARKET_CATALOGUE);
    assert_eq!(
        requests[2].1["filter"],
        serde_json::json!({"bspOnly": true, "withOrders": ["EXECUTION_COMPLETE"], "eventIds": ["bsp-event"]})
    );

    for (index, (method, params)) in requests[3..].iter().enumerate() {
        assert_eq!(method, METHOD_LIST_CURRENT_ORDERS);
        assert_eq!(params["orderProjection"], "ALL");
        assert_eq!(params["marketIds"], serde_json::json!(["1.179082386"]));
        assert!(params.get("betIds").is_none());
        assert!(params.get("customerOrderRefs").is_none());
        assert!(params.get("recordCount").is_none());

        if index == 0 {
            assert!(params.get("fromRecord").is_none());
        } else {
            assert_eq!(params["fromRecord"], 1000);
        }
    }

    assert_eq!(
        state
            .betting_current_orders_returned
            .load(Ordering::Relaxed),
        1002
    );
    assert!(rx.try_recv().is_err());
    client.disconnect().await.unwrap();
    let _ = server_done_tx.send(());
    server.await.unwrap();
}

#[rstest]
#[case::events_error(METHOD_LIST_EVENTS, false)]
#[case::catalogue_error(METHOD_LIST_MARKET_CATALOGUE, false)]
#[case::catalogue_limit(METHOD_LIST_MARKET_CATALOGUE, true)]
#[tokio::test]
async fn test_open_only_bsp_discovery_failure_preserves_pending_replace(
    #[case] method: &str,
    #[case] saturated: bool,
) {
    let (addr, state) = start_mock_http().await;
    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, cache) = create_test_execution_client(addr, stream_port);
    let (server_done_tx, server_done_rx) = tokio::sync::oneshot::channel();

    let server = tokio::spawn(async move {
        let (_reader, write_half) = accept_and_activate(&listener).await;
        let _ = server_done_rx.await;
        drop(write_half);
    });

    let instrument_id = "1.179082386-235.BETFAIR";
    let reference = "O-BSP-DISCOVERY-REPLACE";
    add_order_to_cache(
        &cache,
        make_accepted_test_order(instrument_id, reference, "discovery-old", "2.50", "10"),
    );
    connect_execution_ready(&mut client).await;

    while rx.try_recv().is_ok() {}
    replace_order_for_reports(
        &client,
        &mut rx,
        &state,
        make_price_modify_order_cmd(instrument_id, reference, "discovery-old", "3.00"),
        "discovery-new",
        true,
    )
    .await;
    let successor = current_order_leg("discovery-new", reference, 6.0, 2.0, 4.0, 3.0);
    *state.betting_current_orders.lock() = Some(vec![
        current_order_leg("discovery-old", reference, 10.0, 4.0, 0.0, 2.5),
        successor.clone(),
        resting_bsp_order("bsp-resting", "foreign-bsp"),
    ]);

    if saturated {
        let catalogues: Vec<_> = (0..1000).map(|index| serde_json::json!({"marketId": format!("1.{index}"), "marketName": "BSP market"})).collect();
        state
            .betting_overrides
            .lock()
            .insert(method.to_string(), Value::from(catalogues));
    } else {
        state
            .betting_error_overrides
            .lock()
            .insert(method.to_string(), betting_api_error("INVALID_INPUT_DATA"));
    }

    state.betting_request_params.lock().clear();
    let command = GenerateOrderStatusReportsBuilder::default()
        .ts_init(get_atomic_clock_realtime().get_time_ns())
        .open_only(true)
        .build()
        .unwrap();
    let error = client
        .generate_order_status_reports(&command)
        .await
        .unwrap_err();

    if saturated {
        assert_eq!(
            error.to_string(),
            "Betfair BSP market discovery reached the catalogue limit for event Some([\"bsp-event\"])"
        );
    } else {
        assert!(
            error.to_string().contains("INVALID_INPUT_DATA"),
            "unexpected discovery error: {error}"
        );
    }

    assert!(rx.try_recv().is_err());
    let failed_requests = state.betting_request_params.lock().clone();
    assert_eq!(failed_requests.last().unwrap().0, method);
    assert_eq!(
        failed_requests
            .iter()
            .filter(|(method, _)| method == METHOD_LIST_CURRENT_ORDERS)
            .count(),
        1
    );
    assert_eq!(failed_requests[0].1["orderProjection"], "EXECUTABLE");
    state.betting_overrides.lock().remove(method);
    state.betting_error_overrides.lock().remove(method);
    let resolving = client
        .generate_order_status_reports(&command)
        .await
        .unwrap();
    let events = drain_events(&mut rx, Duration::from_millis(100)).await;
    let reports = client
        .generate_order_status_reports(&command)
        .await
        .unwrap();
    assert_eq!(resolving.len(), 1);
    assert_resting_bsp_report(
        &resolving[0],
        instrument_id,
        "bsp-resting",
        "foreign-bsp",
        OrderSide::Buy,
        OrderType::Market,
        command.ts_init,
    );
    assert_eq!(events.len(), 1);
    let updates = order_updates(&events);
    assert_eq!(updates.len(), 1);
    assert_eq!(updates[0].client_order_id, ClientOrderId::from(reference));
    assert_eq!(
        updates[0].venue_order_id,
        Some(VenueOrderId::from("discovery-new"))
    );
    assert_eq!(updates[0].quantity, Quantity::from("10"));
    assert_eq!(updates[0].price, Some(Price::from("3.00")));
    assert_eq!(reports.len(), 2);
    let report = reports
        .iter()
        .find(|report| report.venue_order_id.as_str() == "discovery-new")
        .unwrap();
    assert_replacement_report(
        report,
        successor,
        reference,
        "6",
        Decimal::from(8) / Decimal::from(3),
    );
    assert_resting_bsp_report(
        reports
            .iter()
            .find(|report| report.venue_order_id.as_str() == "bsp-resting")
            .unwrap(),
        instrument_id,
        "bsp-resting",
        "foreign-bsp",
        OrderSide::Buy,
        OrderType::Market,
        command.ts_init,
    );
    assert!(
        state
            .betting_request_params
            .lock()
            .iter()
            .filter(|(method, _)| method == METHOD_LIST_CURRENT_ORDERS)
            .all(|(_, params)| params["orderProjection"] == "EXECUTABLE"
                || params["marketIds"].is_array()
                || params["betIds"].is_array()
                || params["customerOrderRefs"].is_array())
    );
    assert!(rx.try_recv().is_err());
    client.disconnect().await.unwrap();
    let _ = server_done_tx.send(());
    server.await.unwrap();
}

#[rstest]
#[case::events(METHOD_LIST_EVENTS)]
#[case::catalogue(METHOD_LIST_MARKET_CATALOGUE)]
#[tokio::test]
async fn test_open_only_bsp_discovery_recovers_from_no_session(#[case] method: &str) {
    let (addr, state) = start_mock_http().await;
    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, _cache) = create_test_execution_client(addr, stream_port);
    let (server_done_tx, server_done_rx) = tokio::sync::oneshot::channel();

    let server = tokio::spawn(async move {
        let (_reader, write_half) = accept_and_activate(&listener).await;
        let _ = server_done_rx.await;
        drop(write_half);
    });

    connect_execution_ready(&mut client).await;

    while rx.try_recv().is_ok() {}
    *state.betting_current_orders.lock() =
        Some(vec![resting_bsp_order("bsp-resting", "foreign-bsp")]);
    state
        .betting_error_one_shot_overrides
        .lock()
        .insert(method.to_string(), betting_api_error("NO_SESSION"));
    state.betting_request_params.lock().clear();
    let keep_alives_before = state.keep_alive_count.load(Ordering::Relaxed);
    let logins_before = state.login_count.load(Ordering::Relaxed);
    let command = GenerateOrderStatusReportsBuilder::default()
        .ts_init(get_atomic_clock_realtime().get_time_ns())
        .open_only(true)
        .build()
        .unwrap();
    let reports = client
        .generate_order_status_reports(&command)
        .await
        .unwrap();
    assert_eq!(reports.len(), 1);
    assert_resting_bsp_report(
        &reports[0],
        "1.179082386-235.BETFAIR",
        "bsp-resting",
        "foreign-bsp",
        OrderSide::Buy,
        OrderType::Market,
        command.ts_init,
    );
    let requests = state.betting_request_params.lock().clone();
    assert_eq!(requests.len(), 5);
    let retried: Vec<_> = requests
        .iter()
        .filter(|(request_method, _)| request_method == method)
        .collect();
    assert_eq!(retried.len(), 2);
    assert_eq!(retried[0].1, retried[1].1);
    assert_eq!(retried[0].1["filter"]["bspOnly"], true);
    assert_eq!(
        retried[0].1["filter"]["withOrders"],
        serde_json::json!(["EXECUTION_COMPLETE"])
    );
    let current_orders: Vec<_> = requests
        .iter()
        .filter(|(method, _)| method == METHOD_LIST_CURRENT_ORDERS)
        .collect();
    assert_eq!(current_orders.len(), 2);
    assert_eq!(current_orders[0].1["orderProjection"], "EXECUTABLE");
    assert_eq!(current_orders[1].1["orderProjection"], "ALL");
    assert_eq!(
        current_orders[1].1["marketIds"],
        serde_json::json!(["1.179082386"])
    );
    assert!(current_orders[1].1.get("betIds").is_none());
    assert!(current_orders[1].1.get("customerOrderRefs").is_none());
    assert_eq!(
        state.keep_alive_count.load(Ordering::Relaxed),
        keep_alives_before + 1
    );
    assert_eq!(state.login_count.load(Ordering::Relaxed), logins_before);
    assert!(rx.try_recv().is_err());
    client.disconnect().await.unwrap();
    let _ = server_done_tx.send(());
    server.await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_open_only_bsp_discovery_partitions_more_than_1000_markets_by_event() {
    let (addr, state) = start_mock_http().await;
    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, _cache) = create_test_execution_client(addr, stream_port);
    let (server_done_tx, server_done_rx) = tokio::sync::oneshot::channel();

    let server = tokio::spawn(async move {
        let (_reader, write_half) = accept_and_activate(&listener).await;
        let _ = server_done_rx.await;
        drop(write_half);
    });

    connect_execution_ready(&mut client).await;

    while rx.try_recv().is_ok() {}
    let market_ids: Vec<_> = (0..1001)
        .map(|index| format!("1.{}", 500_000 + index))
        .collect();
    let resting = resting_bsp_order("bsp", "foreign-bsp");

    let orders = market_ids
        .iter()
        .enumerate()
        .map(|(index, market_id)| {
            let mut order = resting.clone();
            order["marketId"] = Value::from(market_id.clone());
            order["betId"] = Value::from(format!("bsp-{index}"));
            order["customerOrderRef"] = Value::from(format!("bsp-ref-{index}"));
            order
        })
        .collect();

    *state.betting_current_orders.lock() = Some(orders);
    state.betting_overrides.lock().insert(
        METHOD_LIST_EVENTS.to_string(),
        serde_json::json!([
            {"event": {"id": "bsp-event-first"}},
            {"event": {"id": "bsp-event-second"}},
        ]),
    );

    let catalogue = |market_ids: &[String]| {
        Value::Array(market_ids.iter().map(|market_id| {
        serde_json::json!({"marketId": market_id, "marketName": "BSP market"})
    }).collect())
    };

    state.betting_response_sequences.lock().insert(
        METHOD_LIST_MARKET_CATALOGUE.to_string(),
        VecDeque::from([catalogue(&market_ids[..600]), catalogue(&market_ids[600..])]),
    );
    state.betting_request_params.lock().clear();
    state
        .betting_current_orders_returned
        .store(0, Ordering::Relaxed);
    let command = GenerateOrderStatusReportsBuilder::default()
        .ts_init(get_atomic_clock_realtime().get_time_ns())
        .open_only(true)
        .build()
        .unwrap();
    let reports = client
        .generate_order_status_reports(&command)
        .await
        .unwrap();
    assert_eq!(reports.len(), 1001);
    let by_bet_id: HashMap<_, _> = reports
        .iter()
        .map(|report| (report.venue_order_id.as_str(), report))
        .collect();
    assert_eq!(by_bet_id.len(), 1001);

    for (index, market_id) in market_ids.iter().enumerate() {
        let bet_id = format!("bsp-{index}");
        assert_resting_bsp_report(
            by_bet_id[bet_id.as_str()],
            &format!("{market_id}-235.BETFAIR"),
            &bet_id,
            &format!("bsp-ref-{index}"),
            OrderSide::Buy,
            OrderType::Market,
            command.ts_init,
        );
    }

    let requests = state.betting_request_params.lock().clone();
    assert_eq!(requests.len(), 9);
    assert_eq!(requests[0].0, METHOD_LIST_CURRENT_ORDERS);
    assert_eq!(requests[0].1["orderProjection"], "EXECUTABLE");
    assert_eq!(requests[1].0, METHOD_LIST_EVENTS);
    assert_eq!(
        requests[1].1["filter"],
        serde_json::json!({"bspOnly": true, "withOrders": ["EXECUTION_COMPLETE"]})
    );

    for ((method, params), event_id) in requests[2..4]
        .iter()
        .zip(["bsp-event-first", "bsp-event-second"])
    {
        assert_eq!(method, METHOD_LIST_MARKET_CATALOGUE);
        assert_eq!(
            params["filter"],
            serde_json::json!({"bspOnly": true, "withOrders": ["EXECUTION_COMPLETE"], "eventIds": [event_id]})
        );
        assert_eq!(params["maxResults"], 1000);
    }

    for ((method, params), expected_markets) in requests[4..].iter().zip(market_ids.chunks(250)) {
        assert_eq!(method, METHOD_LIST_CURRENT_ORDERS);
        assert_eq!(params["orderProjection"], "ALL");
        assert_eq!(params["marketIds"], serde_json::json!(expected_markets));
        assert!(params.get("betIds").is_none());
        assert!(params.get("customerOrderRefs").is_none());
        assert!(params.get("fromRecord").is_none());
    }

    assert_eq!(
        state
            .betting_current_orders_returned
            .load(Ordering::Relaxed),
        1001
    );
    assert!(rx.try_recv().is_err());
    client.disconnect().await.unwrap();
    let _ = server_done_tx.send(());
    server.await.unwrap();
}

fn resting_bsp_order(bet_id: &str, client_order_id: &str) -> Value {
    let mut order = current_order_leg(bet_id, client_order_id, 0.0, 0.0, 0.0, 1.01);
    order["orderType"] = Value::from("MARKET_ON_CLOSE");
    order["side"] = Value::from("LAY");
    order["status"] = Value::from("EXECUTION_COMPLETE");
    order["bspLiability"] = Value::from(5.0);
    order["sizeLapsed"] = Value::from(0.0);
    order["sizeVoided"] = Value::from(0.0);
    order["averagePriceMatched"] = Value::from(0.0);
    order
}

fn assert_resting_bsp_report(
    report: &OrderStatusReport,
    instrument_id: &str,
    bet_id: &str,
    reference: &str,
    side: OrderSide,
    order_type: OrderType,
    ts_init: UnixNanos,
) {
    let ts_accepted = parse_betfair_timestamp("2021-03-24T06:47:02.000Z").unwrap();
    assert!(report.ts_init >= ts_init);
    assert!(report.ts_init <= get_atomic_clock_realtime().get_time_ns());
    let mut expected = OrderStatusReport::new(
        AccountId::from("BETFAIR-001"),
        InstrumentId::from(instrument_id),
        Some(ClientOrderId::from(reference)),
        VenueOrderId::from(bet_id),
        Some(side),
        order_type,
        TimeInForce::AtTheClose,
        OrderStatus::Accepted,
        Quantity::from("5"),
        Quantity::from("0"),
        ts_accepted,
        ts_accepted,
        report.ts_init,
        Some(report.report_id),
    )
    .with_price(Price::from("1.01"));
    expected.avg_px = Some(Decimal::ZERO);
    assert_eq!(report, &expected);
}

/// A FOK limit order must serialize with `timeInForce=FILL_OR_KILL` and no
/// `persistenceType` so Betfair rejects unmatched residue rather than parking
/// it on the book.
#[rstest]
#[tokio::test]
async fn test_submit_order_fok_sends_fill_or_kill_payload() {
    let (addr, state) = start_mock_http().await;
    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, cache) = create_test_execution_client(addr, stream_port);

    let (server_done_tx, server_done_rx) = tokio::sync::oneshot::channel();

    let server = tokio::spawn(async move {
        let (_reader, write_half) = accept_and_activate(&listener).await;
        let _ = server_done_rx.await;
        drop(write_half);
    });

    connect_execution_ready(&mut client).await;

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

    wait_for_mock_state(&state, "METHOD_PLACE_ORDERS request count >= 1", |state| {
        betting_method_count(state, METHOD_PLACE_ORDERS) >= 1
    })
    .await;

    let params = state
        .betting_request_params
        .lock()
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
    let _ = server_done_tx.send(());
    server.await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_submit_limit_at_the_close_sends_limit_on_close_payload() {
    let (addr, state) = start_mock_http().await;
    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, cache) = create_test_execution_client(addr, stream_port);
    let (server_done_tx, server_done_rx) = tokio::sync::oneshot::channel();

    let server = tokio::spawn(async move {
        let (_reader, write_half) = accept_and_activate(&listener).await;
        let _ = server_done_rx.await;
        drop(write_half);
    });

    connect_execution_ready(&mut client).await;

    while rx.try_recv().is_ok() {}

    let order = OrderTestBuilder::new(OrderType::Limit)
        .trader_id(TraderId::from("TESTER-001"))
        .strategy_id(StrategyId::from("S-001"))
        .instrument_id(InstrumentId::from("1.181005744-86362-0.BETFAIR"))
        .client_order_id(ClientOrderId::from("O-LOC"))
        .side(OrderSide::Buy)
        .price(Price::from("2.50"))
        .quantity(Quantity::from("12"))
        .time_in_force(TimeInForce::AtTheClose)
        .build();
    add_order_to_cache(&cache, order.clone());
    client.submit_order(make_submit_order_cmd(&order)).unwrap();

    wait_for_mock_state(&state, "METHOD_PLACE_ORDERS request count >= 1", |state| {
        betting_method_count(state, METHOD_PLACE_ORDERS) >= 1
    })
    .await;

    let params = state
        .betting_request_params
        .lock()
        .iter()
        .find(|(method, _)| method == METHOD_PLACE_ORDERS)
        .cloned()
        .expect("placeOrders call must be recorded")
        .1;
    let instruction = &params["instructions"][0];
    assert_valid_customer_ref(&params);
    assert_eq!(instruction["orderType"], "LIMIT_ON_CLOSE");
    assert_eq!(instruction["limitOnCloseOrder"]["liability"], "12");
    assert_eq!(instruction["limitOnCloseOrder"]["price"], "2.50");
    assert!(instruction.get("limitOrder").is_none() || instruction["limitOrder"].is_null());

    client.disconnect().await.unwrap();
    let _ = server_done_tx.send(());
    server.await.unwrap();
}

/// A Market AtTheClose order must serialize as a `marketOnCloseOrder` (BSP)
/// with the order quantity used as `liability`, not as a regular limit.
#[rstest]
#[tokio::test]
async fn test_submit_order_market_on_close_sends_bsp_instruction() {
    let (addr, state) = start_mock_http().await;
    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, cache) = create_test_execution_client(addr, stream_port);

    let (server_done_tx, server_done_rx) = tokio::sync::oneshot::channel();

    let server = tokio::spawn(async move {
        let (_reader, write_half) = accept_and_activate(&listener).await;
        let _ = server_done_rx.await;
        drop(write_half);
    });

    connect_execution_ready(&mut client).await;

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

    wait_for_mock_state(&state, "METHOD_PLACE_ORDERS request count >= 1", |state| {
        betting_method_count(state, METHOD_PLACE_ORDERS) >= 1
    })
    .await;

    let params = state
        .betting_request_params
        .lock()
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
    let _ = server_done_tx.send(());
    server.await.unwrap();
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
        .insert(METHOD_PLACE_ORDERS.to_string(), 502);

    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, cache) = create_test_execution_client(addr, stream_port);

    let (server_done_tx, server_done_rx) = tokio::sync::oneshot::channel();

    let server = tokio::spawn(async move {
        let (_reader, write_half) = accept_and_activate(&listener).await;
        let _ = server_done_rx.await;
        drop(write_half);
    });

    connect_execution_ready(&mut client).await;

    while rx.try_recv().is_ok() {}

    let order = make_test_order("1.181005744-86362-0.BETFAIR", "O-AMB", "2.58", "10");
    add_order_to_cache(&cache, order.clone());

    client.submit_order(make_submit_order_cmd(&order)).unwrap();

    // Wait for the placeOrders dispatch to actually hit the mock so the
    // no-Rejected assertion below is grounded in the 5xx path having fired.
    // Without this, a regression that stops dispatching placeOrders after
    // local submit would still pass.
    wait_for_mock_state(&state, "METHOD_PLACE_ORDERS request count >= 1", |state| {
        betting_method_count(state, METHOD_PLACE_ORDERS) >= 1
    })
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
    let _ = server_done_tx.send(());
    server.await.unwrap();
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

    let (server_done_tx, server_done_rx) = tokio::sync::oneshot::channel();

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

        let _ = server_done_rx.await;
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
    let _ = server_done_tx.send(());
    server.await.unwrap();
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

    let (server_done_tx, server_done_rx) = tokio::sync::oneshot::channel();

    let server = tokio::spawn(async move {
        let (mut reader, mut write_half) = accept_and_auth(&listener).await;

        let mut line = String::new();
        tokio::io::AsyncBufReadExt::read_line(&mut reader, &mut line)
            .await
            .unwrap();

        write_lines(&mut write_half, &lines).await;

        let _ = server_done_rx.await;
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
    let _ = server_done_tx.send(());
    server.await.unwrap();
}

const RECONNECT_CONNECTION_MSG: &[u8] =
    b"{\"op\":\"connection\",\"connectionId\":\"reconnect\"}\r\n\
      {\"op\":\"status\",\"id\":1,\"statusCode\":\"SUCCESS\",\"connectionClosed\":false}\r\n\
      {\"op\":\"status\",\"id\":2,\"statusCode\":\"SUCCESS\",\"connectionClosed\":false}\r\n\
      {\"op\":\"ocm\",\"id\":2,\"pt\":1000,\"ct\":\"SUB_IMAGE\",\"oc\":[]}\r\n";
const STREAM_CLOSED_MSG: &[u8] =
    b"{\"op\":\"status\",\"id\":1,\"statusCode\":\"FAILURE\",\"connectionClosed\":true}\r\n";

/// Counts recorded calls of `method`, over the request params rather than the method list.
///
/// The mock pushes the method name first and the params second, publishing after each, so a
/// predicate over the method list can wake between the two and read params that are not yet
/// recorded. Counting the later of the two collections makes every caller of this function
/// observe a request whose payload is already readable.
fn betting_method_count(state: &MockState, method: &str) -> usize {
    state
        .betting_request_params
        .lock()
        .iter()
        .filter(|(seen, _)| seen.as_str() == method)
        .count()
}

fn response_gate_waiter_count(state: &MockState) -> usize {
    state
        .betting_response_gate
        .lock()
        .as_ref()
        .map_or(0, |gate| gate.waiters.load(Ordering::Relaxed))
}

fn submit_single_and_list(
    client: &BetfairExecutionClient,
    cache: &Rc<RefCell<Cache>>,
    suffix: &str,
) -> Vec<ClientOrderId> {
    let ids = [
        format!("O-{suffix}-SINGLE"),
        format!("O-{suffix}-LIST-1"),
        format!("O-{suffix}-LIST-2"),
    ];
    let orders = ids
        .iter()
        .map(|id| make_test_order("1.181005744-86362-0.BETFAIR", id, "2.58", "10"))
        .collect::<Vec<_>>();

    for order in &orders {
        add_order_to_cache(cache, order.clone());
    }

    client
        .submit_order(make_submit_order_cmd(&orders[0]))
        .unwrap();
    let (cmd, _) = make_submit_order_list_cmd(
        "1.181005744-86362-0.BETFAIR",
        &[orders[1].clone(), orders[2].clone()],
    );
    client.submit_order_list(cmd).unwrap();

    ids.iter()
        .map(|id| ClientOrderId::from(id.as_str()))
        .collect()
}

async fn stream_reconciling_denials(
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
    expected: usize,
) -> Vec<ClientOrderId> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
    let expected_reason = OrderDeniedReason::StreamReconciling.to_string();
    let mut denied = Vec::new();

    while denied.len() < expected && tokio::time::Instant::now() < deadline {
        if let Ok(Some(ExecutionEvent::Order(OrderEventAny::Denied(event)))) =
            tokio::time::timeout(Duration::from_millis(250), rx.recv()).await
        {
            assert_eq!(event.reason, expected_reason);
            denied.push(event.client_order_id);
        }
    }

    denied
}

#[rstest]
#[tokio::test]
async fn test_transport_loss_denies_submit_before_reconnect() {
    let (addr, state) = start_mock_http().await;
    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, cache) = create_test_execution_client(addr, stream_port);
    let (closed_tx, closed_rx) = tokio::sync::oneshot::channel();

    let server = tokio::spawn(async move {
        let (mut reader, write_half) = accept_and_auth(&listener).await;
        let mut line = String::new();
        tokio::io::AsyncBufReadExt::read_line(&mut reader, &mut line)
            .await
            .unwrap();
        drop(listener);
        drop(reader);
        drop(write_half);
        closed_tx.send(()).unwrap();
    });

    client.connect().await.unwrap();
    closed_rx.await.unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;

    let order = make_test_order(
        "1.181005744-86362-0.BETFAIR",
        "O-INACTIVE-HALT",
        "2.58",
        "10",
    );
    add_order_to_cache(&cache, order.clone());
    client.submit_order(make_submit_order_cmd(&order)).unwrap();
    let denied = stream_reconciling_denials(&mut rx, 1).await;

    assert!(client.is_reconciling());
    assert_eq!(denied, vec![ClientOrderId::from("O-INACTIVE-HALT")]);
    assert_eq!(betting_method_count(&state, METHOD_PLACE_ORDERS), 0);

    client.disconnect().await.unwrap();
    assert!(!client.is_reconciling());
    server.await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_active_replacement_stream_denies_submit_before_connection_message() {
    let (addr, state) = start_mock_http().await;
    let fixture = load_fixture("rest/list_current_orders_empty.json");
    let response: Value = serde_json::from_str(&fixture).unwrap();
    state.betting_overrides.lock().insert(
        METHOD_LIST_CURRENT_ORDERS.to_string(),
        response["result"].clone(),
    );
    let (stream_port, listener) = start_mock_stream().await;
    let mut stream_config = plain_stream_config(stream_port);
    stream_config.heartbeat_secs = Some(1);
    let (mut client, mut rx, _data_rx, cache) = create_test_execution_client_with_configs(
        addr,
        stream_config,
        BetfairExecutionClientConfig::default(),
    );
    let (replacement_active_tx, replacement_active_rx) = tokio::sync::oneshot::channel();
    let (send_connection_tx, send_connection_rx) = tokio::sync::oneshot::channel();
    let (server_done_tx, server_done_rx) = tokio::sync::oneshot::channel();

    let server = tokio::spawn(async move {
        let (mut initial_reader, initial_writer) = accept_and_auth(&listener).await;
        let mut line = String::new();
        tokio::io::AsyncBufReadExt::read_line(&mut initial_reader, &mut line)
            .await
            .unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(line.trim()).unwrap()["op"],
            "orderSubscription",
        );
        drop(initial_reader);
        drop(initial_writer);

        let (replacement, _) = listener.accept().await.unwrap();
        let (read_half, mut write_half) = replacement.into_split();
        let mut reader = tokio::io::BufReader::new(read_half);

        for expected in ["authentication", "orderSubscription"] {
            line.clear();
            tokio::io::AsyncBufReadExt::read_line(&mut reader, &mut line)
                .await
                .unwrap();
            assert_eq!(
                serde_json::from_str::<Value>(line.trim()).unwrap()["op"],
                expected,
            );
        }

        loop {
            line.clear();
            tokio::time::timeout(
                Duration::from_secs(3),
                tokio::io::AsyncBufReadExt::read_line(&mut reader, &mut line),
            )
            .await
            .expect("replacement stream did not become active")
            .unwrap();
            if serde_json::from_str::<Value>(line.trim()).unwrap()["op"] == "heartbeat" {
                break;
            }
        }
        replacement_active_tx.send(()).unwrap();

        send_connection_rx.await.unwrap();
        tokio::io::AsyncWriteExt::write_all(&mut write_half, RECONNECT_CONNECTION_MSG)
            .await
            .unwrap();
        let _ = server_done_rx.await;
    });

    client.connect().await.unwrap();

    while rx.try_recv().is_ok() {}
    tokio::time::timeout(Duration::from_secs(4), replacement_active_rx)
        .await
        .expect("replacement stream did not become active")
        .unwrap();

    let expected = submit_single_and_list(&client, &cache, "ACTIVE-NO-CONNECTION");
    let denied = stream_reconciling_denials(&mut rx, expected.len()).await;

    assert!(client.is_reconciling());
    assert_eq!(denied, expected);
    assert_eq!(betting_method_count(&state, METHOD_PLACE_ORDERS), 0);

    send_connection_tx.send(()).unwrap();
    wait_for_reconciliation_state(&client, false).await;

    client.disconnect().await.unwrap();
    let _ = server_done_tx.send(());
    server.await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_stream_closed_status_denies_submit_before_reconnect() {
    let (addr, state) = start_mock_http().await;
    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, cache) = create_test_execution_client(addr, stream_port);
    let (server_done_tx, server_done_rx) = tokio::sync::oneshot::channel();

    let server = tokio::spawn(async move {
        let (mut reader, mut write_half) = accept_and_auth(&listener).await;
        let mut line = String::new();
        tokio::io::AsyncBufReadExt::read_line(&mut reader, &mut line)
            .await
            .unwrap();
        tokio::io::AsyncWriteExt::write_all(&mut write_half, STREAM_CLOSED_MSG)
            .await
            .unwrap();
        let _ = server_done_rx.await;
        drop(write_half);
    });

    client.connect().await.unwrap();

    while rx.try_recv().is_ok() {}
    wait_for_reconciliation_state(&client, true).await;

    let order = make_test_order(
        "1.181005744-86362-0.BETFAIR",
        "O-STREAM-CLOSED-HALT",
        "2.58",
        "10",
    );
    add_order_to_cache(&cache, order.clone());
    client.submit_order(make_submit_order_cmd(&order)).unwrap();
    let denied = stream_reconciling_denials(&mut rx, 1).await;

    assert_eq!(denied, vec![ClientOrderId::from("O-STREAM-CLOSED-HALT")]);
    assert_eq!(betting_method_count(&state, METHOD_PLACE_ORDERS), 0);

    client.disconnect().await.unwrap();
    let _ = server_done_tx.send(());
    server.await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_degraded_order_stream_denies_submit_until_current_heartbeat() {
    let (addr, state) = start_mock_http().await;
    let fixture = load_fixture("rest/list_current_orders_empty.json");
    let response: Value = serde_json::from_str(&fixture).unwrap();
    state.betting_overrides.lock().insert(
        METHOD_LIST_CURRENT_ORDERS.to_string(),
        response["result"].clone(),
    );
    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, cache) = create_test_execution_client(addr, stream_port);
    let (stream_tx, mut stream_rx) = tokio::sync::mpsc::unbounded_channel::<&'static [u8]>();

    let server = tokio::spawn(async move {
        let (_reader, mut write_half) = accept_and_activate(&listener).await;
        while let Some(message) = stream_rx.recv().await {
            tokio::io::AsyncWriteExt::write_all(&mut write_half, message)
                .await
                .unwrap();
        }
    });

    connect_execution_ready(&mut client).await;

    while rx.try_recv().is_ok() {}

    stream_tx
        .send(b"{\"op\":\"ocm\",\"id\":2,\"pt\":1001,\"ct\":\"HEARTBEAT\",\"status\":503}\r\n")
        .unwrap();
    wait_for_connection_state(&client, false).await;

    let denied = make_test_order(
        "1.181005744-86362-0.BETFAIR",
        "O-DEGRADED-DENIED",
        "2.58",
        "10",
    );
    add_order_to_cache(&cache, denied.clone());
    client.submit_order(make_submit_order_cmd(&denied)).unwrap();
    assert_eq!(
        stream_reconciling_denials(&mut rx, 1).await,
        vec![denied.client_order_id()],
    );
    assert_eq!(betting_method_count(&state, METHOD_PLACE_ORDERS), 0);
    assert!(client.is_reconciling());

    stream_tx
        .send(b"{\"op\":\"ocm\",\"id\":2,\"pt\":1002,\"ct\":\"HEARTBEAT\"}\r\n")
        .unwrap();
    wait_for_connection_state(&client, true).await;

    let allowed = make_test_order(
        "1.181005744-86362-0.BETFAIR",
        "O-DEGRADED-ALLOWED",
        "2.58",
        "10",
    );
    add_order_to_cache(&cache, allowed.clone());
    client
        .submit_order(make_submit_order_cmd(&allowed))
        .unwrap();
    wait_for_mock_state(&state, "METHOD_PLACE_ORDERS request count == 1", |state| {
        betting_method_count(state, METHOD_PLACE_ORDERS) == 1
    })
    .await;

    assert!(client.is_connected());
    assert_eq!(betting_method_count(&state, METHOD_PLACE_ORDERS), 1);
    client.disconnect().await.unwrap();
    drop(stream_tx);
    server.await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_post_reconnect_dispatches_mass_status() {
    let (addr, state) = start_mock_http().await;

    let fixture = load_fixture("rest/list_current_orders_empty.json");
    let v: Value = serde_json::from_str(&fixture).unwrap();
    state
        .betting_overrides
        .lock()
        .insert(METHOD_LIST_CURRENT_ORDERS.to_string(), v["result"].clone());

    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, _cache) = create_test_execution_client(addr, stream_port);

    let (server_done_tx, server_done_rx) = tokio::sync::oneshot::channel();

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

        let _ = server_done_rx.await;
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

    wait_for_reconciliation_state(&client, false).await;
    assert!(!client.is_reconciling());

    client.disconnect().await.unwrap();
    let _ = server_done_tx.send(());
    server.await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_command_before_reconnect_image_keeps_recovery_pending() {
    let (addr, state) = start_mock_http().await;
    let fixture = load_fixture("rest/list_current_orders_empty.json");
    let response: Value = serde_json::from_str(&fixture).unwrap();
    state.betting_overrides.lock().insert(
        METHOD_LIST_CURRENT_ORDERS.to_string(),
        response["result"].clone(),
    );

    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, cache) = create_test_execution_client(addr, stream_port);
    let (reconnect_tx, reconnect_rx) = tokio::sync::oneshot::channel();
    let (image_tx, image_rx) = tokio::sync::oneshot::channel();
    let (server_done_tx, server_done_rx) = tokio::sync::oneshot::channel();

    let server = tokio::spawn(async move {
        let (_reader, mut write_half) = accept_and_activate(&listener).await;
        reconnect_rx.await.unwrap();
        tokio::io::AsyncWriteExt::write_all(
            &mut write_half,
            b"{\"op\":\"connection\",\"connectionId\":\"reconnect\"}\r\n\
              {\"op\":\"status\",\"id\":1,\"statusCode\":\"SUCCESS\",\"connectionClosed\":false}\r\n\
              {\"op\":\"status\",\"id\":2,\"statusCode\":\"SUCCESS\",\"connectionClosed\":false}\r\n",
        )
        .await
        .unwrap();
        image_rx.await.unwrap();
        tokio::io::AsyncWriteExt::write_all(
            &mut write_half,
            b"{\"op\":\"ocm\",\"id\":2,\"pt\":1001,\"ct\":\"SUB_IMAGE\",\"oc\":[]}\r\n",
        )
        .await
        .unwrap();
        let _ = server_done_rx.await;
    });

    connect_execution_ready(&mut client).await;
    reconnect_tx.send(()).unwrap();
    wait_for_reconciliation_state(&client, true).await;

    while rx.try_recv().is_ok() {}

    let order = make_test_order(
        "1.181005744-86362-0.BETFAIR",
        "O-BEFORE-IMAGE",
        "2.58",
        "10",
    );
    add_order_to_cache(&cache, order.clone());
    client.submit_order(make_submit_order_cmd(&order)).unwrap();
    assert_eq!(
        stream_reconciling_denials(&mut rx, 1).await,
        vec![order.client_order_id()],
    );

    image_tx.send(()).unwrap();
    let mass_status = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Some(ExecutionEvent::Report(ExecutionReport::MassStatus(status))) =
                rx.recv().await
            {
                break status;
            }
        }
    })
    .await
    .expect("recovery was not queued after the replacement image");

    assert!(mass_status.order_reports().is_empty());
    wait_for_reconciliation_state(&client, false).await;
    assert!(!client.is_reconciling());
    client.disconnect().await.unwrap();
    let _ = server_done_tx.send(());
    server.await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_post_reconnect_paginates_coherent_fill_snapshot() {
    let (addr, state) = start_mock_http().await;
    let fixture = load_fixture("rest/list_current_orders_execution_complete.json");
    let mut response: Value = serde_json::from_str(&fixture).unwrap();
    let matched = nautilus_core::time::get_atomic_clock_realtime()
        .get_time_ns()
        .saturating_sub(nautilus_core::DurationNanos::from_secs(60));

    for order in response["result"]["currentOrders"].as_array_mut().unwrap() {
        if order.get("matchedDate").is_some() {
            order["matchedDate"] = Value::from(matched.to_rfc3339());
        }
    }

    let orders = response["result"]["currentOrders"]
        .as_array()
        .expect("currentOrders must be an array");

    let mut page1 = response["result"].clone();
    page1["currentOrders"] = Value::Array(vec![orders[0].clone(), orders[1].clone()]);
    page1["currentOrders"][1]["sizeMatched"] = Value::from(4);
    page1["moreAvailable"] = Value::Bool(true);
    let mut page2 = response["result"].clone();
    page2["currentOrders"] = Value::Array(vec![orders[1].clone(), orders[2].clone()]);
    page2["currentOrders"][0]["sizeMatched"] = Value::from(8);
    page2["moreAvailable"] = Value::Bool(false);
    state.betting_response_sequences.lock().insert(
        METHOD_LIST_CURRENT_ORDERS.to_string(),
        VecDeque::from([page1, page2]),
    );

    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, _cache) = create_test_execution_client(addr, stream_port);
    let (server_done_tx, server_done_rx) = tokio::sync::oneshot::channel();

    let server = tokio::spawn(async move {
        let (mut reader, mut write_half) = accept_and_auth(&listener).await;
        let mut line = String::new();
        tokio::io::AsyncBufReadExt::read_line(&mut reader, &mut line)
            .await
            .unwrap();
        tokio::io::AsyncWriteExt::write_all(&mut write_half, RECONNECT_CONNECTION_MSG)
            .await
            .unwrap();
        let _ = server_done_rx.await;
    });

    client.connect().await.unwrap();

    let mass_status = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Some(ExecutionEvent::Report(ExecutionReport::MassStatus(status))) =
                rx.recv().await
            {
                break status;
            }
        }
    })
    .await
    .expect("paginated recovery did not publish mass status");

    let fill_count = mass_status
        .fill_reports()
        .values()
        .map(Vec::len)
        .sum::<usize>();
    assert_eq!(mass_status.order_reports().len(), 3);
    assert_eq!(fill_count, 2);
    assert!(!client.is_reconciling());

    let list_params = state
        .betting_request_params
        .lock()
        .iter()
        .filter(|(method, _)| method == METHOD_LIST_CURRENT_ORDERS)
        .map(|(_, params)| params.clone())
        .collect::<Vec<_>>();
    assert_eq!(list_params.len(), 2);

    for params in &list_params {
        assert_eq!(params["orderProjection"], "ALL");
        assert!(params.get("orderBy").is_none());
        assert!(params.get("dateRange").is_none());
    }

    assert!(list_params[0].get("fromRecord").is_none());
    assert_eq!(list_params[1]["fromRecord"], 2);
    let bet_id = VenueOrderId::from("228059821049");
    assert_eq!(
        mass_status.order_reports()[&bet_id].filled_qty,
        Quantity::from(8)
    );
    assert_eq!(mass_status.fill_reports()[&bet_id].len(), 1);
    assert_eq!(
        mass_status.fill_reports()[&bet_id][0].last_qty,
        Quantity::from(8)
    );

    client.disconnect().await.unwrap();
    let _ = server_done_tx.send(());
    server.await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_post_reconnect_retries_transient_mass_status_failure() {
    let (addr, state) = start_mock_http().await;
    let fixture = load_fixture("rest/list_current_orders_execution_complete.json");
    let mut response: Value = serde_json::from_str(&fixture).unwrap();
    let matched = nautilus_core::time::get_atomic_clock_realtime()
        .get_time_ns()
        .saturating_sub(nautilus_core::DurationNanos::from_secs(60));

    for order in response["result"]["currentOrders"].as_array_mut().unwrap() {
        if order.get("matchedDate").is_some() {
            order["matchedDate"] = Value::from(matched.to_rfc3339());
        }
    }

    state.betting_overrides.lock().insert(
        METHOD_LIST_CURRENT_ORDERS.to_string(),
        response["result"].clone(),
    );
    state.betting_error_one_shot_overrides.lock().insert(
        METHOD_LIST_CURRENT_ORDERS.to_string(),
        jsonrpc_error(-32603, "Internal error"),
    );

    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, _cache) = create_test_execution_client(addr, stream_port);
    let (server_done_tx, server_done_rx) = tokio::sync::oneshot::channel();

    let server = tokio::spawn(async move {
        let (mut reader, mut write_half) = accept_and_auth(&listener).await;
        let mut line = String::new();
        tokio::io::AsyncBufReadExt::read_line(&mut reader, &mut line)
            .await
            .unwrap();
        tokio::io::AsyncWriteExt::write_all(&mut write_half, RECONNECT_CONNECTION_MSG)
            .await
            .unwrap();
        let _ = server_done_rx.await;
    });

    client.connect().await.unwrap();

    let mut mass_status_counts = None;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while tokio::time::Instant::now() < deadline {
        if let Ok(Some(ExecutionEvent::Report(ExecutionReport::MassStatus(status)))) =
            tokio::time::timeout(Duration::from_millis(250), rx.recv()).await
        {
            mass_status_counts = Some((
                status.order_reports().len(),
                status.fill_reports().values().map(Vec::len).sum::<usize>(),
            ));
            break;
        }
    }

    assert_eq!(mass_status_counts, Some((3, 2)));
    assert!(!client.is_reconciling());
    assert_eq!(betting_method_count(&state, METHOD_LIST_CURRENT_ORDERS), 2);
    {
        let params = state.betting_request_params.lock();
        for (_, params) in params
            .iter()
            .filter(|(method, _)| method == METHOD_LIST_CURRENT_ORDERS)
        {
            assert_eq!(params["orderProjection"], "ALL");
            assert!(params.get("orderBy").is_none());
            assert!(params.get("dateRange").is_none());
        }
    }

    client.disconnect().await.unwrap();
    let _ = server_done_tx.send(());
    server.await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_post_reconnect_retry_exhaustion_keeps_submissions_halted() {
    let (addr, state) = start_mock_http().await;
    state.betting_error_overrides.lock().insert(
        METHOD_LIST_CURRENT_ORDERS.to_string(),
        jsonrpc_error(-32603, "Internal error"),
    );
    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, cache) = create_test_execution_client(addr, stream_port);
    let (server_done_tx, server_done_rx) = tokio::sync::oneshot::channel();

    let server = tokio::spawn(async move {
        let (mut reader, mut write_half) = accept_and_auth(&listener).await;
        let mut line = String::new();
        tokio::io::AsyncBufReadExt::read_line(&mut reader, &mut line)
            .await
            .unwrap();
        tokio::io::AsyncWriteExt::write_all(&mut write_half, RECONNECT_CONNECTION_MSG)
            .await
            .unwrap();
        let _ = server_done_rx.await;
    });

    client.connect().await.unwrap();
    wait_for_mock_state(
        &state,
        "METHOD_LIST_CURRENT_ORDERS request count == 4",
        |state| betting_method_count(state, METHOD_LIST_CURRENT_ORDERS) == 4,
    )
    .await;

    let expected = submit_single_and_list(&client, &cache, "RETRY-EXHAUSTED");
    let denied = stream_reconciling_denials(&mut rx, expected.len()).await;
    assert_eq!(denied, expected);
    assert!(client.is_reconciling());
    assert_eq!(betting_method_count(&state, METHOD_LIST_CURRENT_ORDERS), 4);
    assert_eq!(betting_method_count(&state, METHOD_PLACE_ORDERS), 0);
    let mut saw_mass_status = false;
    while let Ok(event) = rx.try_recv() {
        saw_mass_status |= matches!(
            event,
            ExecutionEvent::Report(ExecutionReport::MassStatus(_))
        );
    }
    assert!(
        !saw_mass_status,
        "an exhausted recovery must not publish mass status"
    );

    client.disconnect().await.unwrap();
    let _ = server_done_tx.send(());
    server.await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_post_reconnect_relogin_waits_for_reconciliation_and_does_not_loop() {
    let (addr, state) = start_mock_http().await;

    let fixture = load_fixture("rest/list_current_orders_empty.json");
    let v: Value = serde_json::from_str(&fixture).unwrap();
    state
        .betting_overrides
        .lock()
        .insert(METHOD_LIST_CURRENT_ORDERS.to_string(), v["result"].clone());
    let response_gate = MockResponseGate {
        method: METHOD_LIST_CURRENT_ORDERS.to_string(),
        waiters: Arc::new(AtomicUsize::new(0)),
        semaphore: Arc::new(tokio::sync::Semaphore::new(0)),
    };
    let server_gate = response_gate.clone();

    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, _rx, _data_rx, _cache) = create_test_execution_client(addr, stream_port);
    let (trigger_tx, trigger_rx) = tokio::sync::oneshot::channel();
    let server_state = state.clone();

    let server = tokio::spawn(async move {
        let (mut reader, write_half) = accept_and_auth(&listener).await;
        let mut initial_sub = String::new();
        tokio::io::AsyncBufReadExt::read_line(&mut reader, &mut initial_sub)
            .await
            .unwrap();
        trigger_rx.await.unwrap();

        let mut write_half = write_half;
        tokio::io::AsyncWriteExt::write_all(&mut write_half, RECONNECT_CONNECTION_MSG)
            .await
            .unwrap();

        wait_for_mock_state(&server_state, "response gate waiter count 1", |state| {
            response_gate_waiter_count(state) == 1
        })
        .await;

        assert!(
            tokio::time::timeout(Duration::from_millis(300), listener.accept())
                .await
                .is_err(),
            "full re-login must not replace the stream before reconciliation completes",
        );
        server_gate.semaphore.add_permits(1);

        let (socket, _) = tokio::time::timeout(Duration::from_secs(5), listener.accept())
            .await
            .expect("full re-login must replace the execution stream")
            .unwrap();
        let (read_half, mut replacement_write_half) = socket.into_split();
        let mut replacement_reader = tokio::io::BufReader::new(read_half);

        let mut auth = String::new();
        tokio::io::AsyncBufReadExt::read_line(&mut replacement_reader, &mut auth)
            .await
            .unwrap();
        let auth_json: Value = serde_json::from_str(&auth).unwrap();
        assert_eq!(auth_json["op"], "authentication");
        assert_eq!(auth_json["session"], "REFRESHED_SESSION_TOKEN");

        let mut replayed_sub = String::new();
        tokio::io::AsyncBufReadExt::read_line(&mut replacement_reader, &mut replayed_sub)
            .await
            .unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&replayed_sub).unwrap(),
            serde_json::from_str::<Value>(&initial_sub).unwrap(),
        );
        *server_state.betting_response_gate.lock() = None;

        let mut keep_alive_response: Value =
            serde_json::from_str(&load_fixture("rest/login_success.json")).unwrap();
        keep_alive_response["token"] = Value::String("REFRESHED_SESSION_TOKEN".to_string());
        *server_state.keep_alive_response_override.lock() =
            Some(serde_json::to_string(&keep_alive_response).unwrap());

        tokio::time::sleep(Duration::from_millis(300)).await;
        tokio::io::AsyncWriteExt::write_all(
            &mut replacement_write_half,
            b"{\"op\":\"connection\",\"connectionId\":\"ordinary-keepalive\"}\r\n",
        )
        .await
        .unwrap();

        assert!(
            tokio::time::timeout(Duration::from_millis(1_500), listener.accept())
                .await
                .is_err(),
            "successful keep-alive must not request another stream reconnect",
        );
    });

    client.connect().await.unwrap();

    *state.betting_response_gate.lock() = Some(response_gate);
    let mut login_response: Value =
        serde_json::from_str(&load_fixture("rest/login_success.json")).unwrap();
    login_response["token"] = Value::String("REFRESHED_SESSION_TOKEN".to_string());
    *state.login_response_override.lock() = Some(serde_json::to_string(&login_response).unwrap());
    *state.keep_alive_response_override.lock() = Some(load_fixture("rest/login_failure.json"));
    trigger_tx.send(()).unwrap();

    server.await.unwrap();
    assert_eq!(
        state.login_count.load(std::sync::atomic::Ordering::Relaxed),
        2,
        "the reconnect handler must perform exactly one full re-login",
    );

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_reconnect_transient_keep_alive_failure_continues_reconciliation() {
    let (addr, state) = start_mock_http().await;
    let fixture = load_fixture("rest/list_current_orders_empty.json");
    let response: Value = serde_json::from_str(&fixture).unwrap();
    state.betting_overrides.lock().insert(
        METHOD_LIST_CURRENT_ORDERS.to_string(),
        response["result"].clone(),
    );

    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, _cache) = create_test_execution_client(addr, stream_port);
    let (trigger_tx, trigger_rx) = tokio::sync::oneshot::channel();
    let (server_done_tx, server_done_rx) = tokio::sync::oneshot::channel();

    let server = tokio::spawn(async move {
        let (mut reader, mut write_half) = accept_and_auth(&listener).await;
        let mut line = String::new();
        tokio::io::AsyncBufReadExt::read_line(&mut reader, &mut line)
            .await
            .unwrap();
        trigger_rx.await.unwrap();
        tokio::io::AsyncWriteExt::write_all(&mut write_half, RECONNECT_CONNECTION_MSG)
            .await
            .unwrap();
        let _ = server_done_rx.await;
        drop(write_half);
    });

    client.connect().await.unwrap();

    while rx.try_recv().is_ok() {}

    *state.keep_alive_status_override.lock() = Some(503);
    trigger_tx.send(()).unwrap();

    let mut saw_mass_status = false;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while tokio::time::Instant::now() < deadline {
        if let Ok(Some(ExecutionEvent::Report(ExecutionReport::MassStatus(_)))) =
            tokio::time::timeout(Duration::from_millis(250), rx.recv()).await
        {
            saw_mass_status = true;
            break;
        }
    }

    assert!(
        saw_mass_status,
        "reconciliation must continue after a transient keep-alive failure",
    );
    wait_for_reconciliation_state(&client, false).await;

    assert_eq!(state.keep_alive_count.load(Ordering::Relaxed), 1);
    assert_eq!(state.login_count.load(Ordering::Relaxed), 1);
    assert_eq!(betting_method_count(&state, METHOD_LIST_CURRENT_ORDERS), 1);
    assert!(!client.is_reconciling());

    client.disconnect().await.unwrap();
    let _ = server_done_tx.send(());
    server.await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_reconnect_auth_failure_keeps_submissions_halted() {
    let (addr, state) = start_mock_http().await;
    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, cache) = create_test_execution_client(addr, stream_port);
    let (trigger_tx, trigger_rx) = tokio::sync::oneshot::channel();
    let (server_done_tx, server_done_rx) = tokio::sync::oneshot::channel();

    let server = tokio::spawn(async move {
        let (mut reader, mut write_half) = accept_and_auth(&listener).await;
        let mut line = String::new();
        tokio::io::AsyncBufReadExt::read_line(&mut reader, &mut line)
            .await
            .unwrap();
        trigger_rx.await.unwrap();
        tokio::io::AsyncWriteExt::write_all(&mut write_half, RECONNECT_CONNECTION_MSG)
            .await
            .unwrap();
        let _ = server_done_rx.await;
        drop(write_half);
    });

    client.connect().await.unwrap();

    while rx.try_recv().is_ok() {}

    *state.keep_alive_response_override.lock() = Some(load_fixture("rest/login_failure.json"));
    *state.login_response_override.lock() = Some(load_fixture("rest/login_failure.json"));
    trigger_tx.send(()).unwrap();

    wait_for_mock_state(&state, "keep-alive count 1 and login count 2", |state| {
        state.keep_alive_count.load(Ordering::Relaxed) >= 1
            && state.login_count.load(Ordering::Relaxed) >= 2
    })
    .await;
    tokio::time::sleep(Duration::from_millis(100)).await;

    let expected = submit_single_and_list(&client, &cache, "AUTH-HALT");
    let denied = stream_reconciling_denials(&mut rx, expected.len()).await;

    assert!(client.is_reconciling());
    assert_eq!(denied, expected);
    assert_eq!(betting_method_count(&state, METHOD_PLACE_ORDERS), 0);

    client.disconnect().await.unwrap();
    assert!(!client.is_reconciling());
    let _ = server_done_tx.send(());
    server.await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_reconnect_mass_status_failure_recovers_on_later_reconnect() {
    let (addr, state) = start_mock_http().await;
    let fixture = load_fixture("rest/list_current_orders_execution_complete.json");
    let mut response: Value = serde_json::from_str(&fixture).unwrap();
    let matched = nautilus_core::time::get_atomic_clock_realtime()
        .get_time_ns()
        .saturating_sub(nautilus_core::DurationNanos::from_secs(60));

    for order in response["result"]["currentOrders"].as_array_mut().unwrap() {
        if order.get("matchedDate").is_some() {
            order["matchedDate"] = Value::from(matched.to_rfc3339());
        }
    }

    state.betting_overrides.lock().insert(
        METHOD_LIST_CURRENT_ORDERS.to_string(),
        response["result"].clone(),
    );
    state.betting_error_overrides.lock().insert(
        METHOD_LIST_CURRENT_ORDERS.to_string(),
        jsonrpc_error(-32603, "Internal error"),
    );

    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, cache) = create_test_execution_client(addr, stream_port);
    let (retry_tx, retry_rx) = tokio::sync::oneshot::channel();
    let (server_done_tx, server_done_rx) = tokio::sync::oneshot::channel();

    let server = tokio::spawn(async move {
        let (mut reader, mut write_half) = accept_and_auth(&listener).await;
        let mut line = String::new();
        tokio::io::AsyncBufReadExt::read_line(&mut reader, &mut line)
            .await
            .unwrap();
        tokio::io::AsyncWriteExt::write_all(&mut write_half, RECONNECT_CONNECTION_MSG)
            .await
            .unwrap();
        retry_rx.await.unwrap();
        tokio::io::AsyncWriteExt::write_all(&mut write_half, STREAM_CLOSED_MSG)
            .await
            .unwrap();
        tokio::io::AsyncWriteExt::write_all(&mut write_half, RECONNECT_CONNECTION_MSG)
            .await
            .unwrap();
        let _ = server_done_rx.await;
        drop(write_half);
    });

    client.connect().await.unwrap();

    while rx.try_recv().is_ok() {}

    wait_for_mock_state(
        &state,
        "METHOD_LIST_CURRENT_ORDERS request count >= 1",
        |state| betting_method_count(state, METHOD_LIST_CURRENT_ORDERS) >= 1,
    )
    .await;
    tokio::time::sleep(Duration::from_millis(100)).await;

    let failed_expected = submit_single_and_list(&client, &cache, "MASS-FAILED-HALT");
    let failed_denied = stream_reconciling_denials(&mut rx, failed_expected.len()).await;

    assert!(client.is_reconciling());
    assert_eq!(failed_denied, failed_expected);
    assert_eq!(betting_method_count(&state, METHOD_LIST_CURRENT_ORDERS), 1);
    assert_eq!(betting_method_count(&state, METHOD_PLACE_ORDERS), 0);

    let failed_params = state
        .betting_request_params
        .lock()
        .iter()
        .find(|(method, _)| method == METHOD_LIST_CURRENT_ORDERS)
        .cloned()
        .expect("failed order recovery request must be recorded")
        .1;
    assert!(
        failed_params.get("dateRange").is_none(),
        "snapshot recovery must retain broad order coverage",
    );

    let response_gate = MockResponseGate {
        method: METHOD_LIST_CURRENT_ORDERS.to_string(),
        waiters: Arc::new(AtomicUsize::new(0)),
        semaphore: Arc::new(tokio::sync::Semaphore::new(0)),
    };
    *state.betting_response_gate.lock() = Some(response_gate.clone());
    state
        .betting_error_overrides
        .lock()
        .remove(METHOD_LIST_CURRENT_ORDERS);
    retry_tx.send(()).unwrap();

    wait_for_mock_state(&state, "response gate waiter count 1", |state| {
        response_gate_waiter_count(state) == 1
    })
    .await;

    let expected = submit_single_and_list(&client, &cache, "MASS-HALT");
    let denied = stream_reconciling_denials(&mut rx, expected.len()).await;

    assert!(client.is_reconciling());
    assert_eq!(denied, expected);
    assert_eq!(betting_method_count(&state, METHOD_PLACE_ORDERS), 0);

    response_gate.semaphore.add_permits(1);

    let mut recovered_counts = None;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
    while tokio::time::Instant::now() < deadline {
        if let Ok(Some(ExecutionEvent::Report(ExecutionReport::MassStatus(status)))) =
            tokio::time::timeout(Duration::from_millis(250), rx.recv()).await
        {
            let fill_count = status.fill_reports().values().map(Vec::len).sum::<usize>();
            recovered_counts = Some((status.order_reports().len(), fill_count));
            break;
        }
    }
    wait_for_reconciliation_state(&client, false).await;

    let allowed = make_test_order(
        "1.181005744-86362-0.BETFAIR",
        "O-MASS-RECOVERED",
        "2.58",
        "10",
    );
    add_order_to_cache(&cache, allowed.clone());
    client
        .submit_order(make_submit_order_cmd(&allowed))
        .unwrap();
    wait_for_mock_state(&state, "METHOD_PLACE_ORDERS request count == 1", |state| {
        betting_method_count(state, METHOD_PLACE_ORDERS) == 1
    })
    .await;

    assert_eq!(recovered_counts, Some((3, 2)));
    assert!(!client.is_reconciling());
    assert_eq!(response_gate.waiters.load(Ordering::Relaxed), 1);
    assert_eq!(betting_method_count(&state, METHOD_LIST_CURRENT_ORDERS), 2);
    assert_eq!(betting_method_count(&state, METHOD_PLACE_ORDERS), 1);

    let list_params = state
        .betting_request_params
        .lock()
        .iter()
        .filter(|(method, _)| method == METHOD_LIST_CURRENT_ORDERS)
        .map(|(_, params)| params.clone())
        .collect::<Vec<_>>();
    assert_eq!(list_params.len(), 2);
    assert!(list_params[0].get("dateRange").is_none());
    assert!(list_params[1].get("dateRange").is_none());

    client.disconnect().await.unwrap();
    assert!(!client.is_reconciling());
    let _ = server_done_tx.send(());
    server.await.unwrap();
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
        .insert(METHOD_LIST_CURRENT_ORDERS.to_string(), v["result"].clone());
    let response_gate = MockResponseGate {
        method: METHOD_LIST_CURRENT_ORDERS.to_string(),
        waiters: Arc::new(AtomicUsize::new(0)),
        semaphore: Arc::new(tokio::sync::Semaphore::new(0)),
    };
    *state.betting_response_gate.lock() = Some(response_gate.clone());

    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, cache) = create_test_execution_client(addr, stream_port);

    let (server_done_tx, server_done_rx) = tokio::sync::oneshot::channel();

    let server = tokio::spawn(async move {
        let (mut reader, mut write_half) = accept_and_auth(&listener).await;
        let mut line = String::new();
        tokio::io::AsyncBufReadExt::read_line(&mut reader, &mut line)
            .await
            .unwrap();
        tokio::io::AsyncWriteExt::write_all(&mut write_half, RECONNECT_CONNECTION_MSG)
            .await
            .unwrap();
        let _ = server_done_rx.await;
        drop(write_half);
    });

    client.connect().await.unwrap();

    while rx.try_recv().is_ok() {}

    wait_for_mock_state(&state, "response gate waiter count 1", |state| {
        response_gate_waiter_count(state) == 1
    })
    .await;
    assert!(client.is_reconciling());

    while rx.try_recv().is_ok() {}

    let order = make_test_order("1.181005744-86362-0.BETFAIR", "O-HALT-001", "2.58", "10");
    add_order_to_cache(&cache, order.clone());
    client.submit_order(make_submit_order_cmd(&order)).unwrap();

    let denied = stream_reconciling_denials(&mut rx, 1).await;
    assert_eq!(denied, vec![ClientOrderId::from("O-HALT-001")]);
    assert_eq!(betting_method_count(&state, METHOD_PLACE_ORDERS), 0);

    response_gate.semaphore.add_permits(1);

    let mut saw_mass_status = false;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
    while tokio::time::Instant::now() < deadline {
        if let Ok(Some(ExecutionEvent::Report(ExecutionReport::MassStatus(_)))) =
            tokio::time::timeout(Duration::from_millis(250), rx.recv()).await
        {
            saw_mass_status = true;
            break;
        }
    }
    wait_for_reconciliation_state(&client, false).await;

    let allowed = make_test_order(
        "1.181005744-86362-0.BETFAIR",
        "O-HALT-ALLOWED",
        "2.58",
        "10",
    );
    add_order_to_cache(&cache, allowed.clone());
    client
        .submit_order(make_submit_order_cmd(&allowed))
        .unwrap();
    wait_for_mock_state(&state, "METHOD_PLACE_ORDERS request count == 1", |state| {
        betting_method_count(state, METHOD_PLACE_ORDERS) == 1
    })
    .await;

    assert!(saw_mass_status);
    assert!(!client.is_reconciling());
    assert_eq!(betting_method_count(&state, METHOD_PLACE_ORDERS), 1);

    client.disconnect().await.unwrap();
    let _ = server_done_tx.send(());
    server.await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_queued_reconnect_generation_stays_halted() {
    let (addr, state) = start_mock_http().await;

    let fixture = load_fixture("rest/list_current_orders_empty.json");
    let v: Value = serde_json::from_str(&fixture).unwrap();
    state
        .betting_overrides
        .lock()
        .insert(METHOD_LIST_CURRENT_ORDERS.to_string(), v["result"].clone());
    let response_gate = MockResponseGate {
        method: METHOD_LIST_CURRENT_ORDERS.to_string(),
        waiters: Arc::new(AtomicUsize::new(0)),
        semaphore: Arc::new(tokio::sync::Semaphore::new(0)),
    };
    *state.betting_response_gate.lock() = Some(response_gate.clone());
    let server_gate = response_gate.clone();

    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, _cache) = create_test_execution_client(addr, stream_port);

    let (release_second_tx, release_second_rx) = tokio::sync::oneshot::channel();
    let (server_done_tx, server_done_rx) = tokio::sync::oneshot::channel();
    let server_state = state.clone();

    let server = tokio::spawn(async move {
        let (mut reader, mut write_half) = accept_and_auth(&listener).await;
        let mut line = String::new();
        tokio::io::AsyncBufReadExt::read_line(&mut reader, &mut line)
            .await
            .unwrap();

        tokio::io::AsyncWriteExt::write_all(&mut write_half, RECONNECT_CONNECTION_MSG)
            .await
            .unwrap();

        wait_for_mock_state(&server_state, "response gate waiter count 1", |state| {
            response_gate_waiter_count(state) == 1
        })
        .await;

        tokio::io::AsyncWriteExt::write_all(&mut write_half, STREAM_CLOSED_MSG)
            .await
            .unwrap();
        tokio::io::AsyncWriteExt::write_all(&mut write_half, RECONNECT_CONNECTION_MSG)
            .await
            .unwrap();

        server_gate.semaphore.add_permits(1);
        wait_for_mock_state(&server_state, "response gate waiter count 2", |state| {
            response_gate_waiter_count(state) == 2
        })
        .await;

        release_second_rx.await.unwrap();
        server_gate.semaphore.add_permits(1);

        let _ = server_done_rx.await;
        drop(write_half);
    });

    client.connect().await.unwrap();

    while rx.try_recv().is_ok() {}

    wait_for_mock_state(&state, "response gate waiter count 2", |state| {
        response_gate_waiter_count(state) == 2
    })
    .await;
    assert!(client.is_reconciling());
    tokio::time::sleep(Duration::from_millis(300)).await;
    let mut saw_stale_mass_status = false;
    let mut saw_stale_account_state = false;

    while let Ok(event) = rx.try_recv() {
        saw_stale_mass_status |= matches!(
            event,
            ExecutionEvent::Report(ExecutionReport::MassStatus(_))
        );
        saw_stale_account_state |= matches!(event, ExecutionEvent::Account(_));
    }
    assert!(
        !saw_stale_mass_status,
        "the stale generation must not publish mass status",
    );
    assert!(
        !saw_stale_account_state,
        "the stale generation must not publish account state",
    );
    release_second_tx.send(()).unwrap();

    let mass_status = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if let Some(ExecutionEvent::Report(ExecutionReport::MassStatus(status))) =
                rx.recv().await
            {
                break status;
            }
        }
    })
    .await
    .expect("current generation did not publish mass status");
    assert!(mass_status.order_reports().is_empty());
    assert_eq!(response_gate.waiters.load(Ordering::Relaxed), 2);

    wait_for_reconciliation_state(&client, false).await;
    assert!(!client.is_reconciling());

    client.disconnect().await.unwrap();
    let _ = server_done_tx.send(());
    server.await.unwrap();
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
        .insert(METHOD_LIST_CURRENT_ORDERS.to_string(), v["result"].clone());
    state.betting_response_delays.lock().insert(
        METHOD_LIST_CURRENT_ORDERS.to_string(),
        Duration::from_millis(800),
    );

    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, cache) = create_test_execution_client(addr, stream_port);

    let (server_done_tx, server_done_rx) = tokio::sync::oneshot::channel();

    let server = tokio::spawn(async move {
        let (mut reader, mut write_half) = accept_and_auth(&listener).await;
        let mut line = String::new();
        tokio::io::AsyncBufReadExt::read_line(&mut reader, &mut line)
            .await
            .unwrap();
        tokio::io::AsyncWriteExt::write_all(&mut write_half, RECONNECT_CONNECTION_MSG)
            .await
            .unwrap();
        let _ = server_done_rx.await;
        drop(write_half);
    });

    client.connect().await.unwrap();

    wait_for_reconciliation_state(&client, true).await;
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
                    denied.reason.contains("STREAM_RECONCILING"),
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
    let _ = server_done_tx.send(());
    server.await.unwrap();
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
        .insert(METHOD_LIST_CURRENT_ORDERS.to_string(), v["result"].clone());
    // Slow enough that the disconnect aborts an in-flight reconciliation.
    state.betting_response_delays.lock().insert(
        METHOD_LIST_CURRENT_ORDERS.to_string(),
        Duration::from_secs(5),
    );

    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, _rx, _data_rx, _cache) = create_test_execution_client(addr, stream_port);

    let (server_done_tx, server_done_rx) = tokio::sync::oneshot::channel();

    let server = tokio::spawn(async move {
        let (mut reader, mut write_half) = accept_and_auth(&listener).await;
        let mut line = String::new();
        tokio::io::AsyncBufReadExt::read_line(&mut reader, &mut line)
            .await
            .unwrap();
        tokio::io::AsyncWriteExt::write_all(&mut write_half, RECONNECT_CONNECTION_MSG)
            .await
            .unwrap();
        let _ = server_done_rx.await;
        drop(write_half);
    });

    client.connect().await.unwrap();

    wait_for_reconciliation_state(&client, true).await;
    assert!(client.is_reconciling());

    // Disconnecting mid-reconcile aborts the reconnect task before it can clear
    // the flag itself; the cleanup in clear_resync_state must do it.
    client.disconnect().await.unwrap();

    assert!(
        !client.is_reconciling(),
        "is_reconciling must be cleared on disconnect even when reconnect task is aborted",
    );

    let _ = server_done_tx.send(());
    server.await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_stop_during_reconciliation_cancels_recovery() {
    let (addr, state) = start_mock_http().await;
    let fixture = load_fixture("rest/list_current_orders_empty.json");
    let response: Value = serde_json::from_str(&fixture).unwrap();
    state.betting_overrides.lock().insert(
        METHOD_LIST_CURRENT_ORDERS.to_string(),
        response["result"].clone(),
    );
    let response_gate = MockResponseGate {
        method: METHOD_LIST_CURRENT_ORDERS.to_string(),
        waiters: Arc::new(AtomicUsize::new(0)),
        semaphore: Arc::new(tokio::sync::Semaphore::new(0)),
    };
    *state.betting_response_gate.lock() = Some(response_gate.clone());

    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, _cache) = create_test_execution_client(addr, stream_port);
    let (server_done_tx, server_done_rx) = tokio::sync::oneshot::channel();

    let server = tokio::spawn(async move {
        let (mut reader, mut write_half) = accept_and_auth(&listener).await;
        let mut line = String::new();
        tokio::io::AsyncBufReadExt::read_line(&mut reader, &mut line)
            .await
            .unwrap();
        tokio::io::AsyncWriteExt::write_all(&mut write_half, RECONNECT_CONNECTION_MSG)
            .await
            .unwrap();
        let _ = server_done_rx.await;
    });

    client.connect().await.unwrap();
    wait_for_mock_state(&state, "response gate waiter count 1", |state| {
        response_gate_waiter_count(state) == 1
    })
    .await;
    assert!(client.is_reconciling());

    while rx.try_recv().is_ok() {}

    client.stop().unwrap();
    assert!(!client.is_reconciling());
    assert!(!client.is_connected());

    response_gate.semaphore.add_permits(1);
    tokio::time::sleep(Duration::from_millis(200)).await;
    let published_mass_status = std::iter::from_fn(|| rx.try_recv().ok()).any(|event| {
        matches!(
            event,
            ExecutionEvent::Report(ExecutionReport::MassStatus(_))
        )
    });
    assert!(!published_mass_status);
    assert_eq!(betting_method_count(&state, METHOD_LIST_CURRENT_ORDERS), 1);

    let _ = server_done_tx.send(());
    server.await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_cancel_allowed_during_reconciliation() {
    let (addr, state) = start_mock_http().await;

    let list_fixture = load_fixture("rest/list_current_orders_empty.json");
    let list_v: Value = serde_json::from_str(&list_fixture).unwrap();
    state.betting_overrides.lock().insert(
        METHOD_LIST_CURRENT_ORDERS.to_string(),
        list_v["result"].clone(),
    );
    let cancel_fixture = load_fixture("rest/betting_cancel_orders_success.json");
    let cancel_v: Value = serde_json::from_str(&cancel_fixture).unwrap();
    state
        .betting_overrides
        .lock()
        .insert(METHOD_CANCEL_ORDERS.to_string(), cancel_v["result"].clone());
    state.betting_response_delays.lock().insert(
        METHOD_LIST_CURRENT_ORDERS.to_string(),
        Duration::from_millis(800),
    );

    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, _cache) = create_test_execution_client(addr, stream_port);

    let (server_done_tx, server_done_rx) = tokio::sync::oneshot::channel();

    let server = tokio::spawn(async move {
        let (mut reader, mut write_half) = accept_and_auth(&listener).await;
        let mut line = String::new();
        tokio::io::AsyncBufReadExt::read_line(&mut reader, &mut line)
            .await
            .unwrap();
        tokio::io::AsyncWriteExt::write_all(&mut write_half, RECONNECT_CONNECTION_MSG)
            .await
            .unwrap();
        let _ = server_done_rx.await;
        drop(write_half);
    });

    client.connect().await.unwrap();

    wait_for_reconciliation_state(&client, true).await;
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
                !rejected.reason.contains("STREAM_RECONCILING"),
                "Cancel must not be denied with STREAM_RECONCILING during reconciliation",
            );
        }
    }

    client.disconnect().await.unwrap();
    let _ = server_done_tx.send(());
    server.await.unwrap();
}
