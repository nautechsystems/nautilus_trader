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
    http::StatusCode,
    response::{IntoResponse, Json, Response},
    routing::{get, post},
};
use nautilus_common::{
    cache::Cache,
    clients::ExecutionClient,
    clock::VirtualClock,
    live::runner::{
        replace_exec_event_sender, replace_system_event_sender, set_exec_event_sender,
        try_get_exec_event_sender,
    },
    messages::{
        ExecutionEvent, SystemEvent,
        execution::{
            BatchCancelOrders, CancelAllOrders, CancelOrder, ExecutionReport, GenerateFillReports,
            GenerateOrderStatusReport, GenerateOrderStatusReports, GeneratePositionStatusReports,
            ModifyOrder, SubmitOrder, SubmitOrderList,
        },
        system::SocketState,
    },
    testing::wait_until_async,
};
use nautilus_core::{DurationNanos, UUID4, UnixNanos};
use nautilus_deribit::{
    common::{
        consts::{DERIBIT_CLIENT_ID, DERIBIT_VENUE},
        enums::DeribitEnvironment,
    },
    config::DeribitExecutionClientConfig,
    execution::DeribitExecutionClient,
    http::{
        client::DeribitHttpClient,
        models::{DeribitCurrency, DeribitProductType},
    },
    websocket::enums::DeribitWsMethod,
};
use nautilus_live::{
    ExecutionClientCore, SocketReconnectRegistry, SocketReconnectRequestOutcome,
    execution::manager::{ExecutionManager, ExecutionManagerConfig},
};
use nautilus_model::{
    accounts::{AccountAny, MarginAccount},
    enums::{
        AccountType, LiquiditySide, OmsType, OrderSide, OrderStatus, OrderType, PositionSide,
        TimeInForce, TriggerType,
    },
    events::{AccountState, OrderEventAny},
    identifiers::{
        AccountId, ClientOrderId, InstrumentId, OrderListId, StrategyId, TradeId, TraderId,
        VenueOrderId,
    },
    orders::{Order, OrderAny, OrderList, OrderTestBuilder, stubs::TestOrderEventStubs},
    reports::{FillReport, OrderStatusReport},
    types::{AccountBalance, Currency, Money, Price, Quantity},
};
use nautilus_network::http::HttpClient;
use rstest::rstest;
use rust_decimal_macros::dec;
use serde::Deserialize;
use serde_json::{Value, json, value::RawValue};
use ustr::Ustr;

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
    command_requests: Arc<tokio::sync::Mutex<Vec<Value>>>,
    command_request_texts: Arc<tokio::sync::Mutex<Vec<String>>>,
    order_lookup_responses: Arc<tokio::sync::Mutex<HashMap<String, (StatusCode, Value)>>>,
    order_lookup_requests: Arc<tokio::sync::Mutex<Vec<Value>>>,
    user_trades_result: Arc<tokio::sync::Mutex<Value>>,
    positions_result: Arc<tokio::sync::Mutex<Option<Value>>>,
    instrument_requests: Arc<AtomicUsize>,
}

async fn handle_jsonrpc_request(
    State(state): State<TestServerState>,
    Json(request): Json<Value>,
) -> impl IntoResponse {
    let method = request.get("method").and_then(|m| m.as_str()).unwrap_or("");
    let id = request.get("id").and_then(|i| i.as_u64()).unwrap_or(0);
    let params = request.get("params").cloned();

    match method {
        "public/get_instruments" => handle_get_instruments(id, params).await,
        "public/get_instrument" => {
            state.instrument_requests.fetch_add(1, Ordering::Relaxed);
            let name = params
                .as_ref()
                .and_then(|value| value.get("instrument_name"))
                .and_then(|value| value.as_str());

            let mut data = if name == Some("BTC_USDC") {
                json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": load_json("http_get_instrument_routed_spot.json"),
                    "usOut": 1_765_297_564_479_750_u64,
                    "testnet": true
                })
            } else {
                let mut future = load_json("http_get_instrument.json");
                future["id"] = json!(id);
                future
            };

            if let Some(name) = name
                && let Some(result) = data.get_mut("result")
            {
                result["instrument_name"] = json!(name);
            }

            Json(data).into_response()
        }
        "private/get_account_summaries" => {
            let mut data = load_json("http_get_account_summaries.json");
            data["id"] = json!(id);
            Json(data).into_response()
        }
        "private/get_order_state"
        | "private/get_open_orders"
        | "private/get_open_orders_by_instrument"
        | "private/get_order_history_by_instrument"
        | "private/get_order_history_by_currency" => {
            state
                .order_lookup_requests
                .lock()
                .await
                .push(request.clone());
            let (status, mut response) = state
                .order_lookup_responses
                .lock()
                .await
                .get(method)
                .cloned()
                .unwrap_or_else(|| (StatusCode::OK, json!({"jsonrpc": "2.0", "result": []})));

            if let Some(body) = response.as_str() {
                return (status, body.to_string()).into_response();
            }

            response["jsonrpc"] = json!("2.0");
            response["id"] = json!(id);
            (status, Json(response)).into_response()
        }
        "private/get_positions" => {
            let mut data = if let Some(result) = state.positions_result.lock().await.clone() {
                json!({"jsonrpc": "2.0", "result": result})
            } else {
                load_json("http_get_positions.json")
            };

            data["id"] = json!(id);
            Json(data).into_response()
        }
        "private/get_user_trades_by_instrument_and_time"
        | "private/get_user_trades_by_currency_and_time" => {
            Json(user_trades_response(&state, method, id, params).await).into_response()
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
                && kind == "spot"
            {
                return Json(json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": [load_json("http_get_instrument_routed_spot.json")],
                    "usOut": 1_765_297_564_479_750_u64,
                    "testnet": true
                }))
                .into_response();
            }

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
                        state.command_requests.lock().await.push(payload.clone());
                        state
                            .command_request_texts
                            .lock()
                            .await
                            .push(text.to_string());
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
                        state
                            .command_request_texts
                            .lock()
                            .await
                            .push(text.to_string());
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

