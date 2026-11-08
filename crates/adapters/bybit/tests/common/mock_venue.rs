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

//! Mock Bybit HTTP and WebSocket venue shared by execution tests and latency benchmarks.

use std::{
    collections::{HashMap, VecDeque},
    net::SocketAddr,
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
    config::BybitExecutionClientConfig,
};
use serde_json::{Value, json};

#[derive(Clone)]
pub(super) struct TestServerState {
    pub(super) ws_connection_count: Arc<tokio::sync::Mutex<usize>>,
    pub(super) private_ws_connections: Arc<AtomicUsize>,
    pub(super) trade_ws_connections: Arc<AtomicUsize>,
    pub(super) trade_order_ret_code: Arc<AtomicUsize>,
    pub(super) trade_order_header: Option<Value>,
    pub(super) trade_batch_ret_code: Arc<AtomicUsize>,
    pub(super) trade_order_requests: Arc<AtomicUsize>,
    pub(super) trade_order_payloads: Arc<tokio::sync::Mutex<Vec<Value>>>,
    pub(super) amend_order_payloads: Arc<tokio::sync::Mutex<Vec<Value>>>,
    pub(super) trade_order_req_id_present: Arc<AtomicBool>,
    pub(super) authenticated: Arc<AtomicBool>,
    pub(super) fee_rate_requests: Arc<AtomicUsize>,
    pub(super) subscriptions: Arc<tokio::sync::Mutex<Vec<String>>>,
    pub(super) disconnect_trigger: Arc<AtomicBool>,
    pub(super) reject_trade_websocket: Arc<AtomicBool>,
    pub(super) empty_orders_realtime: Arc<AtomicBool>,
    pub(super) rejected_orders_realtime: Arc<AtomicBool>,
    pub(super) cancel_all_delayed_reports: Arc<AtomicBool>,
    pub(super) cancel_all_partial_fills: Arc<AtomicBool>,
    pub(super) cancel_all_history: Arc<AtomicBool>,
    pub(super) orders_realtime_requests: Arc<AtomicUsize>,
    pub(super) cancel_order_ret_code: Arc<AtomicUsize>,
    pub(super) cancel_order_requests: Arc<AtomicUsize>,
    pub(super) cancel_order_reports: Arc<tokio::sync::Mutex<HashMap<String, VecDeque<Value>>>>,
    pub(super) orders_history_requests: Arc<AtomicUsize>,
    pub(super) cancel_all_requests: Arc<AtomicUsize>,
    pub(super) execution_response: Arc<tokio::sync::Mutex<Option<Value>>>,
    pub(super) subscription_execution: Arc<tokio::sync::Mutex<Option<Value>>>,
    pub(super) position_requests: Arc<AtomicUsize>,
    pub(super) wallet_balance_requests: Arc<AtomicUsize>,
    pub(super) repay_requests: Arc<AtomicUsize>,
    pub(super) borrow_bodies: Arc<tokio::sync::Mutex<Vec<Value>>>,
    pub(super) no_convert_repay_bodies: Arc<tokio::sync::Mutex<Vec<Value>>>,
    pub(super) no_convert_result_status: Arc<tokio::sync::Mutex<&'static str>>,
    pub(super) ping_count: Arc<AtomicUsize>,
    pub(super) switch_mode_requests: Arc<tokio::sync::Mutex<Vec<Value>>>,
    pub(super) set_leverage_requests: Arc<tokio::sync::Mutex<Vec<Value>>>,
    pub(super) set_margin_mode_requests: Arc<tokio::sync::Mutex<Vec<Value>>>,
}

