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
    common::{
        consts::{BYBIT_CLIENT_ID, BYBIT_VENUE},
        enums::{
            BybitEnvironment, BybitMarginMode, BybitOrderSmpType, BybitPositionMode,
            BybitProductType,
        },
    },
    config::BybitExecutionClientConfig,
    execution::BybitExecutionClient,
};
use nautilus_common::{
    cache::Cache,
    clients::ExecutionClient,
    live::runner::{replace_system_event_sender, set_exec_event_sender},
    messages::{
        ExecutionEvent, SystemEvent,
        execution::{
            CancelOrder, ExecutionReport, GenerateOrderStatusReports,
            GeneratePositionStatusReports, ModifyOrder, SubmitOrder,
        },
        system::SocketState,
    },
    testing::wait_until_async,
};
use nautilus_core::{UUID4, UnixNanos, params::Params};
use nautilus_live::{ExecutionClientCore, SocketReconnectRegistry, SocketReconnectRequestOutcome};
use nautilus_model::{
    accounts::{AccountAny, MarginAccount},
    enums::{
        AccountType, OmsType, OrderSide, OrderStatus, TimeInForce, TrailingOffsetType, TriggerType,
    },
    events::{AccountState, OrderDenied, OrderEventAny},
    identifiers::{
        AccountId, ClientOrderId, InstrumentId, OrderListId, StrategyId, Symbol, TraderId,
        VenueOrderId,
    },
    orders::{MarketOrder, Order, OrderAny, TrailingStopMarketOrder},
    types::{AccountBalance, Money, Price, Quantity},
};
use nautilus_network::http::HttpClient;
use rstest::rstest;
use serde_json::{Value, json};
use ustr::Ustr;

#[derive(Clone)]
struct TestServerState {
    ws_connection_count: Arc<tokio::sync::Mutex<usize>>,
    private_ws_connections: Arc<AtomicUsize>,
    trade_ws_connections: Arc<AtomicUsize>,
    trade_order_ret_code: Arc<AtomicUsize>,
    trade_order_requests: Arc<AtomicUsize>,
    trade_order_payloads: Arc<tokio::sync::Mutex<Vec<Value>>>,
    trade_order_req_id_present: Arc<AtomicBool>,
    authenticated: Arc<AtomicBool>,
    subscriptions: Arc<tokio::sync::Mutex<Vec<String>>>,
    disconnect_trigger: Arc<AtomicBool>,
    reject_trade_websocket: Arc<AtomicBool>,
    empty_orders_realtime: Arc<AtomicBool>,
    rejected_orders_realtime: Arc<AtomicBool>,
    orders_realtime_requests: Arc<AtomicUsize>,
    position_requests: Arc<AtomicUsize>,
    wallet_balance_requests: Arc<AtomicUsize>,
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
            trade_order_ret_code: Arc::new(AtomicUsize::new(0)),
            trade_order_requests: Arc::new(AtomicUsize::new(0)),
            trade_order_payloads: Arc::new(tokio::sync::Mutex::new(Vec::new())),
            trade_order_req_id_present: Arc::new(AtomicBool::new(false)),
            authenticated: Arc::new(AtomicBool::new(false)),
            subscriptions: Arc::new(tokio::sync::Mutex::new(Vec::new())),
            disconnect_trigger: Arc::new(AtomicBool::new(false)),
            reject_trade_websocket: Arc::new(AtomicBool::new(false)),
            empty_orders_realtime: Arc::new(AtomicBool::new(false)),
            rejected_orders_realtime: Arc::new(AtomicBool::new(false)),
            orders_realtime_requests: Arc::new(AtomicUsize::new(0)),
            position_requests: Arc::new(AtomicUsize::new(0)),
            wallet_balance_requests: Arc::new(AtomicUsize::new(0)),
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

async fn handle_get_wallet_balance(
    State(state): State<TestServerState>,
    headers: HeaderMap,
) -> impl IntoResponse {
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
    state
        .wallet_balance_requests
        .fetch_add(1, Ordering::Relaxed);
    let wallet = load_test_data("http_get_wallet_balance.json");
    Json(wallet).into_response()
}

async fn handle_get_positions(
    State(state): State<TestServerState>,
    headers: HeaderMap,
) -> impl IntoResponse {
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
    state.position_requests.fetch_add(1, Ordering::Relaxed);
    let positions = load_test_data("http_get_positions.json");
    Json(positions).into_response()
}

async fn handle_get_empty_report_list(headers: HeaderMap) -> impl IntoResponse {
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
            "list": [],
            "nextPageCursor": ""
        },
        "retExtInfo": {},
        "time": 1704470400123i64
    }))
    .into_response()
}