async fn user_trades_response(
    state: &TestServerState,
    method: &str,
    id: u64,
    params: Option<Value>,
) -> Value {
    let stored = state.user_trades_result.lock().await.clone();
    let mut result = if stored.get("trades").is_some() {
        stored
    } else {
        json!({"has_more": false, "trades": []})
    };

    if let Some(trades) = result
        .get_mut("trades")
        .and_then(|value| value.as_array_mut())
    {
        if method.ends_with("by_instrument_and_time") {
            let instrument_name = params
                .as_ref()
                .and_then(|value| value.get("instrument_name"))
                .and_then(|value| value.as_str())
                .unwrap_or("");
            trades.retain(|trade| {
                trade
                    .get("instrument_name")
                    .and_then(|value| value.as_str())
                    == Some(instrument_name)
            });
        } else if let Some(currency) = params
            .as_ref()
            .and_then(|value| value.get("currency"))
            .and_then(|value| value.as_str())
        {
            let hyphen = format!("{currency}-");
            let spot = format!("{currency}_");
            trades.retain(|trade| {
                trade
                    .get("instrument_name")
                    .and_then(|value| value.as_str())
                    .is_some_and(|name| name.starts_with(&hyphen) || name.starts_with(&spot))
            });
        }
    }

    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": result,
        "testnet": true,
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
    let http_client = HttpClient::builder().build().unwrap();
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

fn create_test_exec_config(addr: SocketAddr) -> DeribitExecutionClientConfig {
    DeribitExecutionClientConfig {
        account_id: AccountId::from("DERIBIT-001"),
        api_key: Some("test_api_key".into()),
        api_secret: Some("test_api_secret".into()),
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
        ..Default::default()
    }
}

fn create_test_execution_client_with_config(
    config: DeribitExecutionClientConfig,
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

    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    if try_get_exec_event_sender().is_none() {
        set_exec_event_sender(tx);
    } else {
        replace_exec_event_sender(tx);
    }

    let mut client = DeribitExecutionClient::new(core, config).unwrap();
    client.start().unwrap();

    (client, rx, cache)
}

fn create_test_execution_client(
    addr: SocketAddr,
) -> (
    DeribitExecutionClient,
    tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
    Rc<RefCell<Cache>>,
) {
    create_test_execution_client_with_config(create_test_exec_config(addr))
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
    let (system_tx, mut system_rx) = tokio::sync::mpsc::unbounded_channel();
    replace_system_event_sender(system_tx);
    let (addr, state) = start_test_server().await.unwrap();
    let registry = SocketReconnectRegistry::default();
    let (mut client, _rx, cache) = registry.scope(|| create_test_execution_client(addr));
    add_test_account_to_cache(&cache, AccountId::from("DERIBIT-001"));

    client.connect().await.unwrap();

    wait_until_async(
        || async { *state.connection_count.lock().await > 0 },
        Duration::from_secs(10),
    )
    .await;
    let event = tokio::time::timeout(Duration::from_secs(10), system_rx.recv())
        .await
        .unwrap()
        .unwrap();
    let SystemEvent::SocketState(change) = event;
    let endpoint = Ustr::from("deribit-user-streams");
    let handle = registry.handle(*DERIBIT_CLIENT_ID, endpoint).unwrap();

    assert!(client.is_connected());
    assert!(state.authenticated.load(Ordering::Relaxed));
    assert_eq!(change.client_id, *DERIBIT_CLIENT_ID);
    assert_eq!(change.venue, Some(*DERIBIT_VENUE));
    assert_eq!(change.endpoint, endpoint);
    assert_eq!(change.state, SocketState::Connected);
    assert_eq!(
        handle.request_reconnect(),
        SocketReconnectRequestOutcome::Accepted
    );
    let event = tokio::time::timeout(Duration::from_secs(10), system_rx.recv())
        .await
        .unwrap()
        .unwrap();
    let SystemEvent::SocketState(change) = event;
    assert_eq!(change.endpoint, endpoint);
    assert_eq!(change.state, SocketState::Disconnected);

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
    assert!(registry.handle(*DERIBIT_CLIENT_ID, endpoint).is_none());
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
#[case::venue_valid("valid", true, true, None)]
#[case::venue_only("venue_only", true, true, None)]
#[case::venue_missing_label("missing_label", true, true, None)]
#[case::venue_absent("absent", true, true, None)]
#[case::venue_auth_failure("auth", true, true, Some("13004"))]
#[case::venue_unavailable("unavailable", true, true, Some("503"))]
#[case::venue_malformed("malformed", true, true, Some("JSON error"))]
#[case::venue_missing_result("missing_result", true, true, Some("neither result nor error"))]
#[case::venue_uncached("uncached", true, true, Some("not found"))]
#[case::venue_parse_failure("parse", true, true, Some("Unknown order direction"))]
#[case::venue_wrong_id("wrong_venue", true, true, Some("venue_order_id=DERIBIT-OTHER"))]
#[case::venue_wrong_label("wrong_client", true, true, Some("client_order_id=Some(\"O-OTHER\")"))]
#[case::venue_wrong_instrument(
    "wrong_instrument",
    true,
    true,
    Some("instrument_id=BTC-27DEC24.DERIBIT")
)]
#[case::client_open("valid", false, true, None)]
#[case::client_closed("closed", false, true, None)]
#[case::client_unscoped("valid", false, false, None)]
#[case::client_empty("empty", false, true, Some("inconclusive"))]
#[case::client_uncached("uncached", false, true, Some("inconclusive"))]
#[case::client_parse_failure("parse", false, true, Some("inconclusive"))]
#[case::client_unscoped_uncached("uncached", false, false, Some("unresolved instrument"))]
#[case::client_unscoped_parse_failure("parse", false, false, Some("inconclusive"))]
#[case::client_auth_failure("auth", false, true, Some("13004"))]
#[case::client_absence_error("absent", false, true, Some("10004"))]
#[case::client_malformed("malformed", false, true, Some("JSON error"))]
#[case::client_wrong_instrument("wrong_instrument", false, true, Some("inconclusive"))]
#[tokio::test]
async fn test_generate_order_status_report(
    #[case] scenario: &str,
    #[case] venue_id: bool,
    #[case] scoped: bool,
    #[case] error: Option<&str>,
) {
    let (addr, state) = start_test_server().await.unwrap();
    let (mut client, _rx, cache) = create_test_execution_client(addr);
    add_test_account_to_cache(&cache, AccountId::from("DERIBIT-001"));
    client.connect().await.unwrap();

    let client_order_id = ClientOrderId::from("O-LOOKUP-17");
    let venue_order_id = VenueOrderId::from("DERIBIT-ORDER-1");
    let response = order_lookup_response(scenario, venue_id, client_order_id);

    let first_method = match (venue_id, scoped) {
        (true, _) => "private/get_order_state",
        (false, true) => "private/get_open_orders_by_instrument",
        (false, false) => "private/get_open_orders",
    };

    let method = if scenario == "closed" {
        "private/get_order_history_by_instrument"
    } else {
        first_method
    };

    state.order_lookup_responses.lock().await.insert(
        method.to_string(),
        (
            match scenario {
                "unavailable" => StatusCode::SERVICE_UNAVAILABLE,
                "absent" => StatusCode::BAD_REQUEST,
                _ => StatusCode::OK,
            },
            response,
        ),
    );

    let cmd = GenerateOrderStatusReport::new(
        UUID4::new(),
        UnixNanos::default(),
        scoped.then(test_instrument_id),
        (scenario != "venue_only").then_some(client_order_id),
        venue_id.then_some(venue_order_id),
        None,
        None,
    );

    let result = client.generate_order_status_report(&cmd).await;
    let requests = state.order_lookup_requests.lock().await.clone();
    client.disconnect().await.unwrap();

    let order_status = if scenario == "closed" {
        OrderStatus::Canceled
    } else {
        OrderStatus::Accepted
    };

    let expected_requests = if scenario == "unavailable" { 2 } else { 1 };

    if let Some(message) = error {
        assert!(result.unwrap_err().to_string().contains(message));
    } else if scenario == "absent" {
        assert_eq!(result.unwrap(), None);
    } else {
        let report = result.unwrap().unwrap();

        let expected = OrderStatusReport::new(
            AccountId::from("DERIBIT-001"),
            test_instrument_id(),
            (scenario != "missing_label").then_some(client_order_id),
            venue_order_id,
            Some(OrderSide::Buy),
            OrderType::Limit,
            TimeInForce::Gtc,
            order_status,
            Quantity::from("12"),
            Quantity::from("0"),
            UnixNanos::from(1_767_978_363_493_000_000),
            UnixNanos::from(1_767_978_363_493_000_000),
            report.ts_init,
            Some(report.report_id),
        )
        .with_price(Price::from("54321.5"))
        .with_post_only(true);

        assert_eq!(report, expected);
    }

    assert_eq!(requests[0]["method"], first_method);

    if venue_id {
        assert_eq!(requests.len(), expected_requests);
        assert_eq!(requests[0]["params"], json!({"order_id": venue_order_id}));
    } else if scoped && error.is_none() {
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0]["params"]["instrument_name"], "BTC-PERPETUAL");
        assert_eq!(
            requests[1]["method"],
            "private/get_order_history_by_instrument"
        );
        assert_eq!(requests[1]["params"]["include_old"], true);
        assert_eq!(requests[1]["params"]["include_unfilled"], true);
    }
}

