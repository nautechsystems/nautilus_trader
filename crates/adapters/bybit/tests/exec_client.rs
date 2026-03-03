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

//! Integration tests for `BybitExecutionClient`.
//!
//! These tests verify execution client operations including connection,
//! order submission, cancellation, and event handling.

use std::{
    cell::RefCell,
    collections::HashMap,
    net::SocketAddr,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};

use axum::{
    Router,
    body::Bytes,
    extract::{
        Query, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Json, Response},
    routing::{get, post},
};
use nautilus_bybit::{
    common::enums::{BybitEnvironment, BybitProductType},
    config::BybitExecClientConfig,
    execution::BybitExecutionClient,
};
use nautilus_common::{
    cache::Cache, clients::ExecutionClient, live::runner::set_exec_event_sender,
    messages::ExecutionEvent, testing::wait_until_async,
};
use nautilus_core::{UUID4, UnixNanos};
use nautilus_live::ExecutionClientCore;
use nautilus_model::{
    accounts::{AccountAny, MarginAccount},
    enums::{AccountType, OmsType},
    events::AccountState,
    identifiers::{AccountId, ClientId, TraderId, Venue},
    types::{AccountBalance, Money},
};
use nautilus_network::http::HttpClient;
use rstest::rstest;
use serde_json::{Value, json};

#[derive(Clone)]
struct TestServerState {
    ws_connection_count: Arc<tokio::sync::Mutex<usize>>,
    private_ws_connections: Arc<AtomicUsize>,
    trade_ws_connections: Arc<AtomicUsize>,
    authenticated: Arc<AtomicBool>,
    subscriptions: Arc<tokio::sync::Mutex<Vec<String>>>,
    disconnect_trigger: Arc<AtomicBool>,
    ping_count: Arc<AtomicUsize>,
}

impl Default for TestServerState {
    fn default() -> Self {
        Self {
            ws_connection_count: Arc::new(tokio::sync::Mutex::new(0)),
            private_ws_connections: Arc::new(AtomicUsize::new(0)),
            trade_ws_connections: Arc::new(AtomicUsize::new(0)),
            authenticated: Arc::new(AtomicBool::new(false)),
            subscriptions: Arc::new(tokio::sync::Mutex::new(Vec::new())),
            disconnect_trigger: Arc::new(AtomicBool::new(false)),
            ping_count: Arc::new(AtomicUsize::new(0)),
        }
    }
}

fn load_test_data(filename: &str) -> Value {
    let path = format!("test_data/{filename}");
    let content = std::fs::read_to_string(path).expect("Failed to read test data");
    serde_json::from_str(&content).expect("Failed to parse test data")
}

fn has_auth_headers(headers: &HeaderMap) -> bool {
    headers.contains_key("x-bapi-api-key")
        && headers.contains_key("x-bapi-sign")
        && headers.contains_key("x-bapi-timestamp")
}

async fn handle_get_instruments(query: Query<HashMap<String, String>>) -> impl IntoResponse {
    let category = query.get("category").map(String::as_str);
    let filename = match category {
        Some("linear") => "http_get_instruments_linear.json",
        Some("spot") => "http_get_instruments_spot.json",
        Some("inverse") => "http_get_instruments_inverse.json",
        Some("option") => "http_get_instruments_option.json",
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "retCode": 10001,
                    "retMsg": "Invalid category",
                    "result": {},
                    "time": 1704470400123i64
                })),
            )
                .into_response();
        }
    };

    let instruments = load_test_data(filename);
    Json(instruments).into_response()
}

async fn handle_get_fee_rate(headers: HeaderMap) -> impl IntoResponse {
    if !has_auth_headers(&headers) {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({
                "retCode": 10003,
                "retMsg": "Invalid API key",
                "result": {},
                "time": 1704470400123i64
            })),
        )
            .into_response();
    }
    let fee_rate = load_test_data("http_get_fee_rate.json");
    Json(fee_rate).into_response()
}

async fn handle_get_wallet_balance(headers: HeaderMap) -> impl IntoResponse {
    if !has_auth_headers(&headers) {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({
                "retCode": 10003,
                "retMsg": "Invalid API key",
                "result": {},
                "time": 1704470400123i64
            })),
        )
            .into_response();
    }
    let wallet = load_test_data("http_get_wallet_balance.json");
    Json(wallet).into_response()
}

async fn handle_get_positions(headers: HeaderMap) -> impl IntoResponse {
    if !has_auth_headers(&headers) {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({
                "retCode": 10003,
                "retMsg": "Invalid API key",
                "result": {},
                "time": 1704470400123i64
            })),
        )
            .into_response();
    }
    let positions = load_test_data("http_get_positions.json");
    Json(positions).into_response()
}

