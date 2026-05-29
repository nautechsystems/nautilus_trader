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

//! Integration tests for the Kraken execution client.

use std::{
    cell::RefCell,
    collections::HashMap,
    net::SocketAddr,
    path::PathBuf,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use axum::{
    Router,
    body::Body,
    extract::{
        Request, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::StatusCode,
    response::Response,
    routing::{any, get},
};
use nautilus_common::{
    cache::Cache,
    clients::ExecutionClient,
    live::runner::set_exec_event_sender,
    messages::{
        ExecutionEvent,
        execution::{BatchCancelOrders, CancelAllOrders, CancelOrder},
    },
    testing::wait_until_async,
};
use nautilus_core::{UUID4, UnixNanos};
use nautilus_kraken::{
    common::{
        consts::{KRAKEN_CLIENT_ID, KRAKEN_VENUE},
        enums::{KrakenEnvironment, KrakenProductType},
    },
    config::KrakenExecClientConfig,
    execution::{KrakenFuturesExecutionClient, KrakenSpotExecutionClient},
};
use nautilus_live::ExecutionClientCore;
use nautilus_model::{
    accounts::{AccountAny, CashAccount, MarginAccount},
    enums::{AccountType, OmsType, OrderSide, TimeInForce},
    events::{AccountState, OrderEventAny},
    identifiers::{AccountId, ClientOrderId, InstrumentId, StrategyId, TraderId, VenueOrderId},
    orders::{LimitOrder, OrderAny},
    types::{AccountBalance, Money, Price, Quantity},
};
use nautilus_network::http::HttpClient;
use rstest::rstest;
use serde_json::{Value, json};

#[derive(Debug, Clone, Copy, Default)]
enum SingleCancelResponse {
    #[default]
    Success,
    AmbiguousFailure,
    NonOrderApiError,
    StructuredReject,
}

#[derive(Debug, Clone, Copy, Default)]
enum BatchCancelResponse {
    #[default]
    Success,
    WholeFailure,
    Mixed,
}

#[derive(Debug, Clone, Copy, Default)]
struct CommandResponses {
    single_cancel: SingleCancelResponse,
    batch_cancel: BatchCancelResponse,
    cancel_all: BatchCancelResponse,
}

#[derive(Clone, Default)]
struct TestServerState {
    command_responses: Arc<tokio::sync::Mutex<CommandResponses>>,
    cancel_request_count: Arc<AtomicUsize>,
    batch_cancel_request_count: Arc<AtomicUsize>,
    cancel_all_request_count: Arc<AtomicUsize>,
}

fn data_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test_data")
}

fn load_test_data(filename: &str) -> String {
    std::fs::read_to_string(data_path().join(filename))
        .unwrap_or_else(|e| panic!("failed to read {filename}: {e}"))
}

async fn handle_ws_upgrade(
    ws: WebSocketUpgrade,
    State(_state): State<TestServerState>,
) -> Response {
    ws.on_upgrade(handle_socket)
}

async fn handle_socket(mut socket: WebSocket) {
    while let Some(message) = socket.recv().await {
        let Ok(message) = message else { break };

        match message {
            Message::Text(text) => {
                let Ok(payload) = serde_json::from_str::<Value>(&text) else {
                    continue;
                };

                if payload.get("event").and_then(|v| v.as_str()) == Some("challenge") {
                    let response = json!({
                        "event": "challenge",
                        "message": "server-challenge",
                    });

                    if socket
                        .send(Message::Text(response.to_string().into()))
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
            }
            Message::Ping(data) => {
                let sent = socket.send(Message::Pong(data)).await;
                if sent.is_err() {
                    break;
                }
            }
            Message::Close(_) => break,
            _ => {}
        }
    }
}

async fn handle_http_request(State(state): State<TestServerState>, req: Request) -> Response {
    match req.uri().path() {
        "/health" => Response::builder()
            .status(StatusCode::OK)
            .body(Body::from("OK"))
            .unwrap(),
        "/derivatives/api/v3/instruments" => {
            json_response(load_test_data("http_futures_instruments.json"))
        }
        "/derivatives/api/v3/accounts" => {
            json_response(r#"{"result":"success","accounts":{}}"#.to_string())
        }
        "/derivatives/api/v3/cancelorder" => {
            state.cancel_request_count.fetch_add(1, Ordering::Relaxed);
            match state.command_responses.lock().await.single_cancel {
                SingleCancelResponse::Success => json_response(
                    r#"{"result":"success","cancelStatus":{"status":"cancelled","order_id":"V-SINGLE"}}"#
                        .to_string(),
                ),
                SingleCancelResponse::AmbiguousFailure => Response::builder()
                    .status(StatusCode::INTERNAL_SERVER_ERROR)
                    .body(Body::from("cancel failed"))
                    .unwrap(),
                SingleCancelResponse::NonOrderApiError => Response::builder()
                    .status(StatusCode::TOO_MANY_REQUESTS)
                    .body(Body::from("rate limit exceeded"))
                    .unwrap(),
                SingleCancelResponse::StructuredReject => json_response(
                    r#"{"result":"error","cancelStatus":{"status":"notFound","order_id":"V-SINGLE"}}"#
                        .to_string(),
                ),
            }
        }
        "/derivatives/api/v3/batchorder" => {
            state
                .batch_cancel_request_count
                .fetch_add(1, Ordering::Relaxed);

            match state.command_responses.lock().await.batch_cancel {
                BatchCancelResponse::Success => json_response(
                    r#"{"result":"success","batchStatus":[{"orderId":"V-BATCH-1","status":"cancelled"},{"orderId":"V-BATCH-2","status":"cancelled"}]}"#
                        .to_string(),
                ),
                BatchCancelResponse::WholeFailure => json_response(
                    r#"{"result":"error","error":"batch failed","batchStatus":[]}"#.to_string(),
                ),
                BatchCancelResponse::Mixed => json_response(
                    r#"{"result":"success","batchStatus":[{"orderId":"V-BATCH-OK","status":"cancelled"},{"orderId":"V-BATCH-REJECT","status":"notFound"}]}"#
                        .to_string(),
                ),
            }
        }
        "/derivatives/api/v3/cancelallorders" => {
            state
                .cancel_all_request_count
                .fetch_add(1, Ordering::Relaxed);

            match state.command_responses.lock().await.cancel_all {
                BatchCancelResponse::Success | BatchCancelResponse::Mixed => json_response(
                    r#"{"result":"success","cancelStatus":{"status":"cancelled","cancelledOrders":[]}}"#
                        .to_string(),
                ),
                BatchCancelResponse::WholeFailure => json_response(
                    r#"{"result":"error","cancelStatus":{"status":"noOrdersToCancel","cancelledOrders":[]}}"#
                        .to_string(),
                ),
            }
        }
        "/0/public/AssetPairs" => json_response(load_test_data("http_asset_pairs.json")),
        "/0/private/GetWebSocketsToken" => json_response(
            r#"{"error":[],"result":{"token":"TEST-TOKEN","expires":900}}"#.to_string(),
        ),
        "/0/private/Balance" => json_response(load_test_data("http_spot_balance.json")),
        "/0/private/CancelOrder" => {
            state.cancel_request_count.fetch_add(1, Ordering::Relaxed);
            match state.command_responses.lock().await.single_cancel {
                SingleCancelResponse::Success => {
                    json_response(r#"{"error":[],"result":{"count":1}}"#.to_string())
                }
                SingleCancelResponse::AmbiguousFailure => Response::builder()
                    .status(StatusCode::INTERNAL_SERVER_ERROR)
                    .body(Body::from("cancel failed"))
                    .unwrap(),
                SingleCancelResponse::NonOrderApiError => {
                    json_response(r#"{"error":["EAPI:Rate limit exceeded"]}"#.to_string())
                }
                SingleCancelResponse::StructuredReject => {
                    json_response(r#"{"error":["EOrder:Unknown order"]}"#.to_string())
                }
            }
        }
        "/0/private/CancelOrderBatch" => {
            state
                .batch_cancel_request_count
                .fetch_add(1, Ordering::Relaxed);

            match state.command_responses.lock().await.batch_cancel {
                BatchCancelResponse::Success => {
                    json_response(r#"{"error":[],"result":{"count":2}}"#.to_string())
                }
                BatchCancelResponse::WholeFailure => {
                    json_response(r#"{"error":["EOrder:Batch failed"]}"#.to_string())
                }
                BatchCancelResponse::Mixed => {
                    json_response(r#"{"error":[],"result":{"count":1}}"#.to_string())
                }
            }
        }
        "/0/private/CancelAll" => {
            state
                .cancel_all_request_count
                .fetch_add(1, Ordering::Relaxed);

            match state.command_responses.lock().await.cancel_all {
                BatchCancelResponse::Success | BatchCancelResponse::Mixed => {
                    json_response(r#"{"error":[],"result":{"count":1}}"#.to_string())
                }
                BatchCancelResponse::WholeFailure => {
                    json_response(r#"{"error":["EOrder:Cancel all failed"]}"#.to_string())
                }
            }
        }
        _ => Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Body::from("not found"))
            .unwrap(),
    }
}

fn json_response(body: String) -> Response {
    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "application/json")
        .body(Body::from(body))
        .unwrap()
}

fn create_test_router(state: TestServerState) -> Router {
    Router::new()
        .route("/ws", get(handle_ws_upgrade))
        .fallback(any(handle_http_request))
        .with_state(state)
}

async fn start_test_server()
-> Result<(SocketAddr, TestServerState), Box<dyn std::error::Error + Send + Sync>> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let state = TestServerState::default();
    let router = create_test_router(state.clone());

    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });

    wait_for_server(addr).await;

    Ok((addr, state))
}

async fn wait_for_server(addr: SocketAddr) {
    let health_url = format!("http://{addr}/health");
    let http_client =
        HttpClient::new(HashMap::new(), Vec::new(), Vec::new(), None, None, None).unwrap();
    wait_until_async(
        || {
            let url = health_url.clone();
            let client = http_client.clone();
            async move { client.get(url, None, None, Some(1), None).await.is_ok() }
        },
        Duration::from_secs(5),
    )
    .await;
}

fn create_test_exec_config(addr: SocketAddr) -> KrakenExecClientConfig {
    KrakenExecClientConfig {
        trader_id: test_trader_id(),
        account_id: test_account_id(),
        api_key: "test_key".to_string(),
        api_secret: "c2VjcmV0".to_string(),
        product_type: KrakenProductType::Futures,
        environment: KrakenEnvironment::Live,
        base_url: Some(format!("http://{addr}")),
        ws_url: Some(format!("ws://{addr}/ws")),
        timeout_secs: 2,
        ..Default::default()
    }
}

fn create_test_spot_exec_config(addr: SocketAddr) -> KrakenExecClientConfig {
    KrakenExecClientConfig {
        trader_id: test_trader_id(),
        account_id: test_account_id(),
        api_key: "test_key".to_string(),
        api_secret: "c2VjcmV0".to_string(),
        product_type: KrakenProductType::Spot,
        environment: KrakenEnvironment::Live,
        base_url: Some(format!("http://{addr}")),
        ws_url: Some(format!("ws://{addr}/ws")),
        timeout_secs: 2,
        spot_account_type: AccountType::Cash,
        use_ws_trade: false,
        ..Default::default()
    }
}

fn create_test_execution_client(
    addr: SocketAddr,
) -> (
    KrakenFuturesExecutionClient,
    tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
    Rc<RefCell<Cache>>,
) {
    let cache = Rc::new(RefCell::new(Cache::default()));
    let core = ExecutionClientCore::new(
        test_trader_id(),
        *KRAKEN_CLIENT_ID,
        *KRAKEN_VENUE,
        OmsType::Netting,
        test_account_id(),
        AccountType::Margin,
        None,
        cache.clone(),
    );
    let config = create_test_exec_config(addr);

    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    set_exec_event_sender(tx);

    let mut client = KrakenFuturesExecutionClient::new(core, config).unwrap();
    client.start().unwrap();

    (client, rx, cache)
}

fn create_test_spot_execution_client(
    addr: SocketAddr,
) -> (
    KrakenSpotExecutionClient,
    tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
    Rc<RefCell<Cache>>,
) {
    let cache = Rc::new(RefCell::new(Cache::default()));
    let core = ExecutionClientCore::new(
        test_trader_id(),
        *KRAKEN_CLIENT_ID,
        *KRAKEN_VENUE,
        OmsType::Netting,
        test_account_id(),
        AccountType::Cash,
        None,
        cache.clone(),
    );
    let config = create_test_spot_exec_config(addr);

    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    set_exec_event_sender(tx);

    let mut client = KrakenSpotExecutionClient::new(core, config).unwrap();
    client.start().unwrap();

    (client, rx, cache)
}

async fn connected_client_with_command_responses(
    responses: CommandResponses,
) -> (
    KrakenFuturesExecutionClient,
    tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
    Rc<RefCell<Cache>>,
    TestServerState,
) {
    let (addr, state) = start_test_server().await.unwrap();
    *state.command_responses.lock().await = responses;

    let (mut client, rx, cache) = create_test_execution_client(addr);
    add_test_account_to_cache(&cache);
    client.connect().await.unwrap();

    (client, rx, cache, state)
}

async fn connected_spot_client_with_command_responses(
    responses: CommandResponses,
) -> (
    KrakenSpotExecutionClient,
    tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
    Rc<RefCell<Cache>>,
    TestServerState,
) {
    let (addr, state) = start_test_server().await.unwrap();
    *state.command_responses.lock().await = responses;

    let (mut client, rx, cache) = create_test_spot_execution_client(addr);
    add_test_spot_account_to_cache(&cache);
    client.connect().await.unwrap();

    (client, rx, cache, state)
}

fn add_test_account_to_cache(cache: &Rc<RefCell<Cache>>) {
    let account_state = AccountState::new(
        test_account_id(),
        AccountType::Margin,
        vec![AccountBalance::new(
            Money::from("1.0 BTC"),
            Money::from("0 BTC"),
            Money::from("1.0 BTC"),
        )],
        vec![],
        true,
        UUID4::new(),
        UnixNanos::default(),
        UnixNanos::default(),
        None,
    );

    let account = AccountAny::Margin(MarginAccount::new(account_state, true));
    cache.borrow_mut().add_account(account).unwrap();
}

fn add_test_spot_account_to_cache(cache: &Rc<RefCell<Cache>>) {
    let account_state = AccountState::new(
        test_account_id(),
        AccountType::Cash,
        vec![
            AccountBalance::new(
                Money::from("10000 USDT"),
                Money::from("0 USDT"),
                Money::from("10000 USDT"),
            ),
            AccountBalance::new(
                Money::from("1 BTC"),
                Money::from("0 BTC"),
                Money::from("1 BTC"),
            ),
        ],
        vec![],
        true,
        UUID4::new(),
        UnixNanos::default(),
        UnixNanos::default(),
        None,
    );

    let account = AccountAny::Cash(CashAccount::new(account_state, true, false));
    cache.borrow_mut().add_account(account).unwrap();
}

fn add_limit_order_to_cache(
    cache: &Rc<RefCell<Cache>>,
    client_order_id: ClientOrderId,
) -> OrderAny {
    let order = LimitOrder::new(
        test_trader_id(),
        test_strategy_id(),
        test_instrument_id(),
        client_order_id,
        OrderSide::Buy,
        Quantity::from("1"),
        Price::from("50000"),
        TimeInForce::Gtc,
        None,
        true,
        false,
        false,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        UUID4::new(),
        UnixNanos::default(),
    );

    let order_any = OrderAny::Limit(order);
    cache
        .borrow_mut()
        .add_order(order_any.clone(), None, None, false)
        .unwrap();
    order_any
}

fn add_spot_limit_order_to_cache(
    cache: &Rc<RefCell<Cache>>,
    client_order_id: ClientOrderId,
) -> OrderAny {
    let order = LimitOrder::new(
        test_trader_id(),
        test_strategy_id(),
        test_spot_instrument_id(),
        client_order_id,
        OrderSide::Buy,
        Quantity::from("0.1"),
        Price::from("50000"),
        TimeInForce::Gtc,
        None,
        true,
        false,
        false,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        UUID4::new(),
        UnixNanos::default(),
    );

    let order_any = OrderAny::Limit(order);
    cache
        .borrow_mut()
        .add_order(order_any.clone(), None, None, false)
        .unwrap();
    order_any
}

fn test_trader_id() -> TraderId {
    TraderId::from("TESTER-001")
}

fn test_strategy_id() -> StrategyId {
    StrategyId::from("S-001")
}

fn test_account_id() -> AccountId {
    AccountId::from("KRAKEN-001")
}

fn test_instrument_id() -> InstrumentId {
    InstrumentId::from("PI_XBTUSD.KRAKEN")
}

fn test_spot_instrument_id() -> InstrumentId {
    InstrumentId::from("BTC/USDT.KRAKEN")
}

fn cancel_order_command(
    client_order_id: ClientOrderId,
    venue_order_id: VenueOrderId,
) -> CancelOrder {
    CancelOrder::new(
        test_trader_id(),
        Some(*KRAKEN_CLIENT_ID),
        test_strategy_id(),
        test_instrument_id(),
        client_order_id,
        Some(venue_order_id),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    )
}

fn spot_cancel_order_command(
    client_order_id: ClientOrderId,
    venue_order_id: VenueOrderId,
) -> CancelOrder {
    CancelOrder::new(
        test_trader_id(),
        Some(*KRAKEN_CLIENT_ID),
        test_strategy_id(),
        test_spot_instrument_id(),
        client_order_id,
        Some(venue_order_id),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    )
}

fn batch_cancel_command(cancels: Vec<CancelOrder>) -> BatchCancelOrders {
    BatchCancelOrders::new(
        test_trader_id(),
        Some(*KRAKEN_CLIENT_ID),
        test_strategy_id(),
        test_instrument_id(),
        cancels,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    )
}

fn spot_batch_cancel_command(cancels: Vec<CancelOrder>) -> BatchCancelOrders {
    BatchCancelOrders::new(
        test_trader_id(),
        Some(*KRAKEN_CLIENT_ID),
        test_strategy_id(),
        test_spot_instrument_id(),
        cancels,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    )
}

fn cancel_all_orders_command() -> CancelAllOrders {
    CancelAllOrders::new(
        test_trader_id(),
        Some(*KRAKEN_CLIENT_ID),
        test_strategy_id(),
        test_instrument_id(),
        OrderSide::NoOrderSide,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    )
}

fn spot_cancel_all_orders_command() -> CancelAllOrders {
    CancelAllOrders::new(
        test_trader_id(),
        Some(*KRAKEN_CLIENT_ID),
        test_strategy_id(),
        test_spot_instrument_id(),
        OrderSide::NoOrderSide,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    )
}

async fn wait_for_count(count: &AtomicUsize, expected: usize) {
    wait_until_async(
        || async { count.load(Ordering::Relaxed) >= expected },
        Duration::from_secs(5),
    )
    .await;
}

async fn assert_no_order_event_matching<F>(
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
    predicate: F,
) where
    F: Fn(&OrderEventAny) -> bool,
{
    let unexpected = tokio::time::timeout(Duration::from_millis(500), async {
        loop {
            let event = rx.recv().await.expect("Execution event channel closed");
            if let ExecutionEvent::Order(order_event) = &event
                && predicate(order_event)
            {
                return event;
            }
        }
    })
    .await;

    if let Ok(event) = unexpected {
        panic!("Unexpected order event: {event:?}");
    }
}

async fn recv_until<F>(
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
    predicate: F,
) -> ExecutionEvent
where
    F: Fn(&ExecutionEvent) -> bool,
{
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let event = rx.recv().await.expect("Execution event channel closed");
            if predicate(&event) {
                return event;
            }
        }
    })
    .await
    .expect("Timed out waiting for execution event")
}

#[rstest]
#[tokio::test]
async fn test_spot_local_cancel_validation_failure_does_not_emit_cancel_rejected() {
    let (client, mut rx, cache, state) =
        connected_spot_client_with_command_responses(CommandResponses::default()).await;

    let client_order_id = ClientOrderId::new("spot-local-cancel-invalid-test-001");
    add_spot_limit_order_to_cache(&cache, client_order_id);

    let command = CancelOrder::new(
        test_trader_id(),
        Some(*KRAKEN_CLIENT_ID),
        test_strategy_id(),
        InstrumentId::from("UNKNOWN.KRAKEN"),
        client_order_id,
        Some(VenueOrderId::from("SPOT-SINGLE")),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );

    client.cancel_order(command).unwrap();

    assert_no_order_event_matching(&mut rx, |event| {
        matches!(
            event,
            OrderEventAny::CancelRejected(event) if event.client_order_id == client_order_id
        )
    })
    .await;

    assert_eq!(state.cancel_request_count.load(Ordering::Relaxed), 0);
}

#[rstest]
#[tokio::test]
async fn test_spot_ambiguous_single_cancel_failure_does_not_emit_cancel_rejected() {
    let (client, mut rx, cache, state) =
        connected_spot_client_with_command_responses(CommandResponses {
            single_cancel: SingleCancelResponse::AmbiguousFailure,
            ..Default::default()
        })
        .await;

    let client_order_id = ClientOrderId::new("spot-ambiguous-cancel-test-001");
    add_spot_limit_order_to_cache(&cache, client_order_id);

    client
        .cancel_order(spot_cancel_order_command(
            client_order_id,
            VenueOrderId::from("SPOT-SINGLE"),
        ))
        .unwrap();

    wait_for_count(&state.cancel_request_count, 1).await;

    assert_no_order_event_matching(&mut rx, |event| {
        matches!(
            event,
            OrderEventAny::CancelRejected(event) if event.client_order_id == client_order_id
        )
    })
    .await;
}

#[rstest]
#[tokio::test]
async fn test_spot_non_order_api_cancel_failure_does_not_emit_cancel_rejected() {
    let (client, mut rx, cache, state) =
        connected_spot_client_with_command_responses(CommandResponses {
            single_cancel: SingleCancelResponse::NonOrderApiError,
            ..Default::default()
        })
        .await;

    let client_order_id = ClientOrderId::new("spot-rate-limit-cancel-test-001");
    add_spot_limit_order_to_cache(&cache, client_order_id);

    client
        .cancel_order(spot_cancel_order_command(
            client_order_id,
            VenueOrderId::from("SPOT-SINGLE"),
        ))
        .unwrap();

    wait_for_count(&state.cancel_request_count, 1).await;

    assert_no_order_event_matching(&mut rx, |event| {
        matches!(
            event,
            OrderEventAny::CancelRejected(event) if event.client_order_id == client_order_id
        )
    })
    .await;
}

#[rstest]
#[tokio::test]
async fn test_spot_explicit_single_cancel_api_error_emits_cancel_rejected() {
    let (client, mut rx, cache, _state) =
        connected_spot_client_with_command_responses(CommandResponses {
            single_cancel: SingleCancelResponse::StructuredReject,
            ..Default::default()
        })
        .await;

    let client_order_id = ClientOrderId::new("spot-venue-cancel-reject-test-001");
    add_spot_limit_order_to_cache(&cache, client_order_id);

    client
        .cancel_order(spot_cancel_order_command(
            client_order_id,
            VenueOrderId::from("SPOT-SINGLE"),
        ))
        .unwrap();

    match recv_until(&mut rx, |event| {
        matches!(
            event,
            ExecutionEvent::Order(OrderEventAny::CancelRejected(event))
                if event.client_order_id == client_order_id
        )
    })
    .await
    {
        ExecutionEvent::Order(OrderEventAny::CancelRejected(event)) => {
            assert_eq!(event.client_order_id, client_order_id);
            assert!(event.reason.as_str().contains("Unknown order"));
        }
        other => panic!("Expected CancelRejected event, was {other:?}"),
    }
}

#[rstest]
#[tokio::test]
async fn test_spot_whole_batch_cancel_failure_does_not_emit_one_reject_per_order() {
    let (client, mut rx, cache, state) =
        connected_spot_client_with_command_responses(CommandResponses {
            batch_cancel: BatchCancelResponse::WholeFailure,
            ..Default::default()
        })
        .await;

    let first_client_order_id = ClientOrderId::new("spot-batch-cancel-whole-fail-001");
    let second_client_order_id = ClientOrderId::new("spot-batch-cancel-whole-fail-002");
    add_spot_limit_order_to_cache(&cache, first_client_order_id);
    add_spot_limit_order_to_cache(&cache, second_client_order_id);

    client
        .batch_cancel_orders(spot_batch_cancel_command(vec![
            spot_cancel_order_command(first_client_order_id, VenueOrderId::from("SPOT-BATCH-1")),
            spot_cancel_order_command(second_client_order_id, VenueOrderId::from("SPOT-BATCH-2")),
        ]))
        .unwrap();

    wait_for_count(&state.batch_cancel_request_count, 1).await;

    assert_no_order_event_matching(&mut rx, |event| {
        matches!(event, OrderEventAny::CancelRejected(_))
    })
    .await;
}

#[rstest]
#[tokio::test]
async fn test_spot_whole_cancel_all_failure_does_not_emit_cancel_rejected() {
    let (client, mut rx, _cache, state) =
        connected_spot_client_with_command_responses(CommandResponses {
            cancel_all: BatchCancelResponse::WholeFailure,
            ..Default::default()
        })
        .await;

    client
        .cancel_all_orders(spot_cancel_all_orders_command())
        .unwrap();

    wait_for_count(&state.cancel_all_request_count, 1).await;

    assert_no_order_event_matching(&mut rx, |event| {
        matches!(event, OrderEventAny::CancelRejected(_))
    })
    .await;
}

#[rstest]
#[tokio::test]
async fn test_ambiguous_single_cancel_failure_does_not_emit_cancel_rejected() {
    let (client, mut rx, cache, state) =
        connected_client_with_command_responses(CommandResponses {
            single_cancel: SingleCancelResponse::AmbiguousFailure,
            ..Default::default()
        })
        .await;

    let client_order_id = ClientOrderId::new("ambiguous-cancel-test-001");
    add_limit_order_to_cache(&cache, client_order_id);

    client
        .cancel_order(cancel_order_command(
            client_order_id,
            VenueOrderId::from("V-SINGLE"),
        ))
        .unwrap();

    wait_for_count(&state.cancel_request_count, 1).await;

    assert_no_order_event_matching(&mut rx, |event| {
        matches!(
            event,
            OrderEventAny::CancelRejected(event) if event.client_order_id == client_order_id
        )
    })
    .await;
}

#[rstest]
#[tokio::test]
async fn test_explicit_structured_single_cancel_rejection_emits_cancel_rejected() {
    let (client, mut rx, cache, _state) =
        connected_client_with_command_responses(CommandResponses {
            single_cancel: SingleCancelResponse::StructuredReject,
            ..Default::default()
        })
        .await;

    let client_order_id = ClientOrderId::new("venue-cancel-reject-test-001");
    add_limit_order_to_cache(&cache, client_order_id);

    client
        .cancel_order(cancel_order_command(
            client_order_id,
            VenueOrderId::from("V-SINGLE"),
        ))
        .unwrap();

    match recv_until(&mut rx, |event| {
        matches!(
            event,
            ExecutionEvent::Order(OrderEventAny::CancelRejected(event))
                if event.client_order_id == client_order_id
        )
    })
    .await
    {
        ExecutionEvent::Order(OrderEventAny::CancelRejected(event)) => {
            assert_eq!(event.client_order_id, client_order_id);
            assert!(event.reason.as_str().contains("notFound"));
        }
        other => panic!("Expected CancelRejected event, was {other:?}"),
    }
}

#[rstest]
#[tokio::test]
async fn test_local_cancel_validation_failure_does_not_emit_cancel_rejected() {
    let (client, mut rx, cache, state) =
        connected_client_with_command_responses(CommandResponses::default()).await;

    let client_order_id = ClientOrderId::new("local-cancel-invalid-test-001");
    add_limit_order_to_cache(&cache, client_order_id);

    let command = CancelOrder::new(
        test_trader_id(),
        Some(*KRAKEN_CLIENT_ID),
        test_strategy_id(),
        InstrumentId::from("UNKNOWN.KRAKEN"),
        client_order_id,
        Some(VenueOrderId::from("V-SINGLE")),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );

    client.cancel_order(command).unwrap();

    assert_no_order_event_matching(&mut rx, |event| {
        matches!(
            event,
            OrderEventAny::CancelRejected(event) if event.client_order_id == client_order_id
        )
    })
    .await;

    assert_eq!(state.cancel_request_count.load(Ordering::Relaxed), 0);
}

#[rstest]
#[tokio::test]
async fn test_batch_cancel_local_validation_failure_does_not_emit_cancel_rejected() {
    let (client, mut rx, cache, state) =
        connected_client_with_command_responses(CommandResponses::default()).await;

    let client_order_id = ClientOrderId::new("batch-local-cancel-invalid-test-001");
    add_limit_order_to_cache(&cache, client_order_id);

    let command = CancelOrder::new(
        test_trader_id(),
        Some(*KRAKEN_CLIENT_ID),
        test_strategy_id(),
        InstrumentId::from("UNKNOWN.KRAKEN"),
        client_order_id,
        Some(VenueOrderId::from("V-BATCH-LOCAL")),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );

    client
        .batch_cancel_orders(BatchCancelOrders::new(
            test_trader_id(),
            Some(*KRAKEN_CLIENT_ID),
            test_strategy_id(),
            test_instrument_id(),
            vec![command],
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ))
        .unwrap();

    assert_no_order_event_matching(&mut rx, |event| {
        matches!(
            event,
            OrderEventAny::CancelRejected(event) if event.client_order_id == client_order_id
        )
    })
    .await;

    assert_eq!(state.batch_cancel_request_count.load(Ordering::Relaxed), 0);
}

#[rstest]
#[tokio::test]
async fn test_whole_batch_cancel_failure_does_not_emit_one_reject_per_order() {
    let (client, mut rx, cache, state) =
        connected_client_with_command_responses(CommandResponses {
            batch_cancel: BatchCancelResponse::WholeFailure,
            ..Default::default()
        })
        .await;

    let first_client_order_id = ClientOrderId::new("batch-cancel-whole-fail-001");
    let second_client_order_id = ClientOrderId::new("batch-cancel-whole-fail-002");
    add_limit_order_to_cache(&cache, first_client_order_id);
    add_limit_order_to_cache(&cache, second_client_order_id);

    client
        .batch_cancel_orders(batch_cancel_command(vec![
            cancel_order_command(first_client_order_id, VenueOrderId::from("V-BATCH-1")),
            cancel_order_command(second_client_order_id, VenueOrderId::from("V-BATCH-2")),
        ]))
        .unwrap();

    wait_for_count(&state.batch_cancel_request_count, 1).await;

    assert_no_order_event_matching(&mut rx, |event| {
        matches!(event, OrderEventAny::CancelRejected(_))
    })
    .await;
}

#[rstest]
#[tokio::test]
async fn test_mixed_per_item_batch_cancel_result_rejects_only_failed_item() {
    let (client, mut rx, cache, state) =
        connected_client_with_command_responses(CommandResponses {
            batch_cancel: BatchCancelResponse::Mixed,
            ..Default::default()
        })
        .await;

    let ok_client_order_id = ClientOrderId::new("batch-cancel-ok-001");
    let reject_client_order_id = ClientOrderId::new("batch-cancel-reject-001");
    add_limit_order_to_cache(&cache, ok_client_order_id);
    add_limit_order_to_cache(&cache, reject_client_order_id);

    client
        .batch_cancel_orders(batch_cancel_command(vec![
            cancel_order_command(ok_client_order_id, VenueOrderId::from("V-BATCH-OK")),
            cancel_order_command(reject_client_order_id, VenueOrderId::from("V-BATCH-REJECT")),
        ]))
        .unwrap();

    wait_for_count(&state.batch_cancel_request_count, 1).await;

    match recv_until(&mut rx, |event| {
        matches!(
            event,
            ExecutionEvent::Order(OrderEventAny::CancelRejected(event))
                if event.client_order_id == reject_client_order_id
        )
    })
    .await
    {
        ExecutionEvent::Order(OrderEventAny::CancelRejected(event)) => {
            assert_eq!(event.client_order_id, reject_client_order_id);
            assert!(event.reason.as_str().contains("notFound"));
        }
        other => panic!("Expected CancelRejected event, was {other:?}"),
    }

    assert_no_order_event_matching(&mut rx, |event| {
        matches!(
            event,
            OrderEventAny::CancelRejected(event) if event.client_order_id == ok_client_order_id
        )
    })
    .await;
}

#[rstest]
#[tokio::test]
async fn test_whole_cancel_all_failure_does_not_emit_cancel_rejected() {
    let (client, mut rx, _cache, state) =
        connected_client_with_command_responses(CommandResponses {
            cancel_all: BatchCancelResponse::WholeFailure,
            ..Default::default()
        })
        .await;

    client
        .cancel_all_orders(cancel_all_orders_command())
        .unwrap();

    wait_for_count(&state.cancel_all_request_count, 1).await;

    assert_no_order_event_matching(&mut rx, |event| {
        matches!(event, OrderEventAny::CancelRejected(_))
    })
    .await;
}