impl Default for TestServerState {
    fn default() -> Self {
        Self {
            ws_connection_count: Arc::new(tokio::sync::Mutex::new(0)),
            private_ws_connections: Arc::new(AtomicUsize::new(0)),
            trade_ws_connections: Arc::new(AtomicUsize::new(0)),
            trade_order_ret_code: Arc::new(AtomicUsize::new(0)),
            trade_order_header: None,
            trade_batch_ret_code: Arc::new(AtomicUsize::new(0)),
            trade_order_requests: Arc::new(AtomicUsize::new(0)),
            trade_order_payloads: Arc::new(tokio::sync::Mutex::new(Vec::new())),
            amend_order_payloads: Arc::default(),
            trade_order_req_id_present: Arc::new(AtomicBool::new(false)),
            authenticated: Arc::new(AtomicBool::new(false)),
            fee_rate_requests: Arc::new(AtomicUsize::new(0)),
            subscriptions: Arc::new(tokio::sync::Mutex::new(Vec::new())),
            disconnect_trigger: Arc::new(AtomicBool::new(false)),
            reject_trade_websocket: Arc::new(AtomicBool::new(false)),
            empty_orders_realtime: Arc::new(AtomicBool::new(false)),
            rejected_orders_realtime: Arc::new(AtomicBool::new(false)),
            cancel_all_delayed_reports: Arc::new(AtomicBool::new(false)),
            cancel_all_partial_fills: Arc::new(AtomicBool::new(false)),
            cancel_all_history: Arc::new(AtomicBool::new(false)),
            orders_realtime_requests: Arc::new(AtomicUsize::new(0)),
            cancel_order_ret_code: Arc::new(AtomicUsize::new(0)),
            cancel_order_requests: Arc::new(AtomicUsize::new(0)),
            cancel_order_reports: Arc::default(),
            orders_history_requests: Arc::default(),
            cancel_all_requests: Arc::new(AtomicUsize::new(0)),
            execution_response: Arc::default(),
            subscription_execution: Arc::default(),
            position_requests: Arc::new(AtomicUsize::new(0)),
            wallet_balance_requests: Arc::new(AtomicUsize::new(0)),
            repay_requests: Arc::new(AtomicUsize::new(0)),
            borrow_bodies: Arc::new(tokio::sync::Mutex::new(Vec::new())),
            no_convert_repay_bodies: Arc::new(tokio::sync::Mutex::new(Vec::new())),
            no_convert_result_status: Arc::new(tokio::sync::Mutex::new("SU")),
            ping_count: Arc::new(AtomicUsize::new(0)),
            switch_mode_requests: Arc::new(tokio::sync::Mutex::new(Vec::new())),
            set_leverage_requests: Arc::new(tokio::sync::Mutex::new(Vec::new())),
            set_margin_mode_requests: Arc::new(tokio::sync::Mutex::new(Vec::new())),
        }
    }
}

pub(super) fn load_test_data(filename: &str) -> Value {
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

    let mut instruments = load_test_data(filename);
    if category == Some("linear") {
        let list = instruments["result"]["list"].as_array_mut().unwrap();
        let mut sol = list[1].clone();
        sol["symbol"] = json!("SOLUSDT");
        sol["baseCoin"] = json!("SOL");
        list.push(sol);
    }

    Json(instruments).into_response()
}