async fn handle_get_orders_realtime(headers: HeaderMap) -> impl IntoResponse {
    if !has_auth_headers(&headers) {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({
                "retCode": 10003,
                "retMsg": "Invalid API key",
                "result": {},
                "time": 1704470400123i64
            })),
        )
            .into_response();
    }
    let orders = load_test_data("http_get_orders_realtime.json");
    Json(orders).into_response()
}

async fn handle_post_order(headers: HeaderMap, body: Bytes) -> impl IntoResponse {
    if !has_auth_headers(&headers) {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({
                "retCode": 10003,
                "retMsg": "Invalid API key",
                "result": {},
                "time": 1704470400123i64
            })),
        )
            .into_response();
    }

    let Ok(order_req): Result<Value, _> = serde_json::from_slice(&body) else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "retCode": 10001,
                "retMsg": "Invalid JSON body",
                "result": {},
                "time": 1704470400123i64
            })),
        )
            .into_response();
    };

    Json(json!({
        "retCode": 0,
        "retMsg": "OK",
        "result": {
            "orderId": "test-order-id-12345",
            "orderLinkId": order_req.get("orderLinkId").and_then(|v| v.as_str()).unwrap_or("")
        },
        "retExtInfo": {},
        "time": 1704470400123i64
    }))
    .into_response()
}

async fn handle_cancel_order(headers: HeaderMap, _body: Bytes) -> impl IntoResponse {
    if !has_auth_headers(&headers) {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({
                "retCode": 10003,
                "retMsg": "Invalid API key",
                "result": {},
                "time": 1704470400123i64
            })),
        )
            .into_response();
    }

    Json(json!({
        "retCode": 0,
        "retMsg": "OK",
        "result": {
            "orderId": "test-order-id-12345",
            "orderLinkId": "test-order"
        },
        "retExtInfo": {},
        "time": 1704470400123i64
    }))
    .into_response()
}

async fn handle_server_time() -> impl IntoResponse {
    Json(json!({
        "retCode": 0,
        "retMsg": "OK",
        "result": {
            "timeSecond": "1704470400",
            "timeNano": "1704470400123456789"
        },
        "retExtInfo": {},
        "time": 1704470400123i64
    }))
}

async fn handle_private_websocket(
    ws: WebSocketUpgrade,
    State(state): State<TestServerState>,
) -> Response {
    state.private_ws_connections.fetch_add(1, Ordering::Relaxed);
    ws.on_upgrade(|socket| handle_socket(socket, state))
}

async fn handle_trade_websocket(
    ws: WebSocketUpgrade,
    State(state): State<TestServerState>,
) -> Response {
    state.trade_ws_connections.fetch_add(1, Ordering::Relaxed);
    ws.on_upgrade(|socket| handle_socket(socket, state))
}