fn order_lookup_response(scenario: &str, venue_id: bool, client_order_id: ClientOrderId) -> Value {
    let mut order = order_message_result(
        &json!({"params": {"label": client_order_id, "amount": 12, "price": 54321.5}}),
        CommandOrderState::Open,
    );

    match scenario {
        "uncached" => order["instrument_name"] = json!("BTC-UNKNOWN"),
        "parse" => order["direction"] = json!("invalid"),
        "wrong_venue" => order["order_id"] = json!("DERIBIT-OTHER"),
        "wrong_client" => order["label"] = json!("O-OTHER"),
        "missing_label" => order["label"] = Value::Null,
        "wrong_instrument" => order["instrument_name"] = json!("BTC-27DEC24"),
        "closed" => order["order_state"] = json!("cancelled"),
        _ => {}
    }

    let result = if venue_id { order } else { json!([order]) };

    match scenario {
        "absent" => json!({"error": {"code": 10004, "message": "order_not_found"}}),
        "auth" => json!({"error": {"code": 13004, "message": "invalid_credentials"}}),
        "unavailable" => json!("service unavailable"),
        "malformed" => json!({"result": {"unexpected": true}}),
        "missing_result" => json!({"jsonrpc": "2.0"}),
        "empty" => json!({"result": []}),
        _ => json!({"result": result}),
    }
}

#[tokio::test]
async fn test_generate_order_status_report_requires_identifier() {
    let (addr, state) = start_test_server().await.unwrap();
    let (client, _rx, _cache) = create_test_execution_client(addr);

    let cmd = GenerateOrderStatusReport::new(
        UUID4::new(),
        UnixNanos::default(),
        Some(test_instrument_id()),
        None,
        None,
        None,
        None,
    );

    let result = client.generate_order_status_report(&cmd).await;

    assert_eq!(
        result.unwrap_err().to_string(),
        "Order lookup requires a client order ID or venue order ID"
    );
    assert_eq!(state.order_lookup_requests.lock().await.len(), 0);
}

#[rstest]
#[case::failed_lookup(13004, None)]
#[case::definitive_absence(10004, Some(OrderStatus::Rejected))]
#[tokio::test]
async fn test_recovery_order_lookup_preserves_failure(
    #[case] code: i64,
    #[case] terminal: Option<OrderStatus>,
) {
    let (addr, state) = start_test_server().await.unwrap();
    let (mut client, _rx, cache) = create_test_execution_client(addr);
    add_test_account_to_cache(&cache, AccountId::from("DERIBIT-001"));
    client.connect().await.unwrap();
    state.order_lookup_responses.lock().await.insert(
        "private/get_order_state".to_string(),
        (StatusCode::OK, json!({"error": {"code": code, "message": if code == 10004 { "order_not_found" } else { "invalid_credentials" }}})),
    );
    let client_order_id = ClientOrderId::from("O-RECOVERY-23");
    let venue_order_id = VenueOrderId::from("DERIBIT-ORDER-1");
    let order = OrderTestBuilder::new(OrderType::Limit)
        .trader_id(test_trader_id())
        .strategy_id(test_strategy_id())
        .instrument_id(test_instrument_id())
        .client_order_id(client_order_id)
        .quantity(Quantity::from("12"))
        .price(Price::from("54321.5"))
        .build();
    let submitted = TestOrderEventStubs::submitted(&order, client.account_id());
    let accepted = TestOrderEventStubs::accepted(&order, client.account_id(), venue_order_id);
    cache
        .borrow_mut()
        .add_order(order, None, Some(client.client_id()), false)
        .unwrap();
    cache.borrow_mut().update_order(&submitted).unwrap();
    cache.borrow_mut().update_order(&accepted).unwrap();
    let clock = Rc::new(RefCell::new(VirtualClock::new()));

    let mut manager = ExecutionManager::new(
        clock,
        cache.clone(),
        ExecutionManagerConfig {
            open_check_threshold_ns: DurationNanos::ZERO,
            open_check_missing_retries: 1,
            open_check_open_only: false,
            single_order_query_delay_ms: 0,
            ..Default::default()
        },
    )
    .unwrap();

    let events = manager.check_open_orders(&[&client]).await;
    for event in &events {
        cache.borrow_mut().update_order(event).unwrap();
    }

    let status = cache.borrow().order(&client_order_id).unwrap().status();
    let requests = state.order_lookup_requests.lock().await.clone();
    client.disconnect().await.unwrap();

    assert_eq!(events.len(), usize::from(terminal.is_some()));
    assert_eq!(status, terminal.unwrap_or(OrderStatus::Accepted));
    assert_eq!(
        requests
            .iter()
            .filter(|request| request["method"] == "private/get_order_state")
            .count(),
        1
    );

    if let Some(event) = events.first() {
        assert!(matches!(event, OrderEventAny::Rejected(rejected)
            if rejected.client_order_id == client_order_id && rejected.reason == "NOT_FOUND_AT_VENUE"));
    }
}

#[rstest]
#[tokio::test]
async fn test_generate_position_status_reports_with_fractional_leverage() {
    let (addr, _state) = start_test_server().await.unwrap();
    let (mut client, _rx, cache) = create_test_execution_client(addr);
    add_test_account_to_cache(&cache, AccountId::from("DERIBIT-001"));

    client.connect().await.unwrap();

    let cmd = GeneratePositionStatusReports::new(
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
        None,
        None,
        None,
    );

    let reports = client.generate_position_status_reports(&cmd).await.unwrap();

    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].account_id, AccountId::from("DERIBIT-001"));
    assert_eq!(reports[0].instrument_id, test_instrument_id());
    assert_eq!(reports[0].position_side, PositionSide::Long);
    assert_eq!(reports[0].quantity, Quantity::from("50"));
    assert_eq!(reports[0].signed_decimal_qty, dec!(50));
    assert_eq!(reports[0].venue_position_id, None);
    assert_eq!(reports[0].avg_px_open, Some(dec!(7440.18)));

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
            assert!(event.reason.contains("post_only_reject"));
        }
        other => panic!("Expected Rejected event, was {other:?}"),
    }
}

#[rstest]
#[tokio::test]
async fn test_local_submit_validation_failure_returns_error() {
    let (client, _rx, cache, request_count) =
        connected_client_with_command_responses(CommandResponses::default()).await;

    let client_order_id = ClientOrderId::new("local-submit-reject-test-001");
    let order_any = add_limit_order_to_cache(&cache, client_order_id, TimeInForce::AtTheOpen);

    let error = client
        .submit_order(submit_order_command(&order_any))
        .unwrap_err();

    assert_eq!(
        error.to_string(),
        "Unsupported time_in_force AtTheOpen for Deribit"
    );
    assert_eq!(request_count.load(Ordering::Relaxed), 0);
}