async fn handle_get_orders_realtime(
    State(state): State<TestServerState>,
    headers: HeaderMap,
) -> impl IntoResponse {
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

    if state.empty_orders_realtime.load(Ordering::Relaxed) {
        state
            .orders_realtime_requests
            .fetch_add(1, Ordering::Relaxed);
        return Json(json!({
            "retCode": 0,
            "retMsg": "OK",
            "result": {
                "category": "linear",
                "list": [],
                "nextPageCursor": ""
            },
            "retExtInfo": {},
            "time": 1704470400123i64
        }))
        .into_response();
    }

    if state.rejected_orders_realtime.load(Ordering::Relaxed) {
        let mut orders = load_test_data("http_get_orders_realtime.json");
        let order = orders
            .get_mut("result")
            .and_then(|result| result.get_mut("list"))
            .and_then(Value::as_array_mut)
            .and_then(|list| list.first_mut())
            .expect("orders realtime fixture has first order");
        order["orderId"] = json!("test-order-id-12345");
        order["orderStatus"] = json!("Cancelled");
        order["cumExecQty"] = json!("0");
        order["rejectReason"] = json!("EC_PostOnlyWillTakeLiquidity");

        state
            .orders_realtime_requests
            .fetch_add(1, Ordering::Relaxed);
        return Json(orders).into_response();
    }

    let orders = load_test_data("http_get_orders_realtime.json");
    state
        .orders_realtime_requests
        .fetch_add(1, Ordering::Relaxed);
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
    if state.reject_trade_websocket.load(Ordering::Relaxed) {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
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
                    Some("order.create" | "order.amend" | "order.cancel") => {
                        state.trade_order_requests.fetch_add(1, Ordering::Relaxed);
                        state.trade_order_payloads.lock().await.push(value.clone());
                        let req_id = value.get("reqId").and_then(|v| v.as_str());
                        state.trade_order_req_id_present.store(
                            req_id.is_some_and(|req_id| !req_id.is_empty()),
                            Ordering::Relaxed,
                        );
                        let ret_code = state.trade_order_ret_code.load(Ordering::Relaxed);
                        let ret_msg = if ret_code == 0 {
                            "OK"
                        } else {
                            "Too many visits."
                        };
                        let response = json!({
                            "retCode": ret_code,
                            "retMsg": ret_msg,
                            "data": {},
                            "retExtInfo": {},
                            "connId": "test-conn-id",
                            "reqId": req_id.unwrap_or(""),
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
        .route("/v5/order/history", get(handle_get_empty_report_list))
        .route("/v5/execution/list", get(handle_get_empty_report_list))
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

fn create_test_exec_config(addr: SocketAddr) -> BybitExecutionClientConfig {
    BybitExecutionClientConfig {
        api_key: Some("test_api_key".into()),
        api_secret: Some("test_api_secret".into()),
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
        auto_repay_spot_borrows: false,
        futures_leverages: None,
        position_mode: None,
        margin_mode: None,
        transport_backend: Default::default(),
        ..Default::default()
    }
}

fn create_test_demo_exec_config(addr: SocketAddr) -> BybitExecutionClientConfig {
    let mut config = create_test_exec_config(addr);
    config.environment = BybitEnvironment::Demo;
    config.max_retries = 0;
    config
}

fn create_test_execution_client(
    addr: SocketAddr,
) -> (
    BybitExecutionClient,
    tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
    Rc<RefCell<Cache>>,
) {
    create_test_execution_client_with_config(create_test_exec_config(addr))
}

fn create_test_execution_client_with_config(
    config: BybitExecutionClientConfig,
) -> (
    BybitExecutionClient,
    tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
    Rc<RefCell<Cache>>,
) {
    let trader_id = TraderId::from("TESTER-001");
    let account_id = AccountId::from("BYBIT-001");
    let client_id = *BYBIT_CLIENT_ID;

    let cache = Rc::new(RefCell::new(Cache::default()));

    let core = ExecutionClientCore::new(
        trader_id,
        client_id,
        *BYBIT_VENUE,
        OmsType::Netting,
        account_id,
        AccountType::Margin,
        None,
        cache.clone(),
    );

    // Event channel must be set before creating client due to thread-local storage
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    set_exec_event_sender(tx);

    let client = BybitExecutionClient::new(core, config).unwrap();

    (client, rx, cache)
}

#[rstest]
#[case(true, 1)]
#[case(false, 0)]
#[tokio::test]
async fn test_exec_client_scoped_spot_position_reports_follow_config(
    #[case] use_spot_position_reports: bool,
    #[case] expected_reports: usize,
) {
    let (addr, _state) = start_test_server().await.unwrap();
    let mut config = create_test_exec_config(addr);
    config.product_types = vec![BybitProductType::Spot];
    config.use_spot_position_reports = use_spot_position_reports;
    let (mut client, _rx, cache) = create_test_execution_client_with_config(config);
    add_test_account_to_cache(&cache, AccountId::from("BYBIT-001"));
    client.connect().await.unwrap();

    let reports = client
        .generate_position_status_reports(&GeneratePositionStatusReports::new(
            UUID4::new(),
            UnixNanos::default(),
            Some(InstrumentId::from("ETHUSDT-SPOT.BYBIT")),
            None,
            None,
            None,
            None,
        ))
        .await
        .unwrap();

    assert_eq!(reports.len(), expected_reports);
    if use_spot_position_reports {
        assert_eq!(reports[0].instrument_id, "ETHUSDT-SPOT.BYBIT".into());
    }

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_exec_client_mixed_unscoped_position_reports_omit_spot() {
    let (addr, state) = start_test_server().await.unwrap();
    let mut config = create_test_exec_config(addr);
    config.product_types = vec![BybitProductType::Linear, BybitProductType::Spot];
    config.use_spot_position_reports = true;
    let (mut client, _rx, cache) = create_test_execution_client_with_config(config);
    add_test_account_to_cache(&cache, AccountId::from("BYBIT-001"));
    client.connect().await.unwrap();

    // Measure against a post-connect baseline, since connect issues requests of its own.
    let positions_before = state.position_requests.load(Ordering::Relaxed);
    let wallet_before = state.wallet_balance_requests.load(Ordering::Relaxed);

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
        .expect("derivative reports must succeed when SPOT coverage is unavailable");

    assert!(
        reports
            .iter()
            .any(|report| report.instrument_id == "BTCUSDT-LINEAR.BYBIT".into())
    );
    // An unscoped LINEAR query is one request per settle coin (USDT, USDC); SPOT adds none.
    assert_eq!(
        state.position_requests.load(Ordering::Relaxed) - positions_before,
        2,
        "the LINEAR product type should still be requested"
    );
    // SPOT is served from wallet balances, so this is what proves it was omitted.
    assert_eq!(
        state.wallet_balance_requests.load(Ordering::Relaxed) - wallet_before,
        0,
        "no SPOT wallet balance request should be issued for a bulk report"
    );

    client.disconnect().await.unwrap();
}

#[rstest]
#[case(vec![], "BTCUSDT-LINEAR.BYBIT", true)]
#[case(vec![], "BTCUSD-INVERSE.BYBIT", false)]
#[case(vec![], "ETHUSDT-SPOT.BYBIT", false)]
#[case(vec![], "BTCUSDT.BYBIT", false)]
#[case(vec![BybitProductType::Spot], "BTCUSDT-LINEAR.BYBIT", false)]
#[case(vec![BybitProductType::Spot], "BTCUSD-INVERSE.BYBIT", false)]
#[case(vec![BybitProductType::Spot], "ETHUSDT-SPOT.BYBIT", false)]
#[case(vec![BybitProductType::Spot], "BTCUSDT.BYBIT", false)]
#[case(
    vec![BybitProductType::Linear, BybitProductType::Spot],
    "BTCUSDT-LINEAR.BYBIT",
    true
)]
#[case(
    vec![BybitProductType::Linear, BybitProductType::Spot],
    "BTCUSD-INVERSE.BYBIT",
    false
)]
#[case(
    vec![BybitProductType::Linear, BybitProductType::Spot],
    "ETHUSDT-SPOT.BYBIT",
    false
)]
#[case(
    vec![BybitProductType::Linear, BybitProductType::Spot],
    "BTCUSDT.BYBIT",
    false
)]
#[case(
    vec![BybitProductType::Inverse, BybitProductType::Spot],
    "BTCUSDT-LINEAR.BYBIT",
    false
)]
#[case(
    vec![BybitProductType::Inverse, BybitProductType::Spot],
    "BTCUSD-INVERSE.BYBIT",
    true
)]
#[case(
    vec![BybitProductType::Inverse, BybitProductType::Spot],
    "ETHUSDT-SPOT.BYBIT",
    false
)]
#[case(
    vec![BybitProductType::Inverse, BybitProductType::Spot],
    "BTCUSDT.BYBIT",
    false
)]
#[tokio::test]
async fn test_exec_client_bulk_position_coverage_by_product_type(
    #[case] product_types: Vec<BybitProductType>,
    #[case] instrument_id: &str,
    #[case] expected: bool,
) {
    let (addr, _state) = start_test_server().await.unwrap();
    let mut config = create_test_exec_config(addr);
    config.product_types = product_types;
    let (client, _rx, _cache) = create_test_execution_client_with_config(config);

    // An identifier carrying no recognized product-type suffix must fail closed: absence of a
    // report from a bulk request is not evidence the position is flat.
    assert_eq!(
        client.provides_bulk_position_coverage(InstrumentId::from(instrument_id)),
        expected
    );
}