async fn handle_socket(mut socket: WebSocket, state: TestServerState) {
    {
        let mut count = state.ws_connection_count.lock().await;
        *count += 1;
    }

    loop {
        if state.disconnect_trigger.load(Ordering::Relaxed) {
            break;
        }

        let msg_opt = match tokio::time::timeout(Duration::from_millis(50), socket.recv()).await {
            Ok(opt) => opt,
            Err(_) => continue,
        };

        let Some(msg) = msg_opt else {
            break;
        };

        let msg = match msg {
            Ok(m) => m,
            Err(_) => break,
        };

        match msg {
            Message::Text(text) => {
                let Ok(value) = serde_json::from_str::<Value>(&text) else {
                    continue;
                };

                let op = value.get("op").and_then(|v| v.as_str());

                match op {
                    Some("ping") => {
                        state.ping_count.fetch_add(1, Ordering::Relaxed);
                        let pong_response = json!({
                            "success": true,
                            "ret_msg": "pong",
                            "conn_id": "test-conn-id",
                            "req_id": value.get("req_id").and_then(|v| v.as_str()).unwrap_or(""),
                            "op": "pong"
                        });

                        if socket
                            .send(Message::Text(pong_response.to_string().into()))
                            .await
                            .is_err()
                        {
                            break;
                        }
                    }
                    Some("auth") => {
                        let api_key = value
                            .get("args")
                            .and_then(|a| a.as_array())
                            .and_then(|arr| arr.first())
                            .and_then(|v| v.as_str());

                        if api_key == Some("test_api_key") {
                            state.authenticated.store(true, Ordering::Relaxed);
                            let auth_response = json!({
                                "success": true,
                                "ret_msg": "",
                                "op": "auth",
                                "conn_id": "test-conn-id"
                            });

                            if socket
                                .send(Message::Text(auth_response.to_string().into()))
                                .await
                                .is_err()
                            {
                                break;
                            }
                        } else {
                            let auth_response = json!({
                                "success": false,
                                "ret_msg": "Invalid API key",
                                "op": "auth",
                                "conn_id": "test-conn-id"
                            });

                            if socket
                                .send(Message::Text(auth_response.to_string().into()))
                                .await
                                .is_err()
                            {
                                break;
                            }
                        }
                    }
                    Some("subscribe") => {
                        let args = value.get("args").and_then(|a| a.as_array());
                        if let Some(topics) = args {
                            for topic in topics {
                                if let Some(topic_str) = topic.as_str() {
                                    let mut subs = state.subscriptions.lock().await;
                                    if !subs.contains(&topic_str.to_string()) {
                                        subs.push(topic_str.to_string());
                                    }
                                }
                            }
                        }

                        let sub_response = json!({
                            "success": true,
                            "ret_msg": "",
                            "conn_id": "test-conn-id",
                            "req_id": value.get("req_id").and_then(|v| v.as_str()).unwrap_or(""),
                            "op": "subscribe"
                        });

                        if socket
                            .send(Message::Text(sub_response.to_string().into()))
                            .await
                            .is_err()
                        {
                            break;
                        }
                    }
                    Some("order.place" | "order.amend" | "order.cancel") => {
                        let req_id = value.get("req_id").and_then(|v| v.as_str());
                        let response = json!({
                            "success": true,
                            "ret_msg": "",
                            "conn_id": "test-conn-id",
                            "req_id": req_id.unwrap_or(""),
                            "op": op.unwrap()
                        });

                        if socket
                            .send(Message::Text(response.to_string().into()))
                            .await
                            .is_err()
                        {
                            break;
                        }
                    }
                    _ => {}
                }
            }
            Message::Ping(_) => {
                state.ping_count.fetch_add(1, Ordering::Relaxed);

                if socket.send(Message::Pong(vec![].into())).await.is_err() {
                    break;
                }
            }
            Message::Close(_) => {
                break;
            }
            _ => {}
        }
    }

    let mut count = state.ws_connection_count.lock().await;
    *count = count.saturating_sub(1);
}