#[rstest]
#[tokio::test]
async fn test_submit_order_list_sends_all_valid_orders() {
    let (addr, state) = start_test_server().await.unwrap();
    let (mut client, mut rx, cache) = create_test_execution_client(addr);
    add_test_account_to_cache(&cache, AccountId::from("DERIBIT-001"));
    client.connect().await.unwrap();
    let sell = OrderTestBuilder::new(OrderType::Limit)
        .trader_id(test_trader_id())
        .strategy_id(test_strategy_id())
        .instrument_id(test_instrument_id())
        .client_order_id(ClientOrderId::from("list-submit-002"))
        .side(OrderSide::Sell)
        .quantity(Quantity::from("20"))
        .price(Price::from("51000"))
        .time_in_force(TimeInForce::Gtc)
        .build();
    cache
        .borrow_mut()
        .add_order(sell.clone(), None, None, false)
        .unwrap();

    let orders = [
        add_sized_limit_order_to_cache(
            &cache,
            ClientOrderId::from("list-submit-001"),
            Quantity::from("10"),
            Price::from("50000"),
        ),
        sell,
    ];

    let order_list = OrderList::new(
        OrderListId::from("list-submit"),
        test_instrument_id(),
        test_strategy_id(),
        orders.iter().map(Order::client_order_id).collect(),
        UnixNanos::default(),
    );
    client
        .submit_order_list(SubmitOrderList::new(
            test_trader_id(),
            Some(*DERIBIT_CLIENT_ID),
            test_strategy_id(),
            order_list,
            orders
                .iter()
                .map(|order| order.init_event().clone())
                .collect(),
            None,
            None,
            None,
            UUID4::new(),
            UnixNanos::default(),
            None,
        ))
        .unwrap();

    let mut submitted = Vec::new();

    for _ in &orders {
        let event = recv_until(&mut rx, |event| {
            matches!(event, ExecutionEvent::Order(OrderEventAny::Submitted(_)))
        })
        .await;

        let ExecutionEvent::Order(OrderEventAny::Submitted(event)) = event else {
            panic!("Expected submitted event, was {event:?}");
        };

        submitted.push(event.client_order_id);
    }

    wait_for_command_requests(&state.command_request_count, 2).await;
    let requests = state.command_requests.lock().await;

    assert_eq!(
        submitted,
        vec![orders[0].client_order_id(), orders[1].client_order_id()]
    );
    assert_eq!(requests.len(), 2);

    for (order, amount, price, method) in [
        (&orders[0], 10, 50000, "private/buy"),
        (&orders[1], 20, 51000, "private/sell"),
    ] {
        let request = find_command_request(&requests, order.client_order_id());
        let params = &request["params"];

        assert_eq!(request["method"].as_str(), Some(method));
        assert_eq!(params["amount"].as_u64(), Some(amount));
        assert_eq!(params["price"].as_u64(), Some(price));
        assert_eq!(params["instrument_name"].as_str(), Some("BTC-PERPETUAL"));
        assert_eq!(params["time_in_force"].as_str(), Some("good_til_cancelled"));
    }

    drop(requests);
    client.disconnect().await.unwrap();
}