#[rstest]
#[tokio::test]
async fn test_exec_client_mass_status_omits_spot_and_preserves_derivative_positions() {
    let (addr, state) = start_test_server().await.unwrap();
    let mut config = create_test_exec_config(addr);
    config.product_types = vec![BybitProductType::Linear, BybitProductType::Spot];
    config.use_spot_position_reports = true;
    let (mut client, _rx, cache) = create_test_execution_client_with_config(config);
    add_test_account_to_cache(&cache, AccountId::from("BYBIT-001"));
    client.connect().await.unwrap();
    state.wallet_balance_requests.store(0, Ordering::Relaxed);

    let mass_status = client
        .generate_mass_status(None)
        .await
        .expect("mass status must not fail when SPOT coverage is unavailable")
        .expect("mass status must be populated");

    assert!(
        mass_status
            .position_reports()
            .contains_key(&InstrumentId::from("BTCUSDT-LINEAR.BYBIT"))
    );
    assert_eq!(state.wallet_balance_requests.load(Ordering::Relaxed), 0);

    client.disconnect().await.unwrap();
}

fn create_test_demo_execution_client(
    addr: SocketAddr,
) -> (
    BybitExecutionClient,
    tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
    Rc<RefCell<Cache>>,
) {
    let trader_id = TraderId::from("TESTER-001");
    let account_id = AccountId::from("BYBIT-001");
    let client_id = *BYBIT_CLIENT_ID;

    let cache = Rc::new(RefCell::new(Cache::default()));

    let core = ExecutionClientCore::new(
        trader_id,
        client_id,
        *BYBIT_VENUE,
        OmsType::Netting,
        account_id,
        AccountType::Margin,
        None,
        cache.clone(),
    );

    let config = create_test_demo_exec_config(addr);

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

async fn drain_execution_events(rx: &mut tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>) {
    while tokio::time::timeout(Duration::from_millis(200), rx.recv())
        .await
        .is_ok()
    {}
}

async fn assert_no_cancel_rejected(
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
    duration: Duration,
) {
    let reject_window = tokio::time::sleep(duration);
    tokio::pin!(reject_window);

    loop {
        tokio::select! {
            () = &mut reject_window => break,
            event = rx.recv() => {
                let event = event.expect("channel closed");
                assert!(
                    !matches!(event, ExecutionEvent::Order(OrderEventAny::CancelRejected(_))),
                    "Ambiguous cancel outcome must not emit OrderCancelRejected: {event:?}",
                );
            }
        }
    }
}

async fn assert_no_modify_rejected(
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
    duration: Duration,
) {
    let reject_window = tokio::time::sleep(duration);
    tokio::pin!(reject_window);

    loop {
        tokio::select! {
            () = &mut reject_window => break,
            event = rx.recv() => {
                let event = event.expect("channel closed");
                assert!(
                    !matches!(event, ExecutionEvent::Order(OrderEventAny::ModifyRejected(_))),
                    "Ambiguous modify outcome must not emit OrderModifyRejected: {event:?}",
                );
            }
        }
    }
}

#[rstest]
#[tokio::test]
async fn test_exec_client_creation() {
    let (addr, _state) = start_test_server().await.unwrap();
    let (client, _rx, _cache) = create_test_execution_client(addr);

    assert_eq!(client.client_id(), *BYBIT_CLIENT_ID);
    assert_eq!(client.venue(), *BYBIT_VENUE);
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

    let mut connected = Vec::new();
    while connected.len() < 2 {
        let event = tokio::time::timeout(Duration::from_secs(2), system_rx.recv())
            .await
            .unwrap()
            .unwrap();
        let SystemEvent::SocketState(change) = event;
        if change.state == SocketState::Connected {
            connected.push(change.endpoint);
        }
    }
    connected.sort_unstable();
    let expected = vec![
        Ustr::from("bybit-trading"),
        Ustr::from("bybit-user-streams"),
    ];
    assert!(client.is_connected());
    assert!(state.authenticated.load(Ordering::Relaxed));
    assert_eq!(connected, expected);

    for endpoint in &expected {
        let handle = registry.handle(*BYBIT_CLIENT_ID, *endpoint).unwrap();
        assert_eq!(
            handle.request_reconnect(),
            SocketReconnectRequestOutcome::Accepted
        );
    }

    let mut disconnected = Vec::new();
    while disconnected.len() < 2 {
        let event = tokio::time::timeout(Duration::from_secs(2), system_rx.recv())
            .await
            .unwrap()
            .unwrap();
        let SystemEvent::SocketState(change) = event;
        if change.state == SocketState::Disconnected {
            disconnected.push(change.endpoint);
        }
    }
    disconnected.sort_unstable();
    assert_eq!(disconnected, expected);

    let subs = state.subscriptions.lock().await;
    assert!(subs.contains(&"order".to_string()));
    assert!(subs.contains(&"execution".to_string()));
    assert!(subs.contains(&"position".to_string()));
    assert!(subs.contains(&"wallet".to_string()));
    drop(subs);

    client.disconnect().await.unwrap();
    assert!(!client.is_connected());
    assert!(
        expected
            .iter()
            .all(|endpoint| registry.handle(*BYBIT_CLIENT_ID, *endpoint).is_none())
    );
}

#[rstest]
#[tokio::test]
async fn test_generate_order_status_reports_open_only_retains_recent_closed() {
    let (addr, state) = start_test_server().await.unwrap();
    let (mut client, _rx, cache) = create_test_execution_client(addr);
    add_test_account_to_cache(&cache, AccountId::from("BYBIT-001"));
    client.connect().await.unwrap();
    state
        .rejected_orders_realtime
        .store(true, Ordering::Relaxed);

    let command = GenerateOrderStatusReports::new(
        UUID4::new(),
        UnixNanos::default(),
        true,
        Some(InstrumentId::from("ETHUSDT-LINEAR.BYBIT")),
        None,
        None,
        None,
        None,
    );
    let reports = client
        .generate_order_status_reports(&command)
        .await
        .unwrap();

    assert_eq!(reports.len(), 2);
    assert_eq!(reports[0].venue_order_id, "test-order-id-12345".into());
    assert_eq!(reports[0].order_status, OrderStatus::Rejected);
    assert_eq!(reports[1].venue_order_id, "open-order-2".into());
    assert_eq!(reports[1].order_status, OrderStatus::Accepted);

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_exec_client_failed_startup_rolls_back_private_stream() {
    let (addr, state) = start_test_server().await.unwrap();
    state.reject_trade_websocket.store(true, Ordering::Relaxed);
    let registry = SocketReconnectRegistry::default();
    let (mut client, _rx, cache) = registry.scope(|| create_test_execution_client(addr));
    add_test_account_to_cache(&cache, AccountId::from("BYBIT-001"));

    let error = client.connect().await.unwrap_err();

    wait_until_async(
        || async { *state.ws_connection_count.lock().await == 0 },
        Duration::from_secs(2),
    )
    .await;
    assert!(
        error.to_string().contains("WebSocket transport error"),
        "unexpected startup error: {error:#}",
    );
    assert_eq!(state.private_ws_connections.load(Ordering::Relaxed), 1);
    assert!(!client.is_connected());
    assert!(
        ["bybit-user-streams", "bybit-trading"]
            .into_iter()
            .all(|endpoint| registry
                .handle(*BYBIT_CLIENT_ID, Ustr::from(endpoint))
                .is_none())
    );
}

#[rstest]
#[tokio::test]
async fn test_exec_client_connect_applies_position_mode_for_derivative_symbols() {
    let (addr, state) = start_test_server().await.unwrap();
    let trader_id = TraderId::from("TESTER-001");
    let account_id = AccountId::from("BYBIT-001");
    let client_id = *BYBIT_CLIENT_ID;

    let cache = Rc::new(RefCell::new(Cache::default()));
    add_test_account_to_cache(&cache, account_id);

    let core = ExecutionClientCore::new(
        trader_id,
        client_id,
        *BYBIT_VENUE,
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
    let client_id = *BYBIT_CLIENT_ID;

    let cache = Rc::new(RefCell::new(Cache::default()));
    add_test_account_to_cache(&cache, account_id);

    let core = ExecutionClientCore::new(
        trader_id,
        client_id,
        *BYBIT_VENUE,
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
    let client_id = *BYBIT_CLIENT_ID;

    let cache = Rc::new(RefCell::new(Cache::default()));
    add_test_account_to_cache(&cache, account_id);

    let core = ExecutionClientCore::new(
        trader_id,
        client_id,
        *BYBIT_VENUE,
        OmsType::Netting,
        account_id,
        AccountType::Margin,
        None,
        cache,
    );

    let config = BybitExecutionClientConfig {
        api_key: Some("test_api_key".into()),
        api_secret: Some("test_api_secret".into()),
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
        auto_repay_spot_borrows: false,
        futures_leverages: None,
        position_mode: None,
        margin_mode: None,
        transport_backend: Default::default(),
        ..Default::default()
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
        Some(*BYBIT_CLIENT_ID),
        StrategyId::from("S-001"),
        InstrumentId::new(Symbol::from("ETHUSDT-LINEAR"), *BYBIT_VENUE),
        ClientOrderId::from("client-open-1"),
        None,
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
#[case::derivatives_config_default(
    "ETHUSDT-LINEAR",
    Some(BybitOrderSmpType::CancelMaker),
    None,
    Some("CancelMaker")
)]
#[case::spot_param_overrides_config(
    "BTCUSDT-SPOT",
    Some(BybitOrderSmpType::CancelMaker),
    Some("CancelBoth"),
    Some("CancelBoth")
)]
#[case::spot_param_without_config("BTCUSDT-SPOT", None, Some("CancelTaker"), Some("CancelTaker"))]
#[case::unset_omits_field("ETHUSDT-LINEAR", None, None, None)]
#[tokio::test]
async fn test_exec_client_submit_order_sends_configured_smp_type(
    #[case] symbol: &str,
    #[case] configured: Option<BybitOrderSmpType>,
    #[case] param: Option<&str>,
    #[case] expected: Option<&str>,
) {
    let (addr, state) = start_test_server().await.unwrap();
    let mut config = create_test_exec_config(addr);
    config.product_types = vec![BybitProductType::Linear, BybitProductType::Spot];
    config.smp_type = configured;

    let (mut client, mut rx, cache) = create_test_execution_client_with_config(config);
    add_test_account_to_cache(&cache, AccountId::from("BYBIT-001"));
    client.start().unwrap();
    client.connect().await.unwrap();

    wait_until_async(
        || async { state.subscriptions.lock().await.len() >= 4 },
        Duration::from_secs(10),
    )
    .await;
    drain_execution_events(&mut rx).await;

    let trader_id = TraderId::from("TESTER-001");
    let strategy_id = StrategyId::from("S-001");
    let client_id = *BYBIT_CLIENT_ID;
    let instrument_id = InstrumentId::new(Symbol::from(symbol), *BYBIT_VENUE);
    let client_order_id = ClientOrderId::from("test-smp-submit");
    let order = OrderAny::Market(MarketOrder::new(
        trader_id,
        strategy_id,
        instrument_id,
        client_order_id,
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
    let init = order.init_event().clone();
    cache
        .borrow_mut()
        .add_order(order, None, Some(client_id), false)
        .unwrap();

    let params = param.map(|value| {
        let mut params = Params::new();
        params.insert("smp_type".to_string(), json!(value));
        params
    });

    let command = SubmitOrder::new(
        trader_id,
        Some(client_id),
        strategy_id,
        instrument_id,
        client_order_id,
        init,
        None,
        None,
        params,
        UUID4::new(),
        UnixNanos::default(),
        None,
    );

    client.submit_order(command).unwrap();

    wait_until_async(
        || async { state.trade_order_payloads.lock().await.len() == 1 },
        Duration::from_secs(5),
    )
    .await;

    let payloads = state.trade_order_payloads.lock().await;
    assert_eq!(payloads.len(), 1);
    assert_eq!(payloads[0]["op"].as_str(), Some("order.create"));

    let order_args = &payloads[0]["args"][0];

    assert_eq!(
        order_args.get("orderLinkId").and_then(Value::as_str),
        Some(client_order_id.as_str())
    );
    assert_eq!(order_args.get("smpType").and_then(Value::as_str), expected);

    drop(payloads);
    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_exec_client_trade_rate_limit_emits_order_rejected() {
    let (addr, state) = start_test_server().await.unwrap();
    let (mut client, mut rx, cache) = create_test_execution_client(addr);
    add_test_account_to_cache(&cache, AccountId::from("BYBIT-001"));
    client.start().unwrap();
    client.connect().await.unwrap();

    wait_until_async(
        || async { state.subscriptions.lock().await.len() >= 4 },
        Duration::from_secs(10),
    )
    .await;
    drain_execution_events(&mut rx).await;
    state.trade_order_ret_code.store(10006, Ordering::Relaxed);

    let trader_id = TraderId::from("TESTER-001");
    let strategy_id = StrategyId::from("S-001");
    let client_id = *BYBIT_CLIENT_ID;
    let instrument_id = InstrumentId::new(Symbol::from("ETHUSDT-LINEAR"), *BYBIT_VENUE);
    let client_order_id = ClientOrderId::from("test-ws-rate-limit-reject");
    let order = OrderAny::Market(MarketOrder::new(
        trader_id,
        strategy_id,
        instrument_id,
        client_order_id,
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
    let init = order.init_event().clone();
    cache
        .borrow_mut()
        .add_order(order, None, Some(client_id), false)
        .unwrap();
    let command = SubmitOrder::new(
        trader_id,
        Some(client_id),
        strategy_id,
        instrument_id,
        client_order_id,
        init,
        None,
        None,
        None,
        UUID4::new(),
        UnixNanos::default(),
        None,
    );

    client.submit_order(command).unwrap();

    let submitted = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("timed out waiting for OrderSubmitted")
        .expect("channel closed");
    assert!(
        matches!(submitted, ExecutionEvent::Order(OrderEventAny::Submitted(ref event))
            if event.client_order_id == client_order_id),
        "Expected OrderSubmitted for {client_order_id}, was {submitted:?}",
    );
    wait_until_async(
        || async { state.trade_order_requests.load(Ordering::Relaxed) == 1 },
        Duration::from_secs(5),
    )
    .await;
    assert!(state.trade_order_req_id_present.load(Ordering::Relaxed));
    let rejected = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("timed out waiting for OrderRejected")
        .expect("channel closed");
    let ExecutionEvent::Order(OrderEventAny::Rejected(event)) = rejected else {
        panic!("Expected OrderRejected, was {rejected:?}");
    };
    assert_eq!(event.client_order_id, client_order_id);
    assert_eq!(event.reason.to_string(), "Too many visits.");

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
        Some(*BYBIT_CLIENT_ID),
        AccountId::from("BYBIT-001"),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None, // correlation_id
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
    let client_id = *BYBIT_CLIENT_ID;
    let strategy_id = StrategyId::from("S-001");
    let instrument_id = InstrumentId::new(Symbol::from("ETHUSDT-LINEAR"), *BYBIT_VENUE);

    let cache = Rc::new(RefCell::new(Cache::default()));
    add_test_account_to_cache(&cache, account_id);

    let core = ExecutionClientCore::new(
        trader_id,
        client_id,
        *BYBIT_VENUE,
        OmsType::Netting,
        account_id,
        AccountType::Margin,
        None,
        cache.clone(),
    );

    let config = BybitExecutionClientConfig {
        api_key: Some("test_api_key".into()),
        api_secret: Some("test_api_secret".into()),
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
        auto_repay_spot_borrows: false,
        futures_leverages: None,
        position_mode: None,
        margin_mode: None,
        transport_backend: Default::default(),
        ..Default::default()
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
        None, // correlation_id
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
async fn test_exec_client_demo_cancel_post_lookup_failure_does_not_reject() {
    let (addr, state) = start_test_server().await.unwrap();
    let (mut client, mut rx, cache) = create_test_demo_execution_client(addr);
    add_test_account_to_cache(&cache, AccountId::from("BYBIT-001"));

    client.connect().await.unwrap();
    client.start().unwrap();

    wait_until_async(
        || async { state.subscriptions.lock().await.len() >= 4 },
        Duration::from_secs(10),
    )
    .await;
    drain_execution_events(&mut rx).await;

    let cmd = CancelOrder::new(
        TraderId::from("TESTER-001"),
        Some(*BYBIT_CLIENT_ID),
        StrategyId::from("S-001"),
        InstrumentId::new(Symbol::from("ETHUSDT-LINEAR"), *BYBIT_VENUE),
        ClientOrderId::from("test-cancel-post-lookup-ambiguous"),
        Some(VenueOrderId::from("test-order-id-12345")),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );

    client.cancel_order(cmd).unwrap();

    assert_no_cancel_rejected(&mut rx, Duration::from_millis(300)).await;

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_exec_client_demo_modify_whole_http_failure_does_not_reject() {
    let (addr, state) = start_test_server().await.unwrap();
    let (mut client, mut rx, cache) = create_test_demo_execution_client(addr);
    add_test_account_to_cache(&cache, AccountId::from("BYBIT-001"));

    client.connect().await.unwrap();
    client.start().unwrap();

    wait_until_async(
        || async { state.subscriptions.lock().await.len() >= 4 },
        Duration::from_secs(10),
    )
    .await;
    drain_execution_events(&mut rx).await;

    let cmd = ModifyOrder::new(
        TraderId::from("TESTER-001"),
        Some(*BYBIT_CLIENT_ID),
        StrategyId::from("S-001"),
        InstrumentId::new(Symbol::from("ETHUSDT-LINEAR"), *BYBIT_VENUE),
        ClientOrderId::from("test-modify-http-ambiguous"),
        Some(VenueOrderId::from("test-order-id-12345")),
        Some(Quantity::from("0.02")),
        Some(Price::from("1600.00")),
        None,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );

    client.modify_order(cmd).unwrap();

    assert_no_modify_rejected(&mut rx, Duration::from_millis(300)).await;

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_exec_client_demo_submit_post_lookup_failure_does_not_reject() {
    let (addr, state) = start_test_server().await.unwrap();

    let trader_id = TraderId::from("TESTER-001");
    let account_id = AccountId::from("BYBIT-001");
    let client_id = *BYBIT_CLIENT_ID;
    let strategy_id = StrategyId::from("S-001");
    let instrument_id = InstrumentId::new(Symbol::from("ETHUSDT-LINEAR"), *BYBIT_VENUE);

    let cache = Rc::new(RefCell::new(Cache::default()));
    add_test_account_to_cache(&cache, account_id);

    let core = ExecutionClientCore::new(
        trader_id,
        client_id,
        *BYBIT_VENUE,
        OmsType::Netting,
        account_id,
        AccountType::Margin,
        None,
        cache.clone(),
    );

    let config = create_test_demo_exec_config(addr);
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

    while tokio::time::timeout(Duration::from_millis(200), rx.recv())
        .await
        .is_ok()
    {}

    state.empty_orders_realtime.store(true, Ordering::Relaxed);
    let order_lookup_requests = state.orders_realtime_requests.load(Ordering::Relaxed);

    let cid = ClientOrderId::from("test-unknown-submit-outcome");
    let order = OrderAny::Market(MarketOrder::new(
        trader_id,
        strategy_id,
        instrument_id,
        cid,
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
    let init = order.init_event().clone();

    cache
        .borrow_mut()
        .add_order(order, None, Some(client_id), false)
        .unwrap();

    let cmd = SubmitOrder::new(
        trader_id,
        Some(client_id),
        strategy_id,
        instrument_id,
        cid,
        init,
        None,
        None,
        None,
        UUID4::new(),
        UnixNanos::default(),
        None, // correlation_id
    );

    client.submit_order(cmd).unwrap();

    let submitted = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("timed out waiting for OrderSubmitted")
        .expect("channel closed");
    assert!(
        matches!(submitted, ExecutionEvent::Order(ref event) if event.to_string().contains("OrderSubmitted")),
        "Expected OrderSubmitted, was {submitted:?}",
    );

    wait_until_async(
        || async { state.orders_realtime_requests.load(Ordering::Relaxed) > order_lookup_requests },
        Duration::from_secs(5),
    )
    .await;

    let reject_window = tokio::time::sleep(Duration::from_millis(300));
    tokio::pin!(reject_window);

    loop {
        tokio::select! {
            () = &mut reject_window => break,
            event = rx.recv() => {
                let event = event.expect("channel closed");
                assert!(
                    !matches!(event, ExecutionEvent::Order(ref order_event) if order_event.to_string().contains("OrderRejected")),
                    "Unknown submit outcome must not emit OrderRejected: {event:?}",
                );
            }
        }
    }

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_exec_client_demo_submit_tp_trigger_price_emits_order_denied() {
    let (addr, state) = start_test_server().await.unwrap();

    let trader_id = TraderId::from("TESTER-001");
    let account_id = AccountId::from("BYBIT-001");
    let client_id = *BYBIT_CLIENT_ID;
    let strategy_id = StrategyId::from("S-001");
    let instrument_id = InstrumentId::new(Symbol::from("ETHUSDT-LINEAR"), *BYBIT_VENUE);

    let cache = Rc::new(RefCell::new(Cache::default()));
    add_test_account_to_cache(&cache, account_id);

    let core = ExecutionClientCore::new(
        trader_id,
        client_id,
        *BYBIT_VENUE,
        OmsType::Netting,
        account_id,
        AccountType::Margin,
        None,
        cache.clone(),
    );

    let config = create_test_demo_exec_config(addr);
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

    while tokio::time::timeout(Duration::from_millis(200), rx.recv())
        .await
        .is_ok()
    {}

    let cid = ClientOrderId::from("test-tp-trigger-denied");
    let order = OrderAny::Market(MarketOrder::new(
        trader_id,
        strategy_id,
        instrument_id,
        cid,
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
    let init = order.init_event().clone();

    cache
        .borrow_mut()
        .add_order(order, None, Some(client_id), false)
        .unwrap();

    let mut params = Params::new();
    params.insert("take_profit".to_string(), json!("3000"));
    params.insert("tp_trigger_price".to_string(), json!("2950"));

    let cmd = SubmitOrder::new(
        trader_id,
        Some(client_id),
        strategy_id,
        instrument_id,
        cid,
        init,
        None,
        None,
        Some(params),
        UUID4::new(),
        UnixNanos::default(),
        None, // correlation_id
    );

    client.submit_order(cmd).unwrap();

    // The demo path cannot carry TP/SL trigger prices, so the first event must be OrderDenied
    // (no OrderSubmitted, no HTTP submission).
    let event = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("timed out waiting for OrderDenied")
        .expect("channel closed");
    let text = match event {
        ExecutionEvent::Order(ref order_event) => order_event.to_string(),
        other => panic!("Expected OrderDenied, was {other:?}"),
    };
    assert!(
        text.contains("OrderDenied")
            && text.contains("UNSUPPORTED_TP_SL")
            && text.contains("TP/SL trigger prices are not supported in demo mode"),
        "Expected OrderDenied with trigger-price reason, was {text}",
    );

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_exec_client_demo_submit_confirmed_rejection_emits_order_rejected() {
    let (addr, state) = start_test_server().await.unwrap();

    let trader_id = TraderId::from("TESTER-001");
    let account_id = AccountId::from("BYBIT-001");
    let client_id = *BYBIT_CLIENT_ID;
    let strategy_id = StrategyId::from("S-001");
    let instrument_id = InstrumentId::new(Symbol::from("ETHUSDT-LINEAR"), *BYBIT_VENUE);

    let cache = Rc::new(RefCell::new(Cache::default()));
    add_test_account_to_cache(&cache, account_id);

    let core = ExecutionClientCore::new(
        trader_id,
        client_id,
        *BYBIT_VENUE,
        OmsType::Netting,
        account_id,
        AccountType::Margin,
        None,
        cache.clone(),
    );

    let config = create_test_demo_exec_config(addr);
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

    while tokio::time::timeout(Duration::from_millis(200), rx.recv())
        .await
        .is_ok()
    {}

    state
        .rejected_orders_realtime
        .store(true, Ordering::Relaxed);
    let order_lookup_requests = state.orders_realtime_requests.load(Ordering::Relaxed);

    let cid = ClientOrderId::from("test-confirmed-submit-reject");
    let order = OrderAny::Market(MarketOrder::new(
        trader_id,
        strategy_id,
        instrument_id,
        cid,
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
    let init = order.init_event().clone();

    cache
        .borrow_mut()
        .add_order(order, None, Some(client_id), false)
        .unwrap();

    let cmd = SubmitOrder::new(
        trader_id,
        Some(client_id),
        strategy_id,
        instrument_id,
        cid,
        init,
        None,
        None,
        None,
        UUID4::new(),
        UnixNanos::default(),
        None, // correlation_id
    );

    client.submit_order(cmd).unwrap();

    let submitted = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("timed out waiting for OrderSubmitted")
        .expect("channel closed");
    assert!(
        matches!(submitted, ExecutionEvent::Order(OrderEventAny::Submitted(ref event)) if event.client_order_id == cid),
        "Expected OrderSubmitted for {cid}, was {submitted:?}",
    );

    wait_until_async(
        || async { state.orders_realtime_requests.load(Ordering::Relaxed) > order_lookup_requests },
        Duration::from_secs(5),
    )
    .await;

    let rejected = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("timed out waiting for OrderRejected")
        .expect("channel closed");
    let ExecutionEvent::Order(OrderEventAny::Rejected(event)) = rejected else {
        panic!("Expected OrderRejected, was {rejected:?}");
    };

    assert_eq!(event.client_order_id, cid);
    assert_eq!(event.reason.to_string(), "EC_PostOnlyWillTakeLiquidity");
    assert!(!event.reconciliation);
    assert!(event.due_post_only);

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
    let client_id = *BYBIT_CLIENT_ID;
    let strategy_id = StrategyId::from("S-001");
    let instrument_id = InstrumentId::new(Symbol::from("ETHUSDT-LINEAR"), *BYBIT_VENUE);

    let cache = Rc::new(RefCell::new(Cache::default()));
    add_test_account_to_cache(&cache, account_id);

    let core = ExecutionClientCore::new(
        trader_id,
        client_id,
        *BYBIT_VENUE,
        OmsType::Netting,
        account_id,
        AccountType::Margin,
        None,
        cache.clone(),
    );

    let config = BybitExecutionClientConfig {
        api_key: Some("test_api_key".into()),
        api_secret: Some("test_api_secret".into()),
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
        auto_repay_spot_borrows: false,
        futures_leverages: None,
        position_mode: None,
        margin_mode: None,
        transport_backend: Default::default(),
        ..Default::default()
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
        None,
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
        None, // correlation_id
    );

    client.submit_order_list(cmd).unwrap();

    // The whole list is denied: the offending TrailingStopMarket leg carries the specific
    // UNSUPPORTED_ORDER_TYPE reason while the valid leg renders ORDER_LIST_DENIED.
    let mut denied = Vec::new();

    for _ in 0..2 {
        match tokio::time::timeout(Duration::from_secs(2), rx.recv()).await {
            Ok(Some(ExecutionEvent::Order(ref event)))
                if event.to_string().contains("OrderDenied") =>
            {
                denied.push(event.to_string());
            }
            _ => break,
        }
    }

    assert_eq!(
        denied.len(),
        2,
        "Both orders should be denied when one leg is invalid"
    );

    let offender = denied
        .iter()
        .find(|text| text.contains("client_order_id=test-deny-order-2"))
        .expect("missing denied event for invalid leg");
    assert!(
        offender.contains("UNSUPPORTED_ORDER_TYPE"),
        "offender reason was: {offender}"
    );

    let sibling = denied
        .iter()
        .find(|text| text.contains("client_order_id=test-deny-order-1"))
        .expect("missing denied event for valid leg");
    assert!(
        sibling.contains("ORDER_LIST_DENIED"),
        "sibling reason was: {sibling}"
    );

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_exec_client_submit_order_unsupported_order_type_emits_order_denied() {
    let (addr, state) = start_test_server().await.unwrap();

    let trader_id = TraderId::from("TESTER-001");
    let account_id = AccountId::from("BYBIT-001");
    let client_id = *BYBIT_CLIENT_ID;
    let strategy_id = StrategyId::from("S-001");
    let instrument_id = InstrumentId::new(Symbol::from("ETHUSDT-LINEAR"), *BYBIT_VENUE);

    let cache = Rc::new(RefCell::new(Cache::default()));
    add_test_account_to_cache(&cache, account_id);

    let core = ExecutionClientCore::new(
        trader_id,
        client_id,
        *BYBIT_VENUE,
        OmsType::Netting,
        account_id,
        AccountType::Margin,
        None,
        cache.clone(),
    );

    let config = create_test_demo_exec_config(addr);
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

    while tokio::time::timeout(Duration::from_millis(200), rx.recv())
        .await
        .is_ok()
    {}

    // Bybit does not support TrailingStopMarket, so the single-order path denies before submit
    let cid = ClientOrderId::from("test-unsupported-order-type");
    let order = OrderAny::TrailingStopMarket(TrailingStopMarketOrder::new(
        trader_id,
        strategy_id,
        instrument_id,
        cid,
        OrderSide::Buy,
        Quantity::from("0.01"),
        None,
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
    let init = order.init_event().clone();

    cache
        .borrow_mut()
        .add_order(order, None, Some(client_id), false)
        .unwrap();

    let cmd = SubmitOrder::new(
        trader_id,
        Some(client_id),
        strategy_id,
        instrument_id,
        cid,
        init,
        None,
        None,
        None,
        UUID4::new(),
        UnixNanos::default(),
        None, // correlation_id
    );

    client.submit_order(cmd).unwrap();

    let event = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("timed out waiting for OrderDenied")
        .expect("channel closed");
    let text = match event {
        ExecutionEvent::Order(ref order_event) => order_event.to_string(),
        other => panic!("Expected OrderDenied, was {other:?}"),
    };
    assert!(
        text.contains("OrderDenied") && text.contains("UNSUPPORTED_ORDER_TYPE"),
        "Expected OrderDenied with unsupported-order-type reason, was {text}",
    );

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_exec_client_submit_order_list_denies_incomplete_when_leg_missing() {
    use nautilus_common::messages::execution::SubmitOrderList;
    use nautilus_model::orders::OrderList;

    let (addr, state) = start_test_server().await.unwrap();
    let trader_id = TraderId::from("TESTER-001");
    let account_id = AccountId::from("BYBIT-001");
    let client_id = *BYBIT_CLIENT_ID;
    let strategy_id = StrategyId::from("S-001");
    let instrument_id = InstrumentId::new(Symbol::from("ETHUSDT-LINEAR"), *BYBIT_VENUE);

    let cache = Rc::new(RefCell::new(Cache::default()));
    add_test_account_to_cache(&cache, account_id);

    let core = ExecutionClientCore::new(
        trader_id,
        client_id,
        *BYBIT_VENUE,
        OmsType::Netting,
        account_id,
        AccountType::Margin,
        None,
        cache.clone(),
    );

    let config = create_test_demo_exec_config(addr);
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

    while tokio::time::timeout(Duration::from_millis(200), rx.recv())
        .await
        .is_ok()
    {}

    // Only the first leg is cached; the second is absent, so the whole list is incomplete
    let cid_present = ClientOrderId::from("test-incomplete-present");
    let cid_missing = ClientOrderId::from("test-incomplete-missing");

    let order_present = OrderAny::Market(MarketOrder::new(
        trader_id,
        strategy_id,
        instrument_id,
        cid_present,
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
    let order_missing = OrderAny::Market(MarketOrder::new(
        trader_id,
        strategy_id,
        instrument_id,
        cid_missing,
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

    let init_present = order_present.init_event().clone();
    let init_missing = order_missing.init_event().clone();

    cache
        .borrow_mut()
        .add_order(order_present, None, Some(client_id), false)
        .unwrap();

    let order_list = OrderList::new(
        OrderListId::from("test-incomplete-list"),
        instrument_id,
        strategy_id,
        vec![cid_present, cid_missing],
        UnixNanos::default(),
    );

    let cmd = SubmitOrderList::new(
        trader_id,
        Some(client_id),
        strategy_id,
        order_list,
        vec![init_present, init_missing],
        None,
        None,
        None,
        UUID4::new(),
        UnixNanos::default(),
        None, // correlation_id
    );

    client.submit_order_list(cmd).unwrap();

    // The missing leg cannot emit (absent from the cache); the cached leg reports the list cause
    let event = tokio::time::timeout(Duration::from_secs(2), rx.recv())
        .await
        .expect("timed out waiting for OrderDenied")
        .expect("channel closed");
    let text = match event {
        ExecutionEvent::Order(ref order_event) => order_event.to_string(),
        other => panic!("Expected OrderDenied, was {other:?}"),
    };
    assert!(
        text.contains("client_order_id=test-incomplete-present")
            && text.contains("ORDER_LIST_INCOMPLETE"),
        "Expected ORDER_LIST_INCOMPLETE for the cached leg, was {text}",
    );

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_exec_client_submit_order_list_denies_closed_leg() {
    use nautilus_common::messages::execution::SubmitOrderList;
    use nautilus_model::orders::OrderList;

    let (addr, state) = start_test_server().await.unwrap();
    let trader_id = TraderId::from("TESTER-001");
    let account_id = AccountId::from("BYBIT-001");
    let client_id = *BYBIT_CLIENT_ID;
    let strategy_id = StrategyId::from("S-001");
    let instrument_id = InstrumentId::new(Symbol::from("ETHUSDT-LINEAR"), *BYBIT_VENUE);

    let cache = Rc::new(RefCell::new(Cache::default()));
    add_test_account_to_cache(&cache, account_id);

    let core = ExecutionClientCore::new(
        trader_id,
        client_id,
        *BYBIT_VENUE,
        OmsType::Netting,
        account_id,
        AccountType::Margin,
        None,
        cache.clone(),
    );

    let config = create_test_demo_exec_config(addr);
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

    while tokio::time::timeout(Duration::from_millis(200), rx.recv())
        .await
        .is_ok()
    {}

    let cid_open = ClientOrderId::from("test-closed-leg-open");
    let cid_closed = ClientOrderId::from("test-closed-leg-closed");

    let order_open = OrderAny::Market(MarketOrder::new(
        trader_id,
        strategy_id,
        instrument_id,
        cid_open,
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
    let mut order_closed = OrderAny::Market(MarketOrder::new(
        trader_id,
        strategy_id,
        instrument_id,
        cid_closed,
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

    let init_open = order_open.init_event().clone();
    let init_closed = order_closed.init_event().clone();

    // Deny the second leg before submission so it is closed when the list is processed
    order_closed
        .apply(OrderEventAny::Denied(OrderDenied::new(
            trader_id,
            strategy_id,
            instrument_id,
            cid_closed,
            Ustr::from("closed before submission"),
            UUID4::new(),
            UnixNanos::default(),
            UnixNanos::default(),
        )))
        .unwrap();

    cache
        .borrow_mut()
        .add_order(order_open, None, Some(client_id), false)
        .unwrap();
    cache
        .borrow_mut()
        .add_order(order_closed, None, Some(client_id), false)
        .unwrap();

    let order_list = OrderList::new(
        OrderListId::from("test-closed-leg-list"),
        instrument_id,
        strategy_id,
        vec![cid_open, cid_closed],
        UnixNanos::default(),
    );

    let cmd = SubmitOrderList::new(
        trader_id,
        Some(client_id),
        strategy_id,
        order_list,
        vec![init_open, init_closed],
        None,
        None,
        None,
        UUID4::new(),
        UnixNanos::default(),
        None, // correlation_id
    );

    client.submit_order_list(cmd).unwrap();

    let mut denied = Vec::new();

    for _ in 0..2 {
        match tokio::time::timeout(Duration::from_secs(2), rx.recv()).await {
            Ok(Some(ExecutionEvent::Order(ref event)))
                if event.to_string().contains("OrderDenied") =>
            {
                denied.push(event.to_string());
            }
            _ => break,
        }
    }

    assert_eq!(denied.len(), 2, "Both legs should be denied: {denied:?}");

    let offender = denied
        .iter()
        .find(|text| text.contains("client_order_id=test-closed-leg-closed"))
        .expect("missing denied event for closed leg");
    assert!(
        offender.contains("VALIDATION_FAILED") && offender.contains("cannot submit closed order"),
        "closed-leg reason was: {offender}"
    );

    let sibling = denied
        .iter()
        .find(|text| text.contains("client_order_id=test-closed-leg-open"))
        .expect("missing denied event for open leg");
    assert!(
        sibling.contains("ORDER_LIST_DENIED"),
        "sibling reason was: {sibling}"
    );

    client.disconnect().await.unwrap();
}
