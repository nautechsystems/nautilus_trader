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

//! Integration tests for `DeribitExecutionClient`.
//!
//! These tests verify execution client operations including connection,
//! authentication, and event handling.

use std::{
    cell::RefCell,
    collections::HashMap,
    net::SocketAddr,
    path::PathBuf,
    rc::Rc,
    str::FromStr,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};

use axum::{
    Router,
    extract::{
        State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    response::{IntoResponse, Json, Response},
    routing::{get, post},
};
use nautilus_common::{
    cache::Cache,
    clients::ExecutionClient,
    live::runner::set_exec_event_sender,
    messages::{
        ExecutionEvent,
        execution::{BatchCancelOrders, CancelAllOrders, CancelOrder, ModifyOrder, SubmitOrder},
    },
    testing::wait_until_async,
};
use nautilus_core::{UUID4, UnixNanos};
use nautilus_deribit::{
    common::{
        consts::{DERIBIT_CLIENT_ID, DERIBIT_VENUE},
        enums::DeribitEnvironment,
    },
    config::DeribitExecClientConfig,
    execution::DeribitExecutionClient,
    http::models::DeribitProductType,
    websocket::enums::DeribitWsMethod,
};
use nautilus_live::ExecutionClientCore;
use nautilus_model::{
    accounts::{AccountAny, MarginAccount},
    enums::{AccountType, OmsType, OrderSide, TimeInForce},
    events::{AccountState, OrderEventAny},
    identifiers::{AccountId, ClientOrderId, InstrumentId, StrategyId, TraderId, VenueOrderId},
    orders::{LimitOrder, Order, OrderAny},
    types::{AccountBalance, Money, Price, Quantity},
};
use nautilus_network::http::HttpClient;
use rstest::rstest;
use serde_json::{Value, json};

fn data_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test_data")
}

fn load_json(filename: &str) -> Value {
    let content = std::fs::read_to_string(data_path().join(filename))
        .unwrap_or_else(|_| panic!("failed to read {filename}"));
    serde_json::from_str(&content).expect("invalid json")
}

#[derive(Debug, Clone, Copy, Default)]
enum CommandResponse {
    #[default]
    Success,
    AmbiguousFailure,
    MalformedSuccess,
    VenueReject {
        code: i64,
        message: &'static str,
    },
}

#[derive(Debug, Clone, Copy, Default)]
struct CommandResponses {
    submit: CommandResponse,
    cancel: CommandResponse,
    modify: CommandResponse,
    cancel_all: CommandResponse,
}

#[derive(Debug, Clone, Copy)]
enum CommandOrderState {
    Open,
    Cancelled,
}

impl CommandOrderState {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Cancelled => "cancelled",
        }
    }
}

#[derive(Clone, Default)]
struct TestServerState {
    connection_count: Arc<tokio::sync::Mutex<usize>>,
    subscriptions: Arc<tokio::sync::Mutex<Vec<String>>>,
    subscription_events: Arc<tokio::sync::Mutex<Vec<(String, bool)>>>,
    authenticated: Arc<AtomicBool>,
    auth_request_count: Arc<AtomicUsize>,
    disconnect_trigger: Arc<AtomicBool>,
    command_responses: Arc<tokio::sync::Mutex<CommandResponses>>,
    command_request_count: Arc<AtomicUsize>,
}

async fn handle_jsonrpc_request(
    State(_state): State<TestServerState>,
    Json(request): Json<Value>,
) -> impl IntoResponse {
    let method = request.get("method").and_then(|m| m.as_str()).unwrap_or("");
    let id = request.get("id").and_then(|i| i.as_u64()).unwrap_or(0);
    let params = request.get("params").cloned();

    match method {
        "public/get_instruments" => handle_get_instruments(id, params).await,
        "public/get_instrument" => {
            let mut data = load_json("http_get_instrument.json");
            data["id"] = json!(id);
            Json(data).into_response()
        }
        "private/get_account_summaries" => {
            let mut data = load_json("http_get_account_summaries.json");
            data["id"] = json!(id);
            Json(data).into_response()
        }
        _ => Json(json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": {
                "code": -32601,
                "message": "Method not found"
            },
            "testnet": true
        }))
        .into_response(),
    }
}