#[rstest]
#[case(OrderType::Market, "market")]
#[case(OrderType::StopMarket, "stop_market")]
#[case(OrderType::MarketIfTouched, "take_market")]
#[tokio::test]
async fn test_market_style_submit_omits_time_in_force(
    #[case] order_type: OrderType,
    #[case] deribit_order_type: &str,
) {
    let (addr, state) = start_test_server().await.unwrap();
    let request_count = state.command_request_count.clone();
    let command_requests = state.command_requests.clone();
    let (mut client, _rx, cache) = create_test_execution_client(addr);
    add_test_account_to_cache(&cache, AccountId::from("DERIBIT-001"));
    client.connect().await.unwrap();

    let client_order_id = ClientOrderId::new("market-submit-tif-test-001");
    let order_any = add_order_to_cache(&cache, client_order_id, order_type, TimeInForce::Ioc);

    client
        .submit_order(submit_order_command(&order_any))
        .unwrap();

    wait_for_command_requests(&request_count, 1).await;

    let requests = command_requests.lock().await;
    let params = find_command_request_params(&requests, client_order_id);

    assert_eq!(
        params.get("type").and_then(Value::as_str),
        Some(deribit_order_type)
    );
    assert_eq!(params.get("amount").and_then(Value::as_f64), Some(1.0));
    assert!(params.get("contracts").is_none());
    assert!(
        params.get("time_in_force").is_none(),
        "market-style order params should omit time_in_force, was {params:?}"
    );

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_limit_submit_sends_time_in_force() {
    let (addr, state) = start_test_server().await.unwrap();
    let request_count = state.command_request_count.clone();
    let command_requests = state.command_requests.clone();
    let (mut client, _rx, cache) = create_test_execution_client(addr);
    add_test_account_to_cache(&cache, AccountId::from("DERIBIT-001"));
    client.connect().await.unwrap();

    let client_order_id = ClientOrderId::new("limit-submit-tif-test-001");
    let order_any = add_order_to_cache(&cache, client_order_id, OrderType::Limit, TimeInForce::Fok);

    client
        .submit_order(submit_order_command(&order_any))
        .unwrap();

    wait_for_command_requests(&request_count, 1).await;

    let requests = command_requests.lock().await;
    let params = find_command_request_params(&requests, client_order_id);

    assert_eq!(params.get("type").and_then(Value::as_str), Some("limit"));
    assert_eq!(params.get("amount").and_then(Value::as_f64), Some(1.0));
    assert!(params.get("contracts").is_none());
    assert_eq!(
        params.get("time_in_force").and_then(Value::as_str),
        Some("fill_or_kill")
    );

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_limit_submit_sends_exact_number_tokens() {
    let (addr, state) = start_test_server().await.unwrap();
    let request_count = state.command_request_count.clone();
    let command_request_texts = state.command_request_texts.clone();
    let (mut client, _rx, cache) = create_test_execution_client(addr);
    add_test_account_to_cache(&cache, AccountId::from("DERIBIT-001"));
    client.connect().await.unwrap();

    let client_order_id = ClientOrderId::new("limit-submit-exact-test-001");
    let order_any = add_sized_limit_order_to_cache(
        &cache,
        client_order_id,
        Quantity::from("10"),
        Price::from("65000.50"),
    );

    client
        .submit_order(submit_order_command(&order_any))
        .unwrap();

    wait_for_command_requests(&request_count, 1).await;

    let texts = command_request_texts.lock().await;
    let request: RawOrderRequest = serde_json::from_str(&texts[0]).unwrap();

    let expected_price = if serde_json_keeps_number_text() {
        "65000.50"
    } else {
        "65000.5"
    };

    assert_eq!(request.params.label, client_order_id.as_str());
    assert_eq!(request.params.amount.get(), "10");
    assert_eq!(
        request.params.price.map(|price| price.get().to_string()),
        Some(expected_price.to_string())
    );

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_limit_submit_sends_or_rejects_unrepresentable_amount() {
    let (addr, state) = start_test_server().await.unwrap();
    let request_count = state.command_request_count.clone();
    let command_request_texts = state.command_request_texts.clone();
    let (mut client, _rx, cache) = create_test_execution_client(addr);
    add_test_account_to_cache(&cache, AccountId::from("DERIBIT-001"));
    client.connect().await.unwrap();

    let client_order_id = ClientOrderId::new("limit-submit-exact-test-002");
    let order_any = add_sized_limit_order_to_cache(
        &cache,
        client_order_id,
        Quantity::from("100000000.005000001"),
        Price::from("65000.5"),
    );

    let result = client.submit_order(submit_order_command(&order_any));
    if serde_json_keeps_number_text() {
        result.unwrap();
        wait_for_command_requests(&request_count, 1).await;

        let texts = command_request_texts.lock().await;
        let request: RawOrderRequest = serde_json::from_str(&texts[0]).unwrap();

        assert_eq!(request.params.amount.get(), "100000000.005000001");
    } else {
        assert_eq!(
            result.unwrap_err().to_string(),
            UNREPRESENTABLE_PARAMS_REASON
        );
        assert_eq!(request_count.load(Ordering::Relaxed), 0);
    }

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_modify_sends_exact_number_tokens() {
    let (addr, state) = start_test_server().await.unwrap();
    let request_count = state.command_request_count.clone();
    let command_request_texts = state.command_request_texts.clone();
    let (mut client, _rx, cache) = create_test_execution_client(addr);
    add_test_account_to_cache(&cache, AccountId::from("DERIBIT-001"));
    client.connect().await.unwrap();

    let client_order_id = ClientOrderId::new("modify-exact-test-001");
    add_limit_order_to_cache(&cache, client_order_id, TimeInForce::Gtc);

    client
        .modify_order(modify_order_command(client_order_id))
        .unwrap();

    wait_for_command_requests(&request_count, 1).await;

    let texts = command_request_texts.lock().await;
    let request: RawEditRequest = serde_json::from_str(&texts[0]).unwrap();

    assert_eq!(request.params.order_id, "DERIBIT-ORDER-1");
    assert_eq!(request.params.amount.get(), "2");
    assert_eq!(request.params.price.get(), "51000.0");

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_modify_sends_or_rejects_unrepresentable_amount() {
    let (addr, state) = start_test_server().await.unwrap();
    let request_count = state.command_request_count.clone();
    let command_request_texts = state.command_request_texts.clone();
    let (mut client, mut rx, cache) = create_test_execution_client(addr);
    add_test_account_to_cache(&cache, AccountId::from("DERIBIT-001"));
    client.connect().await.unwrap();

    let client_order_id = ClientOrderId::new("modify-exact-test-002");
    add_limit_order_to_cache(&cache, client_order_id, TimeInForce::Gtc);

    let result = client.modify_order(sized_modify_order_command(
        client_order_id,
        Quantity::from("100000000.005000001"),
    ));

    if serde_json_keeps_number_text() {
        result.unwrap();
        wait_for_command_requests(&request_count, 1).await;

        let texts = command_request_texts.lock().await;
        let request: RawEditRequest = serde_json::from_str(&texts[0]).unwrap();

        assert_eq!(request.params.order_id, "DERIBIT-ORDER-1");
        assert_eq!(request.params.amount.get(), "100000000.005000001");
        assert_eq!(request.params.price.get(), "51000.0");
    } else {
        assert_eq!(
            result.unwrap_err().to_string(),
            UNREPRESENTABLE_PARAMS_REASON
        );

        match recv_until(&mut rx, |event| matches!(event, ExecutionEvent::Order(_))).await {
            ExecutionEvent::Order(OrderEventAny::ModifyRejected(event)) => {
                assert_eq!(event.client_order_id, client_order_id);
                assert_eq!(
                    event.venue_order_id,
                    Some(VenueOrderId::from("DERIBIT-ORDER-1"))
                );
                assert_eq!(event.reason.as_str(), UNREPRESENTABLE_PARAMS_REASON);
            }
            other => panic!("Expected ModifyRejected event, was {other:?}"),
        }

        assert_eq!(request_count.load(Ordering::Relaxed), 0);
    }

    client.disconnect().await.unwrap();
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
            assert!(event.reason.contains("order_not_found"));
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
            assert!(event.reason.contains("price_not_changed"));
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
            assert!(event.reason.contains("venue_order_id required"));
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
            assert!(event.reason.contains("price required"));
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
            assert!(event.reason.contains("Order not found"));
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
    add_order_to_cache(cache, client_order_id, OrderType::Limit, time_in_force)
}

fn add_order_to_cache(
    cache: &Rc<RefCell<Cache>>,
    client_order_id: ClientOrderId,
    order_type: OrderType,
    time_in_force: TimeInForce,
) -> OrderAny {
    let mut builder = OrderTestBuilder::new(order_type);
    builder
        .trader_id(test_trader_id())
        .strategy_id(test_strategy_id())
        .instrument_id(test_instrument_id())
        .client_order_id(client_order_id)
        .side(OrderSide::Buy)
        .quantity(Quantity::from("1"))
        .time_in_force(time_in_force);

    match order_type {
        OrderType::Limit => {
            builder.price(Price::from("50000.0")).post_only(true);
        }
        OrderType::Market => {}
        OrderType::StopMarket => {
            builder
                .trigger_price(Price::from("49000.0"))
                .trigger_type(TriggerType::LastPrice);
        }
        OrderType::MarketIfTouched => {
            builder
                .trigger_price(Price::from("51000.0"))
                .trigger_type(TriggerType::LastPrice);
        }
        _ => panic!("Unsupported test order type {order_type:?}"),
    }

    let order = builder.build();
    cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .unwrap();
    order
}

fn add_sized_limit_order_to_cache(
    cache: &Rc<RefCell<Cache>>,
    client_order_id: ClientOrderId,
    quantity: Quantity,
    price: Price,
) -> OrderAny {
    let order = OrderTestBuilder::new(OrderType::Limit)
        .trader_id(test_trader_id())
        .strategy_id(test_strategy_id())
        .instrument_id(test_instrument_id())
        .client_order_id(client_order_id)
        .side(OrderSide::Buy)
        .quantity(quantity)
        .price(price)
        .build();
    cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .unwrap();
    order
}

#[derive(Deserialize)]
struct RawOrderRequest<'a> {
    #[serde(borrow)]
    params: RawOrderParams<'a>,
}

#[derive(Deserialize)]
struct RawOrderParams<'a> {
    label: &'a str,
    #[serde(borrow)]
    amount: &'a RawValue,
    #[serde(borrow)]
    price: Option<&'a RawValue>,
}

#[derive(Deserialize)]
struct RawEditRequest<'a> {
    #[serde(borrow)]
    params: RawEditParams<'a>,
}

#[derive(Deserialize)]
struct RawEditParams<'a> {
    order_id: &'a str,
    #[serde(borrow)]
    amount: &'a RawValue,
    #[serde(borrow)]
    price: &'a RawValue,
}

const UNREPRESENTABLE_PARAMS_REASON: &str = "Cannot serialize order params: exact decimal number serialization requires serde_json/arbitrary_precision";

// `serde_json/arbitrary_precision` keeps a number's text, including trailing zeros
fn serde_json_keeps_number_text() -> bool {
    serde_json::Number::from_str("0.10").unwrap().to_string() == "0.10"
}

fn find_command_request_params(requests: &[Value], client_order_id: ClientOrderId) -> &Value {
    &find_command_request(requests, client_order_id)["params"]
}

fn find_command_request(requests: &[Value], client_order_id: ClientOrderId) -> &Value {
    requests
        .iter()
        .find(|request| {
            request
                .get("params")
                .and_then(|params| params.get("label"))
                .and_then(Value::as_str)
                .is_some_and(|label| label == client_order_id.as_str())
        })
        .expect("order request should be captured")
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
    sized_modify_order_command(client_order_id, Quantity::from("2"))
}

fn sized_modify_order_command(client_order_id: ClientOrderId, quantity: Quantity) -> ModifyOrder {
    ModifyOrder::new(
        test_trader_id(),
        Some(*DERIBIT_CLIENT_ID),
        test_strategy_id(),
        test_instrument_id(),
        client_order_id,
        Some(VenueOrderId::from("DERIBIT-ORDER-1")),
        Some(quantity),
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
        None,
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

const UNREPRESENTABLE_FEE: &str = "79228162514264337593543950335";
const CORRECTED_FEE: &str = "0.00036801";

#[expect(clippy::too_many_arguments)]
fn user_trade(
    trade_id: &str,
    order_id: &str,
    instrument_name: &str,
    direction: &str,
    price: &str,
    amount: &str,
    fee: &str,
    liquidity: &str,
    label: &str,
) -> Value {
    json!({
        "trade_id": trade_id,
        "order_id": order_id,
        "instrument_name": instrument_name,
        "direction": direction,
        "price": price,
        "amount": amount,
        "fee": fee,
        "fee_currency": "BTC",
        "timestamp": 1_700_000_000_000_u64,
        "trade_seq": 1,
        "liquidity": liquidity,
        "order_type": "limit",
        "index_price": "50100",
        "mark_price": "50110",
        "tick_direction": 1,
        "state": "filled",
        "label": label,
        "reduce_only": false,
        "post_only": false,
        "profit_loss": "0.1",
    })
}

fn valid_btc_trade() -> Value {
    user_trade(
        "BTC-VALID",
        "BTC-ORDER-VALID",
        "BTC-PERPETUAL",
        "buy",
        "50000.0",
        "10",
        "0.00010000",
        "T",
        "O-VALID",
    )
}

fn bad_btc_trade(fee: &str) -> Value {
    user_trade(
        "BTC-BAD",
        "BTC-ORDER-BAD",
        "BTC-PERPETUAL",
        "sell",
        "50123.5",
        "30",
        fee,
        "M",
        "O-BAD",
    )
}

fn uncached_eth_trade() -> Value {
    user_trade(
        "ETH-BAD",
        "ETH-ORDER-BAD",
        "ETH-PERPETUAL",
        "buy",
        "2000.0",
        "1",
        UNREPRESENTABLE_FEE,
        "T",
        "O-ETH",
    )
}

fn trades_result(trades: &[Value]) -> Value {
    json!({"has_more": false, "trades": trades})
}

fn fill_command(
    instrument_id: Option<InstrumentId>,
    venue_order_id: Option<VenueOrderId>,
) -> GenerateFillReports {
    GenerateFillReports::new(
        UUID4::new(),
        UnixNanos::default(),
        instrument_id,
        venue_order_id,
        None,
        None,
        None,
        None,
    )
}

fn assert_commission_failure(error: &anyhow::Error) {
    let message = format!("{error:#}");
    assert!(
        message.contains("failed to parse trade BTC-BAD for BTC-PERPETUAL"),
        "error was: {message}"
    );
    assert!(
        message.contains("Overflow when scaling mantissa to fixed precision")
            || message.contains("exceeds MoneyRaw range"),
        "error was: {message}"
    );
}

fn assert_corrected_fill(report: &FillReport) {
    assert_eq!(
        report.instrument_id,
        InstrumentId::from("BTC-PERPETUAL.DERIBIT")
    );
    assert_eq!(report.venue_order_id, VenueOrderId::from("BTC-ORDER-BAD"));
    assert_eq!(report.trade_id, TradeId::from("BTC-BAD"));
    assert_eq!(report.client_order_id, Some(ClientOrderId::from("O-BAD")));
    assert_eq!(report.order_side, OrderSide::Sell);
    assert_eq!(report.last_qty, Quantity::from("30"));
    assert_eq!(report.last_px, Price::from("50123.5"));
    assert_eq!(report.commission, Money::from("0.00036801 BTC"));
    assert_eq!(report.commission.currency, Currency::BTC());
    assert_eq!(report.liquidity_side, LiquiditySide::Maker);
}

fn assert_valid_fill(report: &FillReport) {
    assert_eq!(
        report.instrument_id,
        InstrumentId::from("BTC-PERPETUAL.DERIBIT")
    );
    assert_eq!(report.venue_order_id, VenueOrderId::from("BTC-ORDER-VALID"));
    assert_eq!(report.trade_id, TradeId::from("BTC-VALID"));
    assert_eq!(report.client_order_id, Some(ClientOrderId::from("O-VALID")));
    assert_eq!(report.order_side, OrderSide::Buy);
    assert_eq!(report.last_qty, Quantity::from("10"));
    assert_eq!(report.last_px, Price::from("50000.0"));
    assert_eq!(report.commission, Money::from("0.00010000 BTC"));
    assert_eq!(report.commission.currency, Currency::BTC());
    assert_eq!(report.liquidity_side, LiquiditySide::Taker);
}

async fn cached_http_client(addr: SocketAddr) -> DeribitHttpClient {
    let client = DeribitHttpClient::new_with_env(
        Some("test_api_key".to_string()),
        Some("test_api_secret".to_string()),
        Some(format!("http://{addr}/api/v2")),
        DeribitEnvironment::Testnet,
        10,
        1,
        100,
        1000,
        None,
    )
    .expect("HTTP client");
    let instruments = client
        .request_instruments(DeribitCurrency::ANY, Some(DeribitProductType::Future))
        .await
        .expect("instruments");
    client.cache_instruments(&instruments);
    client
}

#[rstest]
#[tokio::test]
async fn test_request_fill_reports_fails_in_scope_historical_miss() {
    let (addr, state) = start_test_server().await.unwrap();
    let client = cached_http_client(addr).await;
    let account_id = AccountId::from("DERIBIT-001");
    let instrument_id = InstrumentId::from("BTC-PERPETUAL.DERIBIT");

    let scoped_empty = client
        .request_fill_reports(account_id, Some(instrument_id), None, None)
        .await
        .expect("empty instrument history");
    let unscoped_empty = client
        .request_fill_reports(account_id, None, None, None)
        .await
        .expect("empty currency history");

    assert!(scoped_empty.is_empty());
    assert!(unscoped_empty.is_empty());

    *state.user_trades_result.lock().await =
        trades_result(&[valid_btc_trade(), uncached_eth_trade()]);

    let unscoped = client
        .request_fill_reports(account_id, None, None, None)
        .await
        .expect_err("in-scope historical miss must not look complete");
    let scoped = client
        .request_fill_reports(account_id, Some(instrument_id), None, None)
        .await
        .expect("cached instrument request");
    let uncached = client
        .request_fill_reports(
            account_id,
            Some(InstrumentId::from("ETH-PERPETUAL.DERIBIT")),
            None,
            None,
        )
        .await
        .expect_err("targeted in-scope historical miss must fail");

    let unscoped_message = unscoped.to_string();
    assert!(
        unscoped_message.contains("unresolved historical instruments"),
        "error was: {unscoped_message}"
    );
    assert!(
        unscoped_message.contains("ETH-PERPETUAL.DERIBIT"),
        "error was: {unscoped_message}"
    );
    assert_eq!(scoped.len(), 1);
    assert_valid_fill(&scoped[0]);
    assert!(uncached.to_string().contains("ETH-PERPETUAL.DERIBIT"));
}

fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_millis() as u64
}

fn reconciliation_order(
    order_id: &str,
    instrument_name: &str,
    order_state: &str,
    ts_ms: u64,
) -> Value {
    json!({
        "label": order_id,
        "price": "50000.0",
        "amount": "10",
        "direction": "buy",
        "time_in_force": "good_til_cancelled",
        "instrument_name": instrument_name,
        "api": true,
        "order_id": order_id,
        "creation_timestamp": ts_ms,
        "filled_amount": 0.0,
        "last_update_timestamp": ts_ms,
        "post_only": false,
        "reduce_only": false,
        "average_price": 0.0,
        "order_state": order_state,
        "order_type": "limit",
    })
}

fn reconciliation_position(instrument_name: &str, size: i64) -> Value {
    let mut position = load_json("http_get_positions.json")["result"][0].clone();
    position["instrument_name"] = json!(instrument_name);
    position["size"] = json!(size);
    position
}

fn btc_scope() -> Vec<InstrumentId> {
    vec![InstrumentId::from("BTC-PERPETUAL.DERIBIT")]
}

async fn set_open_orders(state: &TestServerState, orders: &[Value]) {
    state.order_lookup_responses.lock().await.insert(
        "private/get_open_orders".to_string(),
        (StatusCode::OK, json!({"result": orders})),
    );
}

async fn connect_scoped_client(
    addr: SocketAddr,
    load_ids: Option<Vec<InstrumentId>>,
) -> DeribitExecutionClient {
    let mut config = create_test_exec_config(addr);
    config.load_ids = load_ids;
    let (mut client, _rx, cache) = create_test_execution_client_with_config(config);
    add_test_account_to_cache(&cache, AccountId::from("DERIBIT-001"));
    client.connect().await.expect("connect");
    client
}

fn order_reports_command(open_only: bool) -> GenerateOrderStatusReports {
    GenerateOrderStatusReports::new(
        UUID4::new(),
        UnixNanos::default(),
        open_only,
        None,
        None,
        None,
        None,
        None,
    )
}

#[rstest]
#[tokio::test]
async fn test_bulk_reconciliation_classifies_mixed_instrument_misses() {
    let (addr, state) = start_test_server().await.unwrap();
    let btc = InstrumentId::from("BTC-PERPETUAL.DERIBIT");
    let ts_ms = now_millis();
    set_open_orders(
        &state,
        &[
            reconciliation_order("BTC-OPEN", "BTC-PERPETUAL", "open", ts_ms),
            reconciliation_order("ETH-OPEN", "ETH-PERPETUAL", "open", ts_ms),
        ],
    )
    .await;
    *state.positions_result.lock().await = Some(json!([
        reconciliation_position("BTC-PERPETUAL", 50),
        reconciliation_position("ETH-PERPETUAL", 2),
    ]));
    let mut recent_eth = uncached_eth_trade();
    recent_eth["timestamp"] = json!(ts_ms);
    *state.user_trades_result.lock().await =
        trades_result(&[valid_btc_trade(), recent_eth.clone()]);

    let mut scoped = connect_scoped_client(addr, Some(btc_scope())).await;
    let orders = scoped
        .generate_order_status_reports(&order_reports_command(true))
        .await
        .expect("out-of-scope open order is omitted");
    let fills = scoped
        .generate_fill_reports(fill_command(None, None))
        .await
        .expect("out-of-scope fill is omitted");
    let positions = scoped
        .generate_position_status_reports(&GeneratePositionStatusReports::new(
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
            None,
            None,
            None,
        ))
        .await
        .expect("out-of-scope position is omitted");
    let mass_status = scoped
        .generate_mass_status(Some(60))
        .await
        .expect("complete scoped mass status")
        .expect("mass status");
    scoped.disconnect().await.unwrap();

    assert_eq!(orders.len(), 1);
    assert_eq!(orders[0].instrument_id, btc);
    assert_eq!(orders[0].venue_order_id, VenueOrderId::from("BTC-OPEN"));
    assert_eq!(fills.len(), 1);
    assert_valid_fill(&fills[0]);
    assert_eq!(positions.len(), 1);
    assert_eq!(positions[0].instrument_id, btc);
    assert!(mass_status.reports_complete());
    assert_eq!(mass_status.lookback_start(), None);

    let mut open_miss = connect_scoped_client(addr, None).await;
    let open_error = open_miss
        .generate_order_status_reports(&order_reports_command(true))
        .await
        .expect_err("in-scope open miss fails the request");
    let position_error = open_miss
        .generate_position_status_reports(&GeneratePositionStatusReports::new(
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
            None,
            None,
            None,
        ))
        .await
        .expect_err("in-scope position miss fails the request");
    let targeted = open_miss
        .generate_position_status_reports(&GeneratePositionStatusReports::new(
            UUID4::new(),
            UnixNanos::default(),
            Some(btc),
            None,
            None,
            None,
            None,
        ))
        .await
        .expect("unrelated position miss does not poison a targeted query");
    let fill_error = open_miss
        .generate_fill_reports(fill_command(None, None))
        .await
        .expect_err("in-scope historical fill miss fails a direct request");
    *state.positions_result.lock().await =
        Some(json!([reconciliation_position("BTC-PERPETUAL", 50)]));
    set_open_orders(&state, &[]).await;
    let mass_status = open_miss
        .generate_mass_status(Some(60))
        .await
        .expect("bounded historical miss returns the other reports")
        .expect("mass status");
    open_miss.disconnect().await.unwrap();

    let open_message = open_error.to_string();
    assert!(
        open_message
            .contains("Open order ETH-OPEN has unresolved instrument ETH-PERPETUAL.DERIBIT"),
        "error was: {open_message}"
    );
    assert!(
        position_error
            .to_string()
            .contains("Position has unresolved instrument ETH-PERPETUAL.DERIBIT")
    );
    assert_eq!(targeted.len(), 1);
    assert_eq!(targeted[0].instrument_id, btc);
    assert!(
        fill_error
            .to_string()
            .contains("unresolved historical instruments: ETH-PERPETUAL.DERIBIT"),
        "error was: {fill_error}"
    );
    assert!(!mass_status.reports_complete());
    assert!(mass_status.lookback_start().is_some());
    assert_eq!(
        mass_status
            .fill_reports()
            .get(&VenueOrderId::from("BTC-ORDER-VALID"))
            .map(Vec::len),
        Some(1)
    );
    assert!(
        mass_status
            .fill_reports()
            .get(&VenueOrderId::from("ETH-ORDER-BAD"))
            .is_none()
    );
}

#[rstest]
#[tokio::test]
async fn test_explicit_scope_omits_cached_out_of_scope_orders() {
    let (addr, state) = start_test_server().await.unwrap();
    let ts_ms = now_millis();
    set_open_orders(
        &state,
        &[
            reconciliation_order("BTC-OPEN", "BTC-PERPETUAL", "open", ts_ms),
            reconciliation_order("BTC-DATED", "BTC-27DEC24", "open", ts_ms),
        ],
    )
    .await;
    let mut client = connect_scoped_client(addr, Some(btc_scope())).await;

    let orders = client
        .generate_order_status_reports(&order_reports_command(true))
        .await
        .expect("cached out-of-scope order is omitted");
    client.disconnect().await.unwrap();

    assert_eq!(orders.len(), 1);
    assert_eq!(
        orders[0].instrument_id,
        InstrumentId::from("BTC-PERPETUAL.DERIBIT")
    );
    assert_eq!(orders[0].venue_order_id, VenueOrderId::from("BTC-OPEN"));
}

#[rstest]
#[tokio::test]
async fn test_flat_uncached_position_does_not_fail_reconciliation() {
    let (addr, state) = start_test_server().await.unwrap();
    *state.positions_result.lock().await =
        Some(json!([reconciliation_position("ETH-PERPETUAL", 0)]));
    let mut client = connect_scoped_client(addr, None).await;

    let reports = client
        .generate_position_status_reports(&GeneratePositionStatusReports::new(
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
            None,
            None,
            None,
        ))
        .await
        .expect("flat position is omitted before instrument resolution");
    client.disconnect().await.unwrap();

    assert!(reports.is_empty());
}

#[rstest]
#[tokio::test]
async fn test_incomplete_window_emits_flat_report_for_closed_cached_position() {
    let (addr, state) = start_test_server().await.unwrap();
    let ts_ms = now_millis();
    let mut recent_btc = valid_btc_trade();
    recent_btc["timestamp"] = json!(ts_ms);
    let mut recent_eth = uncached_eth_trade();
    recent_eth["timestamp"] = json!(ts_ms);
    *state.user_trades_result.lock().await = trades_result(&[recent_btc, recent_eth]);
    *state.positions_result.lock().await = Some(json!([]));
    let mut client = connect_scoped_client(addr, None).await;

    let mass_status = client
        .generate_mass_status(Some(60))
        .await
        .expect("bounded historical miss keeps the cached fill")
        .expect("mass status");
    client.disconnect().await.unwrap();

    let btc = InstrumentId::from("BTC-PERPETUAL.DERIBIT");
    let reports = mass_status
        .position_reports()
        .get(&btc)
        .cloned()
        .expect("closed cached instrument has an explicit flat report");
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].instrument_id, btc);
    assert_eq!(reports[0].quantity, Quantity::from("0"));
    assert!(!mass_status.reports_complete());
    assert!(mass_status.lookback_start().is_some());
}