fn create_test_router(state: TestServerState) -> Router {
    Router::new()
        .route("/v5/market/instruments-info", get(handle_get_instruments))
        .route("/v5/account/fee-rate", get(handle_get_fee_rate))
        .route("/v5/account/wallet-balance", get(handle_get_wallet_balance))
        .route("/v5/position/list", get(handle_get_positions))
        .route("/v5/order/realtime", get(handle_get_orders_realtime))
        .route("/v5/order/create", post(handle_post_order))
        .route("/v5/order/cancel", post(handle_cancel_order))
        .route("/v3/public/time", get(handle_server_time))
        .route("/v5/private", get(handle_private_websocket))
        .route("/v5/trade", get(handle_trade_websocket))
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

    let health_url = format!("http://{addr}/v3/public/time");
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

fn create_test_exec_config(addr: SocketAddr) -> BybitExecClientConfig {
    BybitExecClientConfig {
        api_key: Some("test_api_key".to_string()),
        api_secret: Some("test_api_secret".to_string()),
        product_types: vec![BybitProductType::Linear],
        environment: BybitEnvironment::Mainnet,
        base_url_http: Some(format!("http://{addr}")),
        base_url_ws_private: Some(format!("ws://{addr}/v5/private")),
        base_url_ws_trade: Some(format!("ws://{addr}/v5/trade")),
        http_proxy_url: None,
        ws_proxy_url: None,
        http_timeout_secs: Some(10),
        max_retries: Some(1),
        retry_delay_initial_ms: Some(100),
        retry_delay_max_ms: Some(1000),
        heartbeat_interval_secs: Some(5),
        recv_window_ms: Some(5000),
        account_id: None,
        use_spot_position_reports: false,
        futures_leverages: None,
        position_mode: None,
        margin_mode: None,
    }
}

fn create_test_execution_client(
    addr: SocketAddr,
) -> (
    BybitExecutionClient,
    tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
    Rc<RefCell<Cache>>,
) {
    let trader_id = TraderId::from("TESTER-001");
    let account_id = AccountId::from("BYBIT-001");
    let client_id = ClientId::from("BYBIT");

    let cache = Rc::new(RefCell::new(Cache::default()));

    let core = ExecutionClientCore::new(
        trader_id,
        client_id,
        Venue::from("BYBIT"),
        OmsType::Netting,
        account_id,
        AccountType::Margin,
        None,
        cache.clone(),
    );

    let config = create_test_exec_config(addr);

    // Event channel must be set before creating client due to thread-local storage
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    set_exec_event_sender(tx);

    let client = BybitExecutionClient::new(core, config).unwrap();

    (client, rx, cache)
}

fn add_test_account_to_cache(cache: &Rc<RefCell<Cache>>, account_id: AccountId) {
    let account_state = AccountState::new(
        account_id,
        AccountType::Margin,
        vec![AccountBalance::new(
            Money::from("10000.0 USDT"),
            Money::from("0 USDT"),
            Money::from("10000.0 USDT"),
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

    assert_eq!(client.client_id(), ClientId::from("BYBIT"));
    assert_eq!(client.venue(), Venue::from("BYBIT"));
    assert_eq!(client.oms_type(), OmsType::Netting);
    assert!(!client.is_connected());
}

#[rstest]
#[tokio::test]
async fn test_exec_client_connect_disconnect() {
    let (addr, state) = start_test_server().await.unwrap();
    let (mut client, _rx, cache) = create_test_execution_client(addr);
    add_test_account_to_cache(&cache, AccountId::from("BYBIT-001"));

    client.connect().await.unwrap();

    wait_until_async(
        || async { *state.ws_connection_count.lock().await >= 2 },
        Duration::from_secs(10),
    )
    .await;
    wait_until_async(
        || async { state.subscriptions.lock().await.len() >= 4 },
        Duration::from_secs(10),
    )
    .await;

    assert!(client.is_connected());
    assert!(state.authenticated.load(Ordering::Relaxed));

    let subs = state.subscriptions.lock().await;
    assert!(subs.contains(&"order".to_string()));
    assert!(subs.contains(&"execution".to_string()));
    assert!(subs.contains(&"position".to_string()));
    assert!(subs.contains(&"wallet".to_string()));
    drop(subs);

    client.disconnect().await.unwrap();
    assert!(!client.is_connected());
}

#[rstest]
#[tokio::test]
async fn test_exec_client_demo_mode_skips_trade_ws() {
    let (addr, state) = start_test_server().await.unwrap();
    let trader_id = TraderId::from("TESTER-001");
    let account_id = AccountId::from("BYBIT-001");
    let client_id = ClientId::from("BYBIT");

    let cache = Rc::new(RefCell::new(Cache::default()));
    add_test_account_to_cache(&cache, account_id);

    let core = ExecutionClientCore::new(
        trader_id,
        client_id,
        Venue::from("BYBIT"),
        OmsType::Netting,
        account_id,
        AccountType::Margin,
        None,
        cache,
    );

    let config = BybitExecClientConfig {
        api_key: Some("test_api_key".to_string()),
        api_secret: Some("test_api_secret".to_string()),
        product_types: vec![BybitProductType::Linear],
        environment: BybitEnvironment::Demo,
        base_url_http: Some(format!("http://{addr}")),
        base_url_ws_private: Some(format!("ws://{addr}/v5/private")),
        base_url_ws_trade: Some(format!("ws://{addr}/v5/trade")),
        http_proxy_url: None,
        ws_proxy_url: None,
        http_timeout_secs: Some(10),
        max_retries: Some(1),
        retry_delay_initial_ms: Some(100),
        retry_delay_max_ms: Some(1000),
        heartbeat_interval_secs: Some(5),
        recv_window_ms: Some(5000),
        account_id: None,
        use_spot_position_reports: false,
        futures_leverages: None,
        position_mode: None,
        margin_mode: None,
    };

    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    set_exec_event_sender(tx);

    let mut client = BybitExecutionClient::new(core, config).unwrap();
    client.connect().await.unwrap();

    // Wait for subscriptions to confirm connection phase is complete
    wait_until_async(
        || async { state.private_ws_connections.load(Ordering::Relaxed) >= 1 },
        Duration::from_secs(10),
    )
    .await;
    wait_until_async(
        || async { state.subscriptions.lock().await.len() >= 4 },
        Duration::from_secs(10),
    )
    .await;

    let private_count = state.private_ws_connections.load(Ordering::Relaxed);
    let trade_count = state.trade_ws_connections.load(Ordering::Relaxed);
    assert_eq!(private_count, 1, "Demo mode should connect to private WS");
    assert_eq!(trade_count, 0, "Demo mode should NOT connect to trade WS");

    assert!(client.is_connected());
    client.disconnect().await.unwrap();
}