async fn handle_get_instruments(id: u64, params: Option<Value>) -> Response {
    let currency = params
        .as_ref()
        .and_then(|p| p.get("currency"))
        .and_then(|c| c.as_str());

    match currency {
        Some("any" | "BTC") | None => {
            let mut data = load_json("http_get_instruments.json");
            data["id"] = json!(id);

            if let Some(kind) = params
                .as_ref()
                .and_then(|p| p.get("kind"))
                .and_then(|k| k.as_str())
                && let Some(result) = data.get_mut("result")
                && let Some(instruments) = result.as_array_mut()
            {
                instruments.retain(|inst| {
                    inst.get("kind")
                        .and_then(|k| k.as_str())
                        .is_some_and(|k| k == kind)
                });
            }

            Json(data).into_response()
        }
        _ => Json(json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": [],
            "testnet": true
        }))
        .into_response(),
    }
}

async fn handle_ws_upgrade(ws: WebSocketUpgrade, State(state): State<TestServerState>) -> Response {
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

async fn handle_socket(mut socket: WebSocket, state: TestServerState) {
    {
        let mut count = state.connection_count.lock().await;
        *count += 1;
    }

    while let Some(message) = socket.recv().await {
        let Ok(message) = message else { break };

        if state.disconnect_trigger.load(Ordering::Relaxed) {
            let _ = socket.send(Message::Close(None)).await;
            break;
        }

        match message {
            Message::Text(text) => {
                let Ok(payload) = serde_json::from_str::<Value>(&text) else {
                    continue;
                };

                let method = payload
                    .get("method")
                    .and_then(|m| m.as_str())
                    .and_then(|method| DeribitWsMethod::from_str(method).ok());
                let id = payload.get("id").and_then(|i| i.as_u64());

                match method {
                    Some(DeribitWsMethod::PublicAuth) => {
                        state.auth_request_count.fetch_add(1, Ordering::Relaxed);
                        state.authenticated.store(true, Ordering::Relaxed);

                        let scope = payload
                            .get("params")
                            .and_then(|p| p.get("scope"))
                            .and_then(|s| s.as_str())
                            .unwrap_or("connection")
                            .to_string();

                        let auth_response = json!({
                            "jsonrpc": "2.0",
                            "id": id,
                            "result": {
                                "access_token": "mock_access_token_12345",
                                "refresh_token": "mock_refresh_token_67890",
                                "expires_in": 900,
                                "scope": scope,
                                "token_type": "bearer",
                                "enabled_features": []
                            },
                            "testnet": true,
                            "usIn": 1699999999000000_u64,
                            "usOut": 1699999999001000_u64,
                            "usDiff": 1000
                        });

                        if socket
                            .send(Message::Text(auth_response.to_string().into()))
                            .await
                            .is_err()
                        {
                            break;
                        }
                    }
                    Some(DeribitWsMethod::PublicSubscribe | DeribitWsMethod::PrivateSubscribe) => {
                        if let Some(params) = payload.get("params")
                            && let Some(channels) =
                                params.get("channels").and_then(|c| c.as_array())
                        {
                            let mut subscribed_channels = Vec::new();

                            for channel in channels {
                                if let Some(channel_str) = channel.as_str() {
                                    state
                                        .subscription_events
                                        .lock()
                                        .await
                                        .push((channel_str.to_string(), true));
                                    state
                                        .subscriptions
                                        .lock()
                                        .await
                                        .push(channel_str.to_string());
                                    subscribed_channels.push(channel_str.to_string());
                                }
                            }

                            let response = json!({
                                "jsonrpc": "2.0",
                                "id": id,
                                "result": subscribed_channels,
                                "testnet": true,
                                "usIn": 1699999999000000_u64,
                                "usOut": 1699999999001000_u64,
                                "usDiff": 1000
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
                    Some(
                        DeribitWsMethod::PublicUnsubscribe | DeribitWsMethod::PrivateUnsubscribe,
                    ) => {
                        if let Some(params) = payload.get("params")
                            && let Some(channels) =
                                params.get("channels").and_then(|c| c.as_array())
                        {
                            let mut unsubscribed = Vec::new();

                            for channel in channels {
                                if let Some(channel_str) = channel.as_str() {
                                    unsubscribed.push(channel_str.to_string());
                                }
                            }

                            let response = json!({
                                "jsonrpc": "2.0",
                                "id": id,
                                "result": unsubscribed,
                                "testnet": true
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
                    Some(DeribitWsMethod::SetHeartbeat) => {
                        let response = json!({
                            "jsonrpc": "2.0",
                            "id": id,
                            "result": "ok",
                            "testnet": true
                        });

                        if socket
                            .send(Message::Text(response.to_string().into()))
                            .await
                            .is_err()
                        {
                            break;
                        }
                    }
                    Some(DeribitWsMethod::Test) => {
                        let response = json!({
                            "jsonrpc": "2.0",
                            "id": id,
                            "result": {
                                "version": "1.2.26"
                            },
                            "testnet": true
                        });

                        if socket
                            .send(Message::Text(response.to_string().into()))
                            .await
                            .is_err()
                        {
                            break;
                        }
                    }
                    Some(DeribitWsMethod::Buy | DeribitWsMethod::Sell) => {
                        state.command_request_count.fetch_add(1, Ordering::Relaxed);
                        let response_kind = state.command_responses.lock().await.submit;
                        let result = order_response_result(&payload);
                        if !send_command_response(&mut socket, id, response_kind, &result).await {
                            break;
                        }
                    }
                    Some(DeribitWsMethod::Cancel) => {
                        state.command_request_count.fetch_add(1, Ordering::Relaxed);
                        let response_kind = state.command_responses.lock().await.cancel;
                        let result = order_message_result(&payload, CommandOrderState::Cancelled);
                        if !send_command_response(&mut socket, id, response_kind, &result).await {
                            break;
                        }
                    }
                    Some(DeribitWsMethod::Edit) => {
                        state.command_request_count.fetch_add(1, Ordering::Relaxed);
                        let response_kind = state.command_responses.lock().await.modify;
                        let result = order_response_result(&payload);
                        if !send_command_response(&mut socket, id, response_kind, &result).await {
                            break;
                        }
                    }
                    Some(DeribitWsMethod::CancelAllByInstrument) => {
                        state.command_request_count.fetch_add(1, Ordering::Relaxed);
                        let response_kind = state.command_responses.lock().await.cancel_all;
                        let result = json!(0);
                        if !send_command_response(&mut socket, id, response_kind, &result).await {
                            break;
                        }
                    }
                    _ => {}
                }
            }
            Message::Ping(_) if socket.send(Message::Pong(vec![].into())).await.is_err() => {
                break;
            }
            Message::Close(_) => break,
            _ => {}
        }
    }

    let mut count = state.connection_count.lock().await;
    *count = count.saturating_sub(1);
}

async fn send_command_response(
    socket: &mut WebSocket,
    id: Option<u64>,
    response_kind: CommandResponse,
    success_result: &Value,
) -> bool {
    let Some(response) = command_response(id, response_kind, success_result) else {
        return true;
    };

    socket
        .send(Message::Text(response.to_string().into()))
        .await
        .is_ok()
}

fn command_response(
    id: Option<u64>,
    response_kind: CommandResponse,
    success_result: &Value,
) -> Option<Value> {
    match response_kind {
        CommandResponse::Success => Some(json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": success_result,
            "testnet": true,
        })),
        CommandResponse::AmbiguousFailure => None,
        CommandResponse::MalformedSuccess => Some(json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": {
                "unexpected": true
            },
            "testnet": true,
        })),
        CommandResponse::VenueReject { code, message } => Some(json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": {
                "code": code,
                "message": message
            },
            "testnet": true,
        })),
    }
}

fn order_response_result(request: &Value) -> Value {
    json!({
        "order": order_message_result(request, CommandOrderState::Open),
        "trades": []
    })
}

fn order_message_result(request: &Value, order_state: CommandOrderState) -> Value {
    let params = request.get("params").unwrap_or(&Value::Null);
    let label = params
        .get("label")
        .and_then(|value| value.as_str())
        .unwrap_or("deribit-test-order");
    let instrument_name = params
        .get("instrument_name")
        .and_then(|value| value.as_str())
        .unwrap_or("BTC-PERPETUAL");
    let amount = params.get("amount").cloned().unwrap_or_else(|| json!(1.0));
    let price = params
        .get("price")
        .cloned()
        .unwrap_or_else(|| json!(50000.0));

    json!({
        "label": label,
        "price": price,
        "amount": amount,
        "direction": "buy",
        "time_in_force": "good_til_cancelled",
        "instrument_name": instrument_name,
        "api": true,
        "order_id": "DERIBIT-ORDER-1",
        "creation_timestamp": 1767978363493_u64,
        "filled_amount": 0.0,
        "last_update_timestamp": 1767978363493_u64,
        "post_only": true,
        "reduce_only": false,
        "average_price": 0.0,
        "order_state": order_state.as_str(),
        "order_type": "limit",
    })
}

fn create_test_router(state: TestServerState) -> Router {
    Router::new()
        .route("/api/v2", post(handle_jsonrpc_request))
        .route("/ws/api/v2", get(handle_ws_upgrade))
        .route("/health", get(|| async { "OK" }))
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

    Ok((addr, state))
}

fn create_test_exec_config(addr: SocketAddr) -> DeribitExecClientConfig {
    DeribitExecClientConfig {
        trader_id: TraderId::from("TESTER-001"),
        account_id: AccountId::from("DERIBIT-001"),
        api_key: Some("test_api_key".to_string()),
        api_secret: Some("test_api_secret".to_string()),
        product_types: vec![DeribitProductType::Future],
        base_url_http: Some(format!("http://{addr}/api/v2")),
        base_url_ws: Some(format!("ws://{addr}/ws/api/v2")),
        environment: DeribitEnvironment::Testnet,
        http_timeout_secs: 10,
        max_retries: 1,
        retry_delay_initial_ms: 100,
        retry_delay_max_ms: 1000,
        proxy_url: None,
        transport_backend: Default::default(),
    }
}

fn create_test_execution_client(
    addr: SocketAddr,
) -> (
    DeribitExecutionClient,
    tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
    Rc<RefCell<Cache>>,
) {
    let trader_id = TraderId::from("TESTER-001");
    let account_id = AccountId::from("DERIBIT-001");
    let client_id = *DERIBIT_CLIENT_ID;

    let cache = Rc::new(RefCell::new(Cache::default()));

    let core = ExecutionClientCore::new(
        trader_id,
        client_id,
        *DERIBIT_VENUE,
        OmsType::Netting,
        account_id,
        AccountType::Margin,
        None,
        cache.clone(),
    );

    let config = create_test_exec_config(addr);

    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    set_exec_event_sender(tx);

    let mut client = DeribitExecutionClient::new(core, config).unwrap();
    client.start().unwrap();

    (client, rx, cache)
}

fn add_test_account_to_cache(cache: &Rc<RefCell<Cache>>, account_id: AccountId) {
    let account_state = AccountState::new(
        account_id,
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

#[rstest]
#[tokio::test]
async fn test_exec_client_creation() {
    let (addr, _state) = start_test_server().await.unwrap();
    let (client, _rx, _cache) = create_test_execution_client(addr);

    assert_eq!(client.client_id(), *DERIBIT_CLIENT_ID);
    assert_eq!(client.venue(), *DERIBIT_VENUE);
    assert_eq!(client.oms_type(), OmsType::Netting);
    assert!(!client.is_connected());
}

#[rstest]
#[tokio::test]
async fn test_exec_client_connect_disconnect() {
    let (addr, state) = start_test_server().await.unwrap();
    let (mut client, _rx, cache) = create_test_execution_client(addr);
    add_test_account_to_cache(&cache, AccountId::from("DERIBIT-001"));

    client.connect().await.unwrap();

    wait_until_async(
        || async { *state.connection_count.lock().await > 0 },
        Duration::from_secs(10),
    )
    .await;

    assert!(client.is_connected());
    assert!(state.authenticated.load(Ordering::Relaxed));

    let subs = state.subscriptions.lock().await;
    assert!(
        subs.iter().any(|s| s.contains("user.orders")),
        "Expected user.orders subscription, found: {subs:?}"
    );
    assert!(
        subs.iter().any(|s| s.contains("user.trades")),
        "Expected user.trades subscription, found: {subs:?}"
    );
    assert!(
        subs.iter().any(|s| s.contains("user.portfolio")),
        "Expected user.portfolio subscription, found: {subs:?}"
    );
    drop(subs);

    client.disconnect().await.unwrap();
    assert!(!client.is_connected());
}

#[rstest]
#[tokio::test]
async fn test_exec_client_connect_emits_account_state() {
    let (addr, state) = start_test_server().await.unwrap();
    let (mut client, mut rx, cache) = create_test_execution_client(addr);
    add_test_account_to_cache(&cache, AccountId::from("DERIBIT-001"));

    client.connect().await.unwrap();

    wait_until_async(
        || async { state.authenticated.load(Ordering::Relaxed) },
        Duration::from_secs(10),
    )
    .await;

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
}

#[rstest]
#[tokio::test]
async fn test_ambiguous_submit_failure_does_not_emit_order_rejected() {
    let (client, mut rx, cache, request_count) =
        connected_client_with_command_responses(CommandResponses {
            submit: CommandResponse::AmbiguousFailure,
            ..Default::default()
        })
        .await;

    let client_order_id = ClientOrderId::new("ambiguous-submit-test-001");
    let order_any = add_limit_order_to_cache(&cache, client_order_id, TimeInForce::Gtc);

    client
        .submit_order(submit_order_command(&order_any))
        .unwrap();

    wait_for_command_requests(&request_count, 1).await;

    assert_no_order_event_matching(&mut rx, |event| {
        matches!(
            event,
            OrderEventAny::Rejected(event) if event.client_order_id == client_order_id
        )
    })
    .await;
}

#[rstest]
#[tokio::test]
async fn test_submit_response_parse_failure_does_not_emit_order_rejected() {
    let (client, mut rx, cache, request_count) =
        connected_client_with_command_responses(CommandResponses {
            submit: CommandResponse::MalformedSuccess,
            ..Default::default()
        })
        .await;

    let client_order_id = ClientOrderId::new("parse-fail-submit-test-001");
    let order_any = add_limit_order_to_cache(&cache, client_order_id, TimeInForce::Gtc);

    client
        .submit_order(submit_order_command(&order_any))
        .unwrap();

    wait_for_command_requests(&request_count, 1).await;

    assert_no_order_event_matching(&mut rx, |event| {
        matches!(
            event,
            OrderEventAny::Rejected(event) if event.client_order_id == client_order_id
        )
    })
    .await;
}

#[rstest]
#[tokio::test]
async fn test_explicit_venue_submit_rejection_emits_order_rejected() {
    let (client, mut rx, cache, _request_count) =
        connected_client_with_command_responses(CommandResponses {
            submit: CommandResponse::VenueReject {
                code: 10044,
                message: "post_only_reject",
            },
            ..Default::default()
        })
        .await;

    let client_order_id = ClientOrderId::new("venue-submit-reject-test-001");
    let order_any = add_limit_order_to_cache(&cache, client_order_id, TimeInForce::Gtc);

    client
        .submit_order(submit_order_command(&order_any))
        .unwrap();

    match recv_until(&mut rx, |event| {
        matches!(
            event,
            ExecutionEvent::Order(OrderEventAny::Rejected(event))
                if event.client_order_id == client_order_id
        )
    })
    .await
    {
        ExecutionEvent::Order(OrderEventAny::Rejected(event)) => {
            assert_eq!(event.client_order_id, client_order_id);
            assert!(event.reason.as_str().contains("post_only_reject"));
        }
        other => panic!("Expected Rejected event, was {other:?}"),
    }
}

#[rstest]
#[tokio::test]
async fn test_local_submit_validation_failure_emits_order_rejected() {
    let (client, mut rx, cache, _request_count) =
        connected_client_with_command_responses(CommandResponses::default()).await;

    let client_order_id = ClientOrderId::new("local-submit-reject-test-001");
    let order_any = add_limit_order_to_cache(&cache, client_order_id, TimeInForce::AtTheOpen);

    client
        .submit_order(submit_order_command(&order_any))
        .unwrap();

    match recv_until(&mut rx, |event| {
        matches!(
            event,
            ExecutionEvent::Order(OrderEventAny::Rejected(event))
                if event.client_order_id == client_order_id
        )
    })
    .await
    {
        ExecutionEvent::Order(OrderEventAny::Rejected(event)) => {
            assert_eq!(event.client_order_id, client_order_id);
            assert!(event.reason.as_str().contains("Unsupported time_in_force"));
        }
        other => panic!("Expected Rejected event, was {other:?}"),
    }
}

#[rstest]
#[tokio::test]
async fn test_ambiguous_cancel_failure_does_not_emit_cancel_rejected() {
    let (client, mut rx, cache, request_count) =
        connected_client_with_command_responses(CommandResponses {
            cancel: CommandResponse::AmbiguousFailure,
            ..Default::default()
        })
        .await;

    let client_order_id = ClientOrderId::new("ambiguous-cancel-test-001");
    add_limit_order_to_cache(&cache, client_order_id, TimeInForce::Gtc);

    client
        .cancel_order(cancel_order_command(client_order_id))
        .unwrap();

    wait_for_command_requests(&request_count, 1).await;

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
async fn test_explicit_venue_cancel_rejection_emits_cancel_rejected() {
    let (client, mut rx, cache, _request_count) =
        connected_client_with_command_responses(CommandResponses {
            cancel: CommandResponse::VenueReject {
                code: 10003,
                message: "order_not_found",
            },
            ..Default::default()
        })
        .await;

    let client_order_id = ClientOrderId::new("venue-cancel-reject-test-001");
    add_limit_order_to_cache(&cache, client_order_id, TimeInForce::Gtc);

    client
        .cancel_order(cancel_order_command(client_order_id))
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
            assert!(event.reason.as_str().contains("order_not_found"));
        }
        other => panic!("Expected CancelRejected event, was {other:?}"),
    }
}

#[rstest]
#[tokio::test]
async fn test_local_cancel_validation_failure_does_not_emit_cancel_rejected() {
    let (client, mut rx, cache, request_count) =
        connected_client_with_command_responses(CommandResponses::default()).await;

    let client_order_id = ClientOrderId::new("local-cancel-invalid-test-001");
    add_limit_order_to_cache(&cache, client_order_id, TimeInForce::Gtc);

    let result = client.cancel_order(cancel_order_command_without_venue_order_id(client_order_id));
    assert!(result.is_ok());

    assert_no_order_event_matching(&mut rx, |event| {
        matches!(
            event,
            OrderEventAny::CancelRejected(event) if event.client_order_id == client_order_id
        )
    })
    .await;

    assert_eq!(request_count.load(Ordering::Relaxed), 0);
}

#[rstest]
#[tokio::test]
async fn test_local_batch_cancel_validation_failure_does_not_emit_cancel_rejected() {
    let (client, mut rx, cache, request_count) =
        connected_client_with_command_responses(CommandResponses::default()).await;

    let client_order_id = ClientOrderId::new("local-batch-cancel-invalid-test-001");
    add_limit_order_to_cache(&cache, client_order_id, TimeInForce::Gtc);

    let result = client.batch_cancel_orders(BatchCancelOrders::new(
        test_trader_id(),
        Some(*DERIBIT_CLIENT_ID),
        test_strategy_id(),
        test_instrument_id(),
        vec![cancel_order_command_without_venue_order_id(client_order_id)],
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    ));
    assert!(result.is_ok());

    assert_no_order_event_matching(&mut rx, |event| {
        matches!(
            event,
            OrderEventAny::CancelRejected(event) if event.client_order_id == client_order_id
        )
    })
    .await;

    assert_eq!(request_count.load(Ordering::Relaxed), 0);
}

#[rstest]
#[tokio::test]
async fn test_ambiguous_modify_failure_does_not_emit_modify_rejected() {
    let (client, mut rx, cache, request_count) =
        connected_client_with_command_responses(CommandResponses {
            modify: CommandResponse::AmbiguousFailure,
            ..Default::default()
        })
        .await;

    let client_order_id = ClientOrderId::new("ambiguous-modify-test-001");
    add_limit_order_to_cache(&cache, client_order_id, TimeInForce::Gtc);

    client
        .modify_order(modify_order_command(client_order_id))
        .unwrap();

    wait_for_command_requests(&request_count, 1).await;

    assert_no_order_event_matching(&mut rx, |event| {
        matches!(
            event,
            OrderEventAny::ModifyRejected(event) if event.client_order_id == client_order_id
        )
    })
    .await;
}

#[rstest]
#[tokio::test]
async fn test_modify_response_parse_failure_does_not_emit_modify_rejected() {
    let (client, mut rx, cache, request_count) =
        connected_client_with_command_responses(CommandResponses {
            modify: CommandResponse::MalformedSuccess,
            ..Default::default()
        })
        .await;

    let client_order_id = ClientOrderId::new("parse-fail-modify-test-001");
    add_limit_order_to_cache(&cache, client_order_id, TimeInForce::Gtc);

    client
        .modify_order(modify_order_command(client_order_id))
        .unwrap();

    wait_for_command_requests(&request_count, 1).await;

    assert_no_order_event_matching(&mut rx, |event| {
        matches!(
            event,
            OrderEventAny::ModifyRejected(event) if event.client_order_id == client_order_id
        )
    })
    .await;
}

#[rstest]
#[tokio::test]
async fn test_explicit_venue_modify_rejection_emits_modify_rejected() {
    let (client, mut rx, cache, _request_count) =
        connected_client_with_command_responses(CommandResponses {
            modify: CommandResponse::VenueReject {
                code: 10003,
                message: "price_not_changed",
            },
            ..Default::default()
        })
        .await;

    let client_order_id = ClientOrderId::new("venue-modify-reject-test-001");
    add_limit_order_to_cache(&cache, client_order_id, TimeInForce::Gtc);

    client
        .modify_order(modify_order_command(client_order_id))
        .unwrap();

    match recv_until(&mut rx, |event| {
        matches!(
            event,
            ExecutionEvent::Order(OrderEventAny::ModifyRejected(event))
                if event.client_order_id == client_order_id
        )
    })
    .await
    {
        ExecutionEvent::Order(OrderEventAny::ModifyRejected(event)) => {
            assert_eq!(event.client_order_id, client_order_id);
            assert!(event.reason.as_str().contains("price_not_changed"));
        }
        other => panic!("Expected ModifyRejected event, was {other:?}"),
    }
}

#[rstest]
#[tokio::test]
async fn test_local_modify_validation_failure_emits_modify_rejected() {
    let (client, mut rx, cache, _request_count) =
        connected_client_with_command_responses(CommandResponses::default()).await;

    let client_order_id = ClientOrderId::new("local-modify-reject-test-001");
    add_limit_order_to_cache(&cache, client_order_id, TimeInForce::Gtc);

    let result = client.modify_order(modify_order_command_without_venue_order_id(client_order_id));
    assert!(result.is_err());

    match recv_until(&mut rx, |event| {
        matches!(
            event,
            ExecutionEvent::Order(OrderEventAny::ModifyRejected(event))
                if event.client_order_id == client_order_id
        )
    })
    .await
    {
        ExecutionEvent::Order(OrderEventAny::ModifyRejected(event)) => {
            assert_eq!(event.client_order_id, client_order_id);
            assert!(event.reason.as_str().contains("venue_order_id required"));
        }
        other => panic!("Expected ModifyRejected event, was {other:?}"),
    }
}

#[rstest]
#[tokio::test]
async fn test_local_modify_missing_price_emits_modify_rejected() {
    let (client, mut rx, cache, _request_count) =
        connected_client_with_command_responses(CommandResponses::default()).await;

    let client_order_id = ClientOrderId::new("local-modify-no-price-test-001");
    add_limit_order_to_cache(&cache, client_order_id, TimeInForce::Gtc);

    let result = client.modify_order(modify_order_command_without_price(client_order_id));
    assert!(result.is_err());

    match recv_until(&mut rx, |event| {
        matches!(
            event,
            ExecutionEvent::Order(OrderEventAny::ModifyRejected(event))
                if event.client_order_id == client_order_id
        )
    })
    .await
    {
        ExecutionEvent::Order(OrderEventAny::ModifyRejected(event)) => {
            assert_eq!(event.client_order_id, client_order_id);
            assert!(event.reason.as_str().contains("price required"));
        }
        other => panic!("Expected ModifyRejected event, was {other:?}"),
    }
}

#[rstest]
#[tokio::test]
async fn test_local_modify_missing_cached_order_emits_modify_rejected() {
    let (client, mut rx, _cache, _request_count) =
        connected_client_with_command_responses(CommandResponses::default()).await;

    let client_order_id = ClientOrderId::new("local-modify-no-cache-test-001");

    let result = client.modify_order(modify_order_command_without_quantity(client_order_id));
    assert!(result.is_err());

    match recv_until(&mut rx, |event| {
        matches!(
            event,
            ExecutionEvent::Order(OrderEventAny::ModifyRejected(event))
                if event.client_order_id == client_order_id
        )
    })
    .await
    {
        ExecutionEvent::Order(OrderEventAny::ModifyRejected(event)) => {
            assert_eq!(event.client_order_id, client_order_id);
            assert!(event.reason.as_str().contains("Order not found"));
        }
        other => panic!("Expected ModifyRejected event, was {other:?}"),
    }
}

#[rstest]
#[tokio::test]
async fn test_whole_cancel_all_failure_does_not_emit_per_order_cancel_rejected() {
    let (client, mut rx, cache, request_count) =
        connected_client_with_command_responses(CommandResponses {
            cancel_all: CommandResponse::AmbiguousFailure,
            ..Default::default()
        })
        .await;

    add_limit_order_to_cache(
        &cache,
        ClientOrderId::new("cancel-all-fail-test-001"),
        TimeInForce::Gtc,
    );
    add_limit_order_to_cache(
        &cache,
        ClientOrderId::new("cancel-all-fail-test-002"),
        TimeInForce::Gtc,
    );

    client
        .cancel_all_orders(cancel_all_orders_command())
        .unwrap();

    wait_for_command_requests(&request_count, 1).await;

    assert_no_order_event_matching(&mut rx, |event| {
        matches!(event, OrderEventAny::CancelRejected(_))
    })
    .await;
}

async fn connected_client_with_command_responses(
    responses: CommandResponses,
) -> (
    DeribitExecutionClient,
    tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
    Rc<RefCell<Cache>>,
    Arc<AtomicUsize>,
) {
    let (addr, state) = start_test_server().await.unwrap();
    *state.command_responses.lock().await = responses;
    let request_count = state.command_request_count.clone();
    let (mut client, rx, cache) = create_test_execution_client(addr);
    add_test_account_to_cache(&cache, AccountId::from("DERIBIT-001"));
    client.connect().await.unwrap();
    (client, rx, cache, request_count)
}

fn test_trader_id() -> TraderId {
    TraderId::from("TESTER-001")
}

fn test_strategy_id() -> StrategyId {
    StrategyId::from("S-001")
}

fn test_instrument_id() -> InstrumentId {
    InstrumentId::from("BTC-PERPETUAL.DERIBIT")
}

fn add_limit_order_to_cache(
    cache: &Rc<RefCell<Cache>>,
    client_order_id: ClientOrderId,
    time_in_force: TimeInForce,
) -> OrderAny {
    let order = LimitOrder::new(
        test_trader_id(),
        test_strategy_id(),
        test_instrument_id(),
        client_order_id,
        OrderSide::Buy,
        Quantity::from("1"),
        Price::from("50000.0"),
        time_in_force,
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

fn submit_order_command(order: &OrderAny) -> SubmitOrder {
    SubmitOrder::new(
        test_trader_id(),
        Some(*DERIBIT_CLIENT_ID),
        test_strategy_id(),
        test_instrument_id(),
        order.client_order_id(),
        order.init_event().clone(),
        None,
        None,
        None,
        UUID4::new(),
        UnixNanos::default(),
        None,
    )
}

fn cancel_order_command(client_order_id: ClientOrderId) -> CancelOrder {
    CancelOrder::new(
        test_trader_id(),
        Some(*DERIBIT_CLIENT_ID),
        test_strategy_id(),
        test_instrument_id(),
        client_order_id,
        Some(VenueOrderId::from("DERIBIT-ORDER-1")),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    )
}

fn cancel_order_command_without_venue_order_id(client_order_id: ClientOrderId) -> CancelOrder {
    CancelOrder::new(
        test_trader_id(),
        Some(*DERIBIT_CLIENT_ID),
        test_strategy_id(),
        test_instrument_id(),
        client_order_id,
        None,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    )
}

fn modify_order_command(client_order_id: ClientOrderId) -> ModifyOrder {
    ModifyOrder::new(
        test_trader_id(),
        Some(*DERIBIT_CLIENT_ID),
        test_strategy_id(),
        test_instrument_id(),
        client_order_id,
        Some(VenueOrderId::from("DERIBIT-ORDER-1")),
        Some(Quantity::from("2")),
        Some(Price::from("51000.0")),
        None,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    )
}

fn modify_order_command_without_venue_order_id(client_order_id: ClientOrderId) -> ModifyOrder {
    ModifyOrder::new(
        test_trader_id(),
        Some(*DERIBIT_CLIENT_ID),
        test_strategy_id(),
        test_instrument_id(),
        client_order_id,
        None,
        Some(Quantity::from("2")),
        Some(Price::from("51000.0")),
        None,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    )
}

fn modify_order_command_without_price(client_order_id: ClientOrderId) -> ModifyOrder {
    ModifyOrder::new(
        test_trader_id(),
        Some(*DERIBIT_CLIENT_ID),
        test_strategy_id(),
        test_instrument_id(),
        client_order_id,
        Some(VenueOrderId::from("DERIBIT-ORDER-1")),
        Some(Quantity::from("2")),
        None,
        None,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    )
}

fn modify_order_command_without_quantity(client_order_id: ClientOrderId) -> ModifyOrder {
    ModifyOrder::new(
        test_trader_id(),
        Some(*DERIBIT_CLIENT_ID),
        test_strategy_id(),
        test_instrument_id(),
        client_order_id,
        Some(VenueOrderId::from("DERIBIT-ORDER-1")),
        None,
        Some(Price::from("51000.0")),
        None,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    )
}

fn cancel_all_orders_command() -> CancelAllOrders {
    CancelAllOrders::new(
        test_trader_id(),
        Some(*DERIBIT_CLIENT_ID),
        test_strategy_id(),
        test_instrument_id(),
        OrderSide::NoOrderSide,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    )
}

async fn wait_for_command_requests(request_count: &AtomicUsize, expected: usize) {
    wait_until_async(
        || async { request_count.load(Ordering::Relaxed) >= expected },
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