#[rstest]
#[tokio::test]
async fn test_incomplete_window_does_not_flatten_spot() {
    let (addr, state) = start_test_server().await.unwrap();
    let ts_ms = now_millis();
    let mut spot = valid_btc_trade();
    spot["instrument_name"] = json!("BTC_USDC");
    spot["trade_id"] = json!("SPOT-1");
    spot["order_id"] = json!("SPOT-ORDER");
    spot["timestamp"] = json!(ts_ms);
    let mut recent_eth = uncached_eth_trade();
    recent_eth["timestamp"] = json!(ts_ms);
    *state.user_trades_result.lock().await = trades_result(&[spot, recent_eth]);
    *state.positions_result.lock().await = Some(json!([]));
    let mut config = create_test_exec_config(addr);
    config.product_types = vec![DeribitProductType::Future, DeribitProductType::Spot];
    let (mut client, _rx, cache) = create_test_execution_client_with_config(config);
    add_test_account_to_cache(&cache, AccountId::from("DERIBIT-001"));
    client.connect().await.expect("connect");

    let mass_status = client
        .generate_mass_status(Some(60))
        .await
        .expect("spot fill stays in the incomplete window")
        .expect("mass status");
    client.disconnect().await.unwrap();

    let spot_id = InstrumentId::from("BTC_USDC.DERIBIT");
    let spot_fills = mass_status
        .fill_reports()
        .values()
        .flatten()
        .filter(|fill| fill.instrument_id == spot_id)
        .count();
    assert_eq!(spot_fills, 1);
    assert!(mass_status.position_reports().get(&spot_id).is_none());
    assert!(!mass_status.reports_complete());
}

