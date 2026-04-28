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
    common::enums::{BybitEnvironment, BybitMarginMode, BybitPositionMode, BybitProductType},
    config::BybitExecClientConfig,
    execution::BybitExecutionClient,
};
use nautilus_common::{
    cache::Cache,
    clients::ExecutionClient,
    live::runner::set_exec_event_sender,
    messages::{ExecutionEvent, execution::ExecutionReport},
    testing::wait_until_async,
};
use nautilus_core::{UUID4, UnixNanos};
use nautilus_live::ExecutionClientCore;
use nautilus_model::{
    accounts::{AccountAny, MarginAccount},
    enums::{AccountType, OmsType, OrderSide, TimeInForce, TrailingOffsetType, TriggerType},
    events::AccountState,
    identifiers::{
        AccountId, ClientId, ClientOrderId, InstrumentId, OrderListId, StrategyId, Symbol,
        TraderId, Venue,
    },
    orders::{MarketOrder, OrderAny, TrailingStopMarketOrder},
    types::{AccountBalance, Money, Price, Quantity},
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
    switch_mode_requests: Arc<tokio::sync::Mutex<Vec<Value>>>,
    set_leverage_requests: Arc<tokio::sync::Mutex<Vec<Value>>>,
    set_margin_mode_requests: Arc<tokio::sync::Mutex<Vec<Value>>>,
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
            switch_mode_requests: Arc::new(tokio::sync::Mutex::new(Vec::new())),
            set_leverage_requests: Arc::new(tokio::sync::Mutex::new(Vec::new())),
            set_margin_mode_requests: Arc::new(tokio::sync::Mutex::new(Vec::new())),
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

async fn handle_switch_mode(
    State(state): State<TestServerState>,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    if !has_auth_headers(&headers) {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({"retCode": 10003, "retMsg": "Invalid API key", "result": {}})),
        )
            .into_response();
    }

    if let Ok(value) = serde_json::from_slice::<Value>(&body) {
        state.switch_mode_requests.lock().await.push(value);
    }

    Json(json!({
        "retCode": 0,
        "retMsg": "OK",
        "result": {},
        "retExtInfo": {},
        "time": 1704470400123i64,
    }))
    .into_response()
}

async fn handle_set_leverage(
    State(state): State<TestServerState>,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    if !has_auth_headers(&headers) {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({"retCode": 10003, "retMsg": "Invalid API key", "result": {}})),
        )
            .into_response();
    }

    if let Ok(value) = serde_json::from_slice::<Value>(&body) {
        state.set_leverage_requests.lock().await.push(value);
    }

    Json(json!({
        "retCode": 0,
        "retMsg": "OK",
        "result": {},
        "retExtInfo": {},
        "time": 1704470400123i64,
    }))
    .into_response()
}