async fn handle_get_fee_rate(
    State(state): State<TestServerState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    state.fee_rate_requests.fetch_add(1, Ordering::Relaxed);

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

async fn handle_get_executions(
    State(state): State<TestServerState>,
    headers: HeaderMap,
) -> Response {
    if let Some(response) = state.execution_response.lock().await.as_ref() {
        return Json(response.clone()).into_response();
    }

    handle_get_empty_report_list(headers).await.into_response()
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
    Query(query): Query<HashMap<String, String>>,
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

    if let Some(response) = state
        .cancel_order_reports
        .lock()
        .await
        .get_mut(query.get("orderId").map_or("", String::as_str))
        .and_then(VecDeque::pop_front)
    {
        state
            .orders_realtime_requests
            .fetch_add(1, Ordering::Relaxed);
        return Json(response).into_response();
    }

    if state.cancel_all_delayed_reports.load(Ordering::Relaxed) {
        let request = state
            .orders_realtime_requests
            .fetch_add(1, Ordering::Relaxed);
        let mut orders = load_test_data("http_get_orders_realtime.json");
        let list = orders["result"]["list"].as_array_mut().unwrap();
        list.retain(|order| {
            query
                .get("orderId")
                .is_some_and(|id| order["orderId"] == *id)
        });

        if request > 0 {
            for order in list {
                order["orderStatus"] = json!("Cancelled");
                order["cancelType"] = json!("CancelByUser");
                order["updatedTime"] = json!("1761355906606");
                if state.cancel_all_partial_fills.load(Ordering::Relaxed) {
                    order["orderStatus"] = json!("PartiallyFilledCanceled");
                    order["cumExecQty"] = json!(if order["orderId"] == "open-order-1" {
                        "0.003"
                    } else {
                        "0.004"
                    });
                    order["avgPrice"] = order["price"].clone();
                }
            }
        }

        return Json(orders).into_response();
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

async fn handle_cancel_order(
    State(state): State<TestServerState>,
    headers: HeaderMap,
    body: Bytes,
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

    let request: Value = serde_json::from_slice(&body).unwrap();
    let order_id = request["orderId"].as_str().unwrap_or("test-order-id-12345");

    state.cancel_order_requests.fetch_add(1, Ordering::Relaxed);

    let ret_code = state.cancel_order_ret_code.load(Ordering::Relaxed);

    if ret_code != 0 {
        return Json(json!({
            "retCode": ret_code,
            "retMsg": format!("Test venue rejection {ret_code}"),
            "result": {},
            "time": 1704470400123i64
        }))
        .into_response();
    }

    Json(json!({
        "retCode": 0,
        "retMsg": "OK",
        "result": {
            "orderId": order_id,
            "orderLinkId": "test-order"
        },
        "retExtInfo": {},
        "time": 1704470400123i64
    }))
    .into_response()
}

async fn handle_amend_order(
    State(state): State<TestServerState>,
    headers: HeaderMap,
    body: Bytes,
) -> StatusCode {
    assert!(has_auth_headers(&headers));
    let payload: Value = serde_json::from_slice(&body).unwrap();
    state.amend_order_payloads.lock().await.push(payload);
    StatusCode::NOT_FOUND
}

async fn handle_cancel_all_orders(
    State(state): State<TestServerState>,
    headers: HeaderMap,
    _body: Bytes,
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

    state.cancel_all_requests.fetch_add(1, Ordering::Relaxed);

    Json(json!({
        "retCode": 0,
        "retMsg": "OK",
        "result": { "list": [] },
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

async fn handle_post_repay(State(state): State<TestServerState>) -> impl IntoResponse {
    state.repay_requests.fetch_add(1, Ordering::Relaxed);
    Json(json!({
        "retCode": 0,
        "retMsg": "OK",
        "result": {"resultStatus": "SU"},
        "retExtInfo": {},
        "time": 1704470400123i64
    }))
}

async fn handle_post_borrow(
    State(state): State<TestServerState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
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

    let params: Value = serde_json::from_slice(&body).expect("valid borrow body");
    let coin = params["coin"].as_str().unwrap_or_default().to_string();
    state.borrow_bodies.lock().await.push(params);
    Json(json!({
        "retCode": 0,
        "retMsg": "OK",
        "result": {"coin": coin, "amount": "0"},
        "retExtInfo": {},
        "time": 1704470400123i64
    }))
    .into_response()
}

async fn handle_post_no_convert_repay(
    State(state): State<TestServerState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
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

    let params: Value = serde_json::from_slice(&body).expect("valid no-convert repay body");
    state.no_convert_repay_bodies.lock().await.push(params);
    let result_status = *state.no_convert_result_status.lock().await;
    Json(json!({
        "retCode": 0,
        "retMsg": "OK",
        "result": {"resultStatus": result_status},
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
    if state.reject_trade_websocket.load(Ordering::Relaxed) {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }

    if state.subscription_execution.lock().await.is_some() {
        tokio::time::sleep(Duration::from_millis(200)).await;
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
                if !handle_text_message(&mut socket, &state, &text).await {
                    break;
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

pub(super) async fn handle_text_message(
    socket: &mut WebSocket,
    state: &TestServerState,
    text: &str,
) -> bool {
    let Ok(value) = serde_json::from_str::<Value>(text) else {
        return true;
    };

    let op = value.get("op").and_then(|v| v.as_str());
    match op {
        Some("ping") => handle_ping_message(socket, state, &value).await,
        Some("auth") => handle_auth_message(socket, state, &value).await,
        Some("subscribe") => handle_subscription_message(socket, state, &value).await,
        Some("order.create" | "order.amend" | "order.cancel" | "order.amend-batch") => {
            handle_order_message(socket, state, op.unwrap(), &value).await
        }
        _ => true,
    }
}

async fn handle_ping_message(
    socket: &mut WebSocket,
    state: &TestServerState,
    value: &Value,
) -> bool {
    state.ping_count.fetch_add(1, Ordering::Relaxed);
    let response = json!({
        "success": true,
        "ret_msg": "pong",
        "conn_id": "test-conn-id",
        "req_id": value.get("req_id").and_then(|v| v.as_str()).unwrap_or(""),
        "op": "pong"
    });
    socket
        .send(Message::Text(response.to_string().into()))
        .await
        .is_ok()
}

async fn handle_auth_message(
    socket: &mut WebSocket,
    state: &TestServerState,
    value: &Value,
) -> bool {
    let api_key = value
        .get("args")
        .and_then(|a| a.as_array())
        .and_then(|arr| arr.first())
        .and_then(|v| v.as_str());
    let authenticated = api_key == Some("test_api_key");
    if authenticated {
        state.authenticated.store(true, Ordering::Relaxed);
    }

    let response = if authenticated {
        json!({
            "success": true,
            "ret_msg": "",
            "op": "auth",
            "conn_id": "test-conn-id"
        })
    } else {
        json!({
            "success": false,
            "ret_msg": "Invalid API key",
            "op": "auth",
            "conn_id": "test-conn-id"
        })
    };

    socket
        .send(Message::Text(response.to_string().into()))
        .await
        .is_ok()
}

async fn handle_subscription_message(
    socket: &mut WebSocket,
    state: &TestServerState,
    value: &Value,
) -> bool {
    let args = value.get("args").and_then(|a| a.as_array());

    let execution_topic = args.is_some_and(|topics| {
        topics
            .iter()
            .any(|topic| topic.as_str() == Some("execution"))
    });

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

    let response = json!({
        "success": true,
        "ret_msg": "",
        "conn_id": "test-conn-id",
        "req_id": value.get("req_id").and_then(|v| v.as_str()).unwrap_or(""),
        "op": "subscribe"
    });

    if socket
        .send(Message::Text(response.to_string().into()))
        .await
        .is_err()
    {
        return false;
    }

    if execution_topic {
        let execution = state.subscription_execution.lock().await.clone();
        if let Some(execution) = execution
            && socket
                .send(Message::Text(execution.to_string().into()))
                .await
                .is_err()
        {
            return false;
        }
    }

    true
}

pub(super) async fn handle_order_message(
    socket: &mut WebSocket,
    state: &TestServerState,
    op: &str,
    value: &Value,
) -> bool {
    state.trade_order_requests.fetch_add(1, Ordering::Relaxed);
    state.trade_order_payloads.lock().await.push(value.clone());
    let req_id = value.get("reqId").and_then(|v| v.as_str());
    state.trade_order_req_id_present.store(
        req_id.is_some_and(|req_id| !req_id.is_empty()),
        Ordering::Relaxed,
    );

    let ret_code = if op == "order.amend-batch" {
        state.trade_batch_ret_code.load(Ordering::Relaxed)
    } else {
        state.trade_order_ret_code.load(Ordering::Relaxed)
    };

    let ret_msg = if ret_code == 0 {
        "OK"
    } else {
        "Too many visits."
    };

    let mut response = json!({
        "retCode": ret_code,
        "retMsg": ret_msg,
        "data": {},
        "retExtInfo": {},
        "connId": "test-conn-id",
        "reqId": req_id.unwrap_or(""),
        "op": op
    });

    if let Some(header) = &state.trade_order_header {
        response["header"] = header.clone();
    }

    socket
        .send(Message::Text(response.to_string().into()))
        .await
        .is_ok()
}

pub(super) fn create_test_router(state: TestServerState) -> Router {
    Router::new()
        .route(
            "/v5/user/query-api",
            get(|| async { Json(load_test_data("http_get_user_query_api.json")) }),
        )
        .route("/v5/market/instruments-info", get(handle_get_instruments))
        .route("/v5/account/fee-rate", get(handle_get_fee_rate))
        .route("/v5/account/wallet-balance", get(handle_get_wallet_balance))
        .route("/v5/position/list", get(handle_get_positions))
        .route("/v5/order/realtime", get(handle_get_orders_realtime))
        .route("/v5/order/history", get(handle_get_orders_history))
        .route("/v5/execution/list", get(handle_get_executions))
        .route("/v5/order/create", post(handle_post_order))
        .route("/v5/order/amend", post(handle_amend_order))
        .route("/v5/order/cancel", post(handle_cancel_order))
        .route("/v5/order/cancel-all", post(handle_cancel_all_orders))
        .route("/v5/position/switch-mode", post(handle_switch_mode))
        .route("/v5/position/set-leverage", post(handle_set_leverage))
        .route("/v5/account/set-margin-mode", post(handle_set_margin_mode))
        .route("/v5/account/repay", post(handle_post_repay))
        .route("/v5/account/borrow", post(handle_post_borrow))
        .route(
            "/v5/account/no-convert-repay",
            post(handle_post_no_convert_repay),
        )
        .route("/v3/public/time", get(handle_server_time))
        .route("/v5/private", get(handle_private_websocket))
        .route("/v5/trade", get(handle_trade_websocket))
        .with_state(state)
}

async fn handle_get_orders_history(
    State(state): State<TestServerState>,
    headers: HeaderMap,
) -> axum::response::Response {
    state
        .orders_history_requests
        .fetch_add(1, Ordering::Relaxed);

    if !has_auth_headers(&headers) {
        return handle_get_empty_report_list(headers).await.into_response();
    }

    if !state.cancel_all_history.load(Ordering::Relaxed) {
        return Json(load_test_data("http_get_orders_history_empty.json")).into_response();
    }

    let mut orders = load_test_data("http_get_orders_realtime.json");
    let list = orders["result"]["list"].as_array_mut().unwrap();

    for order in list.iter_mut() {
        order["orderStatus"] = json!("Cancelled");
        order["cancelType"] = json!("CancelByUser");
        order["updatedTime"] = json!("1761355906606");
    }

    list[0]["orderLinkId"] = json!("");
    let mut unrelated = list[1].clone();
    unrelated["orderId"] = json!("unrelated-order");
    unrelated["orderLinkId"] = json!("unrelated-client");
    list.insert(0, unrelated);
    Json(orders).into_response()
}

pub(super) fn create_test_exec_config(addr: SocketAddr) -> BybitExecutionClientConfig {
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