#[rstest]
#[tokio::test]
async fn test_historical_miss_outside_window_and_commission_stay_distinct() {
    let (addr, state) = start_test_server().await.unwrap();
    *state.user_trades_result.lock().await =
        trades_result(&[uncached_eth_trade(), bad_btc_trade(UNREPRESENTABLE_FEE)]);
    let mut client = connect_scoped_client(addr, None).await;

    let commission_error = client
        .generate_fill_reports(fill_command(None, None))
        .await
        .expect_err("commission failure still fails the request");
    let unbounded = client
        .generate_mass_status(None)
        .await
        .expect_err("commission failure is not incomplete history");
    client.disconnect().await.unwrap();

    assert_commission_failure(&commission_error);
    assert_commission_failure(&unbounded);

    *state.user_trades_result.lock().await = trades_result(&[uncached_eth_trade()]);
    let mut client = connect_scoped_client(addr, None).await;
    let mass_status = client
        .generate_mass_status(Some(60))
        .await
        .expect("old historical miss is outside the bounded window")
        .expect("mass status");
    let unbounded = client
        .generate_mass_status(None)
        .await
        .expect("unbounded historical miss stays a warning")
        .expect("mass status");
    client.disconnect().await.unwrap();

    assert!(mass_status.reports_complete());
    assert_eq!(mass_status.lookback_start(), None);
    assert!(unbounded.reports_complete());
    assert_eq!(unbounded.lookback_start(), None);
}