async fn handle_set_margin_mode(
    State(state): State<TestServerState>,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    if !has_auth_headers(&headers) {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({"retCode": 10003, "retMsg": "Invalid API key", "result": {}})),
        )
            .into_response();
    }

    if let Ok(value) = serde_json::from_slice::<Value>(&body) {
        state.set_margin_mode_requests.lock().await.push(value);
    }

    Json(json!({
        "retCode": 0,
        "retMsg": "OK",
        "result": {},
        "retExtInfo": {},
        "time": 1704470400123i64,
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
        .route("/v5/position/switch-mode", post(handle_switch_mode))
        .route("/v5/position/set-leverage", post(handle_set_leverage))
        .route("/v5/account/set-margin-mode", post(handle_set_margin_mode))
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
        proxy_url: None,
        http_timeout_secs: 10,
        max_retries: 1,
        retry_delay_initial_ms: 100,
        retry_delay_max_ms: 1000,
        heartbeat_interval_secs: 5,
        recv_window_ms: 5000,
        account_id: None,
        use_spot_position_reports: false,
        futures_leverages: None,
        position_mode: None,
        margin_mode: None,
        transport_backend: Default::default(),
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
async fn test_exec_client_connect_applies_position_mode_for_derivative_symbols() {
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

    let mut position_mode = std::collections::HashMap::new();
    position_mode.insert("ETHUSDT-LINEAR".to_string(), BybitPositionMode::BothSides);
    position_mode.insert(
        "BTCUSD-INVERSE".to_string(),
        BybitPositionMode::MergedSingle,
    );
    // Spot symbol must be filtered out (Bybit rejects switch-mode on Spot).
    position_mode.insert("BTCUSDT-SPOT".to_string(), BybitPositionMode::MergedSingle);

    let mut config = create_test_exec_config(addr);
    config.position_mode = Some(position_mode);

    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    set_exec_event_sender(tx);

    let mut client = BybitExecutionClient::new(core, config).unwrap();

    client.connect().await.unwrap();

    wait_until_async(
        || async { state.switch_mode_requests.lock().await.len() >= 2 },
        Duration::from_secs(10),
    )
    .await;

    let requests = state.switch_mode_requests.lock().await;
    assert_eq!(
        requests.len(),
        2,
        "switch-mode should be called for Linear+Inverse only, not Spot",
    );

    let symbols: Vec<&str> = requests
        .iter()
        .filter_map(|r| r.get("symbol").and_then(|v| v.as_str()))
        .collect();
    assert!(symbols.contains(&"ETHUSDT"));
    assert!(symbols.contains(&"BTCUSD"));

    let categories: Vec<&str> = requests
        .iter()
        .filter_map(|r| r.get("category").and_then(|v| v.as_str()))
        .collect();
    assert!(categories.contains(&"linear"));
    assert!(categories.contains(&"inverse"));

    drop(requests);
    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_exec_client_connect_applies_leverage_and_margin_mode() {
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

    let mut leverages = std::collections::HashMap::new();
    leverages.insert("ETHUSDT-LINEAR".to_string(), 5);

    let mut config = create_test_exec_config(addr);
    config.futures_leverages = Some(leverages);
    config.margin_mode = Some(BybitMarginMode::RegularMargin);

    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    set_exec_event_sender(tx);

    let mut client = BybitExecutionClient::new(core, config).unwrap();

    client.connect().await.unwrap();

    wait_until_async(
        || async {
            !state.set_leverage_requests.lock().await.is_empty()
                && !state.set_margin_mode_requests.lock().await.is_empty()
        },
        Duration::from_secs(10),
    )
    .await;

    let leverage_reqs = state.set_leverage_requests.lock().await;
    assert_eq!(leverage_reqs.len(), 1);
    assert_eq!(
        leverage_reqs[0].get("symbol").and_then(|v| v.as_str()),
        Some("ETHUSDT"),
    );
    assert_eq!(
        leverage_reqs[0].get("buyLeverage").and_then(|v| v.as_str()),
        Some("5"),
    );
    drop(leverage_reqs);

    let margin_reqs = state.set_margin_mode_requests.lock().await;
    assert_eq!(margin_reqs.len(), 1);
    assert_eq!(
        margin_reqs[0].get("setMarginMode").and_then(|v| v.as_str()),
        Some("REGULAR_MARGIN"),
    );
    drop(margin_reqs);

    client.disconnect().await.unwrap();
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
        proxy_url: None,
        http_timeout_secs: 10,
        max_retries: 1,
        retry_delay_initial_ms: 100,
        retry_delay_max_ms: 1000,
        heartbeat_interval_secs: 5,
        recv_window_ms: 5000,
        account_id: None,
        use_spot_position_reports: false,
        futures_leverages: None,
        position_mode: None,
        margin_mode: None,
        transport_backend: Default::default(),
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

#[rstest]
#[tokio::test]
async fn test_exec_client_query_order() {
    use nautilus_common::messages::execution::QueryOrder;

    let (addr, _state) = start_test_server().await.unwrap();
    let (mut client, mut rx, cache) = create_test_execution_client(addr);
    add_test_account_to_cache(&cache, AccountId::from("BYBIT-001"));

    client.connect().await.unwrap();
    client.start().unwrap();

    wait_until_async(|| async { client.is_connected() }, Duration::from_secs(10)).await;

    // Drain connection events (account state, subscriptions)
    while tokio::time::timeout(Duration::from_millis(200), rx.recv())
        .await
        .is_ok()
    {}

    let cmd = QueryOrder::new(
        TraderId::from("TESTER-001"),
        Some(ClientId::from("BYBIT")),
        StrategyId::from("S-001"),
        InstrumentId::new(Symbol::from("ETHUSDT-LINEAR"), Venue::from("BYBIT")),
        ClientOrderId::from("client-open-1"),
        None,
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
            assert_eq!(
                report.client_order_id,
                Some(ClientOrderId::from("client-open-1")),
            );
        }
        other => panic!("Expected OrderStatusReport, was {other:?}"),
    }

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_query_account_does_not_block_within_runtime() {
    use nautilus_common::messages::execution::QueryAccount;

    let (addr, _state) = start_test_server().await.unwrap();
    let (mut client, mut rx, cache) = create_test_execution_client(addr);
    add_test_account_to_cache(&cache, AccountId::from("BYBIT-001"));

    client.connect().await.unwrap();
    client.start().unwrap();

    wait_until_async(|| async { client.is_connected() }, Duration::from_secs(10)).await;

    // Drain connection events (account state, subscriptions)
    while tokio::time::timeout(Duration::from_millis(200), rx.recv())
        .await
        .is_ok()
    {}

    let cmd = QueryAccount::new(
        TraderId::from("TESTER-001"),
        Some(ClientId::from("BYBIT")),
        AccountId::from("BYBIT-001"),
        UUID4::new(),
        UnixNanos::default(),
        None,
    );

    client.query_account(cmd).unwrap();

    let event = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("timed out waiting for query_account event")
        .expect("channel closed");

    assert!(
        matches!(event, ExecutionEvent::Account(_)),
        "Expected ExecutionEvent::Account, was {event:?}"
    );

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_exec_client_submit_order_list_demo() {
    use nautilus_common::messages::execution::SubmitOrderList;
    use nautilus_model::orders::OrderList;

    let (addr, state) = start_test_server().await.unwrap();
    let trader_id = TraderId::from("TESTER-001");
    let account_id = AccountId::from("BYBIT-001");
    let client_id = ClientId::from("BYBIT");
    let strategy_id = StrategyId::from("S-001");
    let instrument_id = InstrumentId::new(Symbol::from("ETHUSDT-LINEAR"), Venue::from("BYBIT"));

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
        cache.clone(),
    );

    let config = BybitExecClientConfig {
        api_key: Some("test_api_key".to_string()),
        api_secret: Some("test_api_secret".to_string()),
        product_types: vec![BybitProductType::Linear],
        environment: BybitEnvironment::Demo,
        base_url_http: Some(format!("http://{addr}")),
        base_url_ws_private: Some(format!("ws://{addr}/v5/private")),
        base_url_ws_trade: Some(format!("ws://{addr}/v5/trade")),
        proxy_url: None,
        http_timeout_secs: 10,
        max_retries: 1,
        retry_delay_initial_ms: 100,
        retry_delay_max_ms: 1000,
        heartbeat_interval_secs: 5,
        recv_window_ms: 5000,
        account_id: None,
        use_spot_position_reports: false,
        futures_leverages: None,
        position_mode: None,
        margin_mode: None,
        transport_backend: Default::default(),
    };

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    set_exec_event_sender(tx);

    let mut client = BybitExecutionClient::new(core, config).unwrap();
    client.connect().await.unwrap();
    client.start().unwrap();

    wait_until_async(
        || async { state.subscriptions.lock().await.len() >= 4 },
        Duration::from_secs(10),
    )
    .await;

    // Drain connection events (account state, subscriptions)
    while tokio::time::timeout(Duration::from_millis(200), rx.recv())
        .await
        .is_ok()
    {}

    let cid1 = ClientOrderId::from("test-list-order-1");
    let cid2 = ClientOrderId::from("test-list-order-2");

    let order1 = OrderAny::Market(MarketOrder::new(
        trader_id,
        strategy_id,
        instrument_id,
        cid1,
        OrderSide::Buy,
        Quantity::from("0.01"),
        TimeInForce::Gtc,
        UUID4::new(),
        UnixNanos::default(),
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
    ));
    let order2 = OrderAny::Market(MarketOrder::new(
        trader_id,
        strategy_id,
        instrument_id,
        cid2,
        OrderSide::Sell,
        Quantity::from("0.01"),
        TimeInForce::Gtc,
        UUID4::new(),
        UnixNanos::default(),
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
    ));

    let init1 = order1.init_event().clone();
    let init2 = order2.init_event().clone();

    cache
        .borrow_mut()
        .add_order(order1, None, Some(client_id), false)
        .unwrap();
    cache
        .borrow_mut()
        .add_order(order2, None, Some(client_id), false)
        .unwrap();

    let order_list = OrderList::new(
        OrderListId::from("test-list-1"),
        instrument_id,
        strategy_id,
        vec![cid1, cid2],
        UnixNanos::default(),
    );

    let cmd = SubmitOrderList::new(
        trader_id,
        Some(client_id),
        strategy_id,
        order_list,
        vec![init1, init2],
        None,
        None,
        None,
        UUID4::new(),
        UnixNanos::default(),
    );

    client.submit_order_list(cmd).unwrap();

    let mut submitted_count = 0;

    for _ in 0..2 {
        let event = tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .expect("timed out waiting for OrderSubmitted")
            .expect("channel closed");

        if let ExecutionEvent::Order(ref order_event) = event
            && order_event.to_string().contains("OrderSubmitted")
        {
            submitted_count += 1;
        }
    }

    assert_eq!(submitted_count, 2, "Expected 2 OrderSubmitted events");

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_exec_client_submit_order_list_denies_all_on_invalid_leg() {
    use nautilus_common::messages::execution::SubmitOrderList;
    use nautilus_model::orders::OrderList;

    let (addr, state) = start_test_server().await.unwrap();
    let trader_id = TraderId::from("TESTER-001");
    let account_id = AccountId::from("BYBIT-001");
    let client_id = ClientId::from("BYBIT");
    let strategy_id = StrategyId::from("S-001");
    let instrument_id = InstrumentId::new(Symbol::from("ETHUSDT-LINEAR"), Venue::from("BYBIT"));

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
        cache.clone(),
    );

    let config = BybitExecClientConfig {
        api_key: Some("test_api_key".to_string()),
        api_secret: Some("test_api_secret".to_string()),
        product_types: vec![BybitProductType::Linear],
        environment: BybitEnvironment::Demo,
        base_url_http: Some(format!("http://{addr}")),
        base_url_ws_private: Some(format!("ws://{addr}/v5/private")),
        base_url_ws_trade: Some(format!("ws://{addr}/v5/trade")),
        proxy_url: None,
        http_timeout_secs: 10,
        max_retries: 1,
        retry_delay_initial_ms: 100,
        retry_delay_max_ms: 1000,
        heartbeat_interval_secs: 5,
        recv_window_ms: 5000,
        account_id: None,
        use_spot_position_reports: false,
        futures_leverages: None,
        position_mode: None,
        margin_mode: None,
        transport_backend: Default::default(),
    };

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    set_exec_event_sender(tx);

    let mut client = BybitExecutionClient::new(core, config).unwrap();
    client.connect().await.unwrap();
    client.start().unwrap();

    wait_until_async(
        || async { state.subscriptions.lock().await.len() >= 4 },
        Duration::from_secs(10),
    )
    .await;

    // Drain connection events (account state, subscriptions)
    while tokio::time::timeout(Duration::from_millis(200), rx.recv())
        .await
        .is_ok()
    {}

    // Valid market order + unsupported TrailingStopMarket order
    let cid1 = ClientOrderId::from("test-deny-order-1");
    let cid2 = ClientOrderId::from("test-deny-order-2");

    let order1 = OrderAny::Market(MarketOrder::new(
        trader_id,
        strategy_id,
        instrument_id,
        cid1,
        OrderSide::Buy,
        Quantity::from("0.01"),
        TimeInForce::Gtc,
        UUID4::new(),
        UnixNanos::default(),
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
    ));

    let order2 = OrderAny::TrailingStopMarket(TrailingStopMarketOrder::new(
        trader_id,
        strategy_id,
        instrument_id,
        cid2,
        OrderSide::Sell,
        Quantity::from("0.01"),
        Price::from("1500.00"),
        TriggerType::LastPrice,
        rust_decimal::Decimal::new(100, 0),
        TrailingOffsetType::BasisPoints,
        TimeInForce::Gtc,
        None,
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
    ));

    let init1 = order1.init_event().clone();
    let init2 = order2.init_event().clone();

    cache
        .borrow_mut()
        .add_order(order1, None, Some(client_id), false)
        .unwrap();
    cache
        .borrow_mut()
        .add_order(order2, None, Some(client_id), false)
        .unwrap();

    let order_list = OrderList::new(
        OrderListId::from("test-deny-list-1"),
        instrument_id,
        strategy_id,
        vec![cid1, cid2],
        UnixNanos::default(),
    );

    let cmd = SubmitOrderList::new(
        trader_id,
        Some(client_id),
        strategy_id,
        order_list,
        vec![init1, init2],
        None,
        None,
        None,
        UUID4::new(),
        UnixNanos::default(),
    );

    client.submit_order_list(cmd).unwrap();

    // Both orders should be denied (not just the invalid one)
    let mut denied_count = 0;

    for _ in 0..2 {
        match tokio::time::timeout(Duration::from_secs(2), rx.recv()).await {
            Ok(Some(ExecutionEvent::Order(ref event)))
                if event.to_string().contains("OrderDenied") =>
            {
                denied_count += 1;
            }
            _ => break,
        }
    }

    assert_eq!(
        denied_count, 2,
        "Both orders should be denied when one leg is invalid"
    );

    client.disconnect().await.unwrap();
}