#[rstest]
#[tokio::test]
async fn test_expired_order_miss_is_historical() {
    let (addr, state) = start_test_server().await.unwrap();
    let client = cached_http_client(addr).await;
    state.order_lookup_responses.lock().await.insert(
        "private/get_order_history_by_currency".to_string(),
        (
            StatusCode::OK,
            json!({"result": [reconciliation_order(
                "ETH-EXPIRED",
                "ETH-PERPETUAL",
                "expired",
                now_millis(),
            )]}),
        ),
    );

    let error = client
        .request_order_status_reports(AccountId::from("DERIBIT-001"), None, None, None, false)
        .await
        .expect_err("expired miss is historical");

    let message = error.to_string();
    assert!(
        message.contains("unresolved historical instruments: ETH-PERPETUAL.DERIBIT"),
        "error was: {message}"
    );
    assert!(!message.contains("Open order"));
}

#[rstest]
#[tokio::test]
async fn test_connect_loads_required_instruments_before_reports() {
    let (addr, state) = start_test_server().await.unwrap();
    let mut client = connect_scoped_client(
        addr,
        Some(vec![InstrumentId::from("ETH-PERPETUAL.DERIBIT")]),
    )
    .await;
    let loaded = state.instrument_requests.load(Ordering::Relaxed);

    let _ = client
        .generate_position_status_reports(&GeneratePositionStatusReports::new(
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
            None,
            None,
            None,
        ))
        .await;
    client.disconnect().await.unwrap();

    assert_eq!(loaded, 1);
    assert_eq!(state.instrument_requests.load(Ordering::Relaxed), loaded);
}

#[rstest]
#[tokio::test]
async fn test_commission_failure_fails_direct_targeted_and_mass_status_then_recovers() {
    let (addr, state) = start_test_server().await.unwrap();
    *state.user_trades_result.lock().await =
        trades_result(&[valid_btc_trade(), bad_btc_trade(UNREPRESENTABLE_FEE)]);
    let (mut client, mut rx, cache) = create_test_execution_client(addr);
    add_test_account_to_cache(&cache, AccountId::from("DERIBIT-001"));
    client.connect().await.expect("connect");
    wait_until_async(
        || async { state.authenticated.load(Ordering::Relaxed) },
        Duration::from_secs(10),
    )
    .await;

    while rx.try_recv().is_ok() {}

    let instrument_id = InstrumentId::from("BTC-PERPETUAL.DERIBIT");
    let bad_order = VenueOrderId::from("BTC-ORDER-BAD");
    let direct_error = client
        .generate_fill_reports(fill_command(Some(instrument_id), None))
        .await
        .expect_err("direct fill request must fail as a unit");
    let targeted_other = client
        .generate_fill_reports(fill_command(
            Some(instrument_id),
            Some(VenueOrderId::from("BTC-ORDER-VALID")),
        ))
        .await
        .expect("an unrelated commission failure must not fail a targeted request");
    let targeted_error = client
        .generate_fill_reports(fill_command(Some(instrument_id), Some(bad_order)))
        .await
        .expect_err("targeted fill request must not omit the failed fill");
    let mass_error = client
        .generate_mass_status(None)
        .await
        .expect_err("mass status must not omit the failed fill");

    assert_eq!(targeted_other.len(), 1);
    assert_valid_fill(&targeted_other[0]);

    for error in [direct_error, targeted_error, mass_error] {
        assert_commission_failure(&error);
    }

    while let Ok(event) = rx.try_recv() {
        let is_fill = matches!(
            &event,
            ExecutionEvent::Report(ExecutionReport::Fill(_))
                | ExecutionEvent::Order(OrderEventAny::Filled(_))
        );
        assert!(
            !is_fill,
            "commission failure must not emit a fallback fill, was {event:?}"
        );
    }

    *state.user_trades_result.lock().await =
        trades_result(&[valid_btc_trade(), bad_btc_trade(CORRECTED_FEE)]);
    let recovered = client
        .generate_fill_reports(fill_command(Some(instrument_id), None))
        .await
        .expect("corrected direct request");
    let targeted = client
        .generate_fill_reports(fill_command(Some(instrument_id), Some(bad_order)))
        .await
        .expect("corrected targeted request");
    let mass_status = client
        .generate_mass_status(None)
        .await
        .expect("corrected mass status")
        .expect("mass status present");

    assert_eq!(recovered.len(), 2);
    assert_valid_fill(
        recovered
            .iter()
            .find(|report| report.trade_id.as_str() == "BTC-VALID")
            .unwrap(),
    );
    assert_corrected_fill(
        recovered
            .iter()
            .find(|report| report.trade_id.as_str() == "BTC-BAD")
            .unwrap(),
    );
    assert_eq!(targeted.len(), 1);
    assert_corrected_fill(&targeted[0]);
    let fills = mass_status.fill_reports();
    assert_eq!(fills.get(&bad_order).expect("corrected mass fill").len(), 1);
    assert_corrected_fill(&fills[&bad_order][0]);
    assert_eq!(
        fills
            .get(&VenueOrderId::from("BTC-ORDER-VALID"))
            .expect("valid mass fill")
            .len(),
        1
    );

    client.disconnect().await.expect("disconnect");
}
