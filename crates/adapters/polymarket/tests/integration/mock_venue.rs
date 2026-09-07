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

//! In-process Polymarket venue used by execution integration tests.

use std::{
    collections::{HashMap, HashSet, VecDeque},
    net::SocketAddr,
    path::PathBuf,
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
    http::{HeaderMap, StatusCode, Uri},
    response::{IntoResponse, Json, Response},
    routing::{delete, get, post},
};
use futures_util::StreamExt;
use nautilus_common::testing::wait_until_async;
use nautilus_network::http::HttpClient;
use nautilus_polymarket::{
    config::PolymarketExecutionClientConfig, http::models::PolymarketOrder,
    signing::eip712::order_hash,
};
use serde_json::{Value, json};

pub(super) const DEFAULT_ACCEPTED_ORDER_ID: &str =
    "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef12";
pub(super) const TEST_API_SECRET_B64: &str = "dGVzdF9zZWNyZXRfa2V5XzMyYnl0ZXNfcGFkMTIzNDU=";
pub(super) const TEST_CONDITION_ID: &str =
    "0xdd22472e552920b8438158ea7238bfadfa4f736aa4cee91a6b86c39ead110917";
pub(super) const TEST_PRIVATE_KEY: &str =
    "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef";
pub(super) const TEST_SIGNER_ADDRESS: &str = "0x1be31a94361a391bbafb2a4ccd704f57dc04d4bb";
pub(super) const TEST_TOKEN_ID: &str =
    "71321045679252212594626385532706912750332728571942532289631379312455583992563";

pub(super) fn execution_config(addr: SocketAddr) -> PolymarketExecutionClientConfig {
    execution_config_with_retries(addr, 0)
}

pub(super) fn execution_config_with_retries(
    addr: SocketAddr,
    max_retries: u32,
) -> PolymarketExecutionClientConfig {
    PolymarketExecutionClientConfig {
        private_key: Some(TEST_PRIVATE_KEY.into()),
        api_key: Some("00000000-0000-0000-0000-000000000001".into()),
        api_secret: Some(TEST_API_SECRET_B64.into()),
        passphrase: Some("test_pass".into()),
        funder: None,
        base_url_http: Some(format!("http://{addr}")),
        base_url_ws: Some(format!("ws://{addr}/ws")),
        base_url_data_api: Some(format!("http://{addr}")),
        http_timeout_secs: 5,
        max_retries,
        retry_delay_initial_ms: 1,
        retry_delay_max_ms: 10,
        ..PolymarketExecutionClientConfig::default()
    }
}

#[derive(Debug)]
pub(super) struct RequestGate {
    enabled: AtomicBool,
    started: AtomicUsize,
    permits: tokio::sync::Semaphore,
}

impl Default for RequestGate {
    fn default() -> Self {
        Self {
            enabled: AtomicBool::new(false),
            started: AtomicUsize::new(0),
            permits: tokio::sync::Semaphore::new(0),
        }
    }
}

impl RequestGate {
    pub(super) fn enable(&self) {
        self.enabled.store(true, Ordering::Release);
    }

    pub(super) fn release(&self) {
        self.permits.add_permits(1);
    }

    pub(super) fn started(&self) -> usize {
        self.started.load(Ordering::Acquire)
    }

    async fn wait(&self) {
        if !self.enabled.load(Ordering::Acquire) {
            return;
        }

        self.started.fetch_add(1, Ordering::AcqRel);
        self.permits
            .acquire()
            .await
            .expect("request gate should remain open")
            .forget();
    }
}

#[derive(Clone)]
pub(super) struct TestServerState {
    pub(super) last_body: Arc<tokio::sync::Mutex<Option<Value>>>,
    pub(super) last_headers: Arc<tokio::sync::Mutex<HashMap<String, String>>>,
    pub(super) last_path: Arc<tokio::sync::Mutex<String>>,
    pub(super) last_query: Arc<tokio::sync::Mutex<HashMap<String, String>>>,
    pub(super) gamma_response: Arc<tokio::sync::Mutex<Option<Value>>>,
    pub(super) version_response: Arc<tokio::sync::Mutex<Value>>,
    pub(super) version_response_status: Arc<tokio::sync::Mutex<StatusCode>>,
    pub(super) startup_request_paths: Arc<tokio::sync::Mutex<Vec<String>>>,
    pub(super) balance_response_status: Arc<tokio::sync::Mutex<StatusCode>>,
    pub(super) balance_response: Arc<tokio::sync::Mutex<Value>>,
    pub(super) order_response: Arc<tokio::sync::Mutex<Option<Value>>>,
    pub(super) order_response_status: Arc<tokio::sync::Mutex<StatusCode>>,
    pub(super) order_response_headers: Arc<tokio::sync::Mutex<HeaderMap>>,
    pub(super) order_post_count: Arc<tokio::sync::Mutex<usize>>,
    /// When > 0, `handle_post_order` returns 500 on this many calls before
    /// reverting to the configured `order_response_status`. Used by retry tests.
    pub(super) order_post_500_remaining: Arc<tokio::sync::Mutex<usize>>,
    pub(super) order_response_uses_request_hash: Arc<AtomicBool>,
    pub(super) batch_order_response: Arc<tokio::sync::Mutex<Option<Value>>>,
    pub(super) batch_order_response_status: Arc<tokio::sync::Mutex<StatusCode>>,
    pub(super) batch_order_post_count: Arc<tokio::sync::Mutex<usize>>,
    pub(super) fee_rate_response: Arc<tokio::sync::Mutex<Option<Value>>>,
    pub(super) fee_rate_response_status: Arc<tokio::sync::Mutex<StatusCode>>,
    pub(super) fee_rate_fetch_count: Arc<tokio::sync::Mutex<usize>>,
    pub(super) fee_rate_overrides: Arc<tokio::sync::Mutex<HashMap<String, (StatusCode, Value)>>>,
    pub(super) heartbeat_response: Arc<tokio::sync::Mutex<Value>>,
    pub(super) heartbeat_response_status: Arc<tokio::sync::Mutex<StatusCode>>,
    pub(super) heartbeat_response_statuses: Arc<tokio::sync::Mutex<VecDeque<StatusCode>>>,
    pub(super) heartbeat_response_headers: Arc<tokio::sync::Mutex<HeaderMap>>,
    pub(super) heartbeat_post_count: Arc<AtomicUsize>,
    pub(super) heartbeat_post_times: Arc<tokio::sync::Mutex<Vec<tokio::time::Instant>>>,
    pub(super) heartbeat_resynchronize_remaining: Arc<AtomicUsize>,
    pub(super) heartbeat_request_gate: Arc<RequestGate>,
    pub(super) cancel_response: Arc<tokio::sync::Mutex<Option<Value>>>,
    pub(super) cancel_response_status: Arc<tokio::sync::Mutex<StatusCode>>,
    pub(super) cancel_delete_count: Arc<tokio::sync::Mutex<usize>>,
    pub(super) cancel_request_gate: Arc<RequestGate>,
    pub(super) batch_cancel_response: Arc<tokio::sync::Mutex<Option<Value>>>,
    pub(super) batch_cancel_response_status: Arc<tokio::sync::Mutex<StatusCode>>,
    pub(super) batch_cancel_response_statuses: Arc<tokio::sync::Mutex<VecDeque<StatusCode>>>,
    pub(super) batch_cancel_response_headers: Arc<tokio::sync::Mutex<VecDeque<HeaderMap>>>,
    pub(super) batch_cancel_bodies: Arc<tokio::sync::Mutex<Vec<Value>>>,
    pub(super) batch_cancel_echo_rejections: Arc<AtomicBool>,
    pub(super) batch_cancel_delete_count: Arc<tokio::sync::Mutex<usize>>,
    pub(super) batch_cancel_request_gate: Arc<RequestGate>,
    pub(super) market_cancel_response: Arc<tokio::sync::Mutex<Option<Value>>>,
    pub(super) market_cancel_response_status: Arc<tokio::sync::Mutex<StatusCode>>,
    pub(super) market_cancel_delete_count: Arc<tokio::sync::Mutex<usize>>,
    pub(super) market_cancel_request_gate: Arc<RequestGate>,
    pub(super) order_request_gate: Arc<RequestGate>,
    pub(super) batch_order_request_gate: Arc<RequestGate>,
    pub(super) open_order_ids: Arc<tokio::sync::Mutex<HashSet<String>>>,
    pub(super) orders_response_override: Arc<tokio::sync::Mutex<Option<Value>>>,
    pub(super) orders_response_status: Arc<tokio::sync::Mutex<StatusCode>>,
    pub(super) orders_get_count: Arc<AtomicUsize>,
    pub(super) book_response: Arc<tokio::sync::Mutex<Option<Value>>>,
    pub(super) single_order_responses: Arc<tokio::sync::Mutex<VecDeque<Value>>>,
    pub(super) single_order_response: Arc<tokio::sync::Mutex<Option<Value>>>,
    pub(super) single_order_get_count: Arc<AtomicUsize>,
    pub(super) trades_response_override: Arc<tokio::sync::Mutex<Option<Value>>>,
    pub(super) positions_response_override: Arc<tokio::sync::Mutex<Option<Value>>>,
    pub(super) user_frames: tokio::sync::broadcast::Sender<String>,
    pub(super) user_socket_count: Arc<AtomicUsize>,
}

impl Default for TestServerState {
    fn default() -> Self {
        let (user_frames, _) = tokio::sync::broadcast::channel(32);

        Self {
            last_body: Arc::new(tokio::sync::Mutex::new(None)),
            last_headers: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            last_path: Arc::new(tokio::sync::Mutex::new(String::new())),
            last_query: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            gamma_response: Arc::new(tokio::sync::Mutex::new(None)),
            version_response: Arc::new(tokio::sync::Mutex::new(load_json(
                "http_version_response.json",
            ))),
            version_response_status: Arc::new(tokio::sync::Mutex::new(StatusCode::OK)),
            startup_request_paths: Arc::new(tokio::sync::Mutex::new(Vec::new())),
            balance_response_status: Arc::new(tokio::sync::Mutex::new(StatusCode::OK)),
            balance_response: Arc::new(tokio::sync::Mutex::new(load_json(
                "http_balance_allowance_collateral.json",
            ))),
            order_response: Arc::new(tokio::sync::Mutex::new(None)),
            order_response_status: Arc::new(tokio::sync::Mutex::new(StatusCode::OK)),
            order_response_headers: Arc::new(tokio::sync::Mutex::new(HeaderMap::new())),
            order_post_count: Arc::new(tokio::sync::Mutex::new(0)),
            order_post_500_remaining: Arc::new(tokio::sync::Mutex::new(0)),
            order_response_uses_request_hash: Arc::new(AtomicBool::new(false)),
            batch_order_response: Arc::new(tokio::sync::Mutex::new(None)),
            batch_order_response_status: Arc::new(tokio::sync::Mutex::new(StatusCode::OK)),
            batch_order_post_count: Arc::new(tokio::sync::Mutex::new(0)),
            fee_rate_response: Arc::new(tokio::sync::Mutex::new(None)),
            fee_rate_response_status: Arc::new(tokio::sync::Mutex::new(StatusCode::OK)),
            fee_rate_fetch_count: Arc::new(tokio::sync::Mutex::new(0)),
            fee_rate_overrides: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            heartbeat_response: Arc::new(tokio::sync::Mutex::new(json!({
                "heartbeat_id": "heartbeat-next",
            }))),
            heartbeat_response_status: Arc::new(tokio::sync::Mutex::new(StatusCode::OK)),
            heartbeat_response_statuses: Arc::new(tokio::sync::Mutex::new(VecDeque::new())),
            heartbeat_response_headers: Arc::new(tokio::sync::Mutex::new(HeaderMap::new())),
            heartbeat_post_count: Arc::new(AtomicUsize::new(0)),
            heartbeat_post_times: Arc::new(tokio::sync::Mutex::new(Vec::new())),
            heartbeat_resynchronize_remaining: Arc::new(AtomicUsize::new(0)),
            heartbeat_request_gate: Arc::new(RequestGate::default()),
            cancel_response: Arc::new(tokio::sync::Mutex::new(None)),
            cancel_response_status: Arc::new(tokio::sync::Mutex::new(StatusCode::OK)),
            cancel_delete_count: Arc::new(tokio::sync::Mutex::new(0)),
            cancel_request_gate: Arc::new(RequestGate::default()),
            batch_cancel_response: Arc::new(tokio::sync::Mutex::new(None)),
            batch_cancel_response_status: Arc::new(tokio::sync::Mutex::new(StatusCode::OK)),
            batch_cancel_response_statuses: Arc::new(tokio::sync::Mutex::new(VecDeque::new())),
            batch_cancel_response_headers: Arc::new(tokio::sync::Mutex::new(VecDeque::new())),
            batch_cancel_bodies: Arc::new(tokio::sync::Mutex::new(Vec::new())),
            batch_cancel_echo_rejections: Arc::new(AtomicBool::new(false)),
            batch_cancel_delete_count: Arc::new(tokio::sync::Mutex::new(0)),
            batch_cancel_request_gate: Arc::new(RequestGate::default()),
            market_cancel_response: Arc::new(tokio::sync::Mutex::new(None)),
            market_cancel_response_status: Arc::new(tokio::sync::Mutex::new(StatusCode::OK)),
            market_cancel_delete_count: Arc::new(tokio::sync::Mutex::new(0)),
            market_cancel_request_gate: Arc::new(RequestGate::default()),
            order_request_gate: Arc::new(RequestGate::default()),
            batch_order_request_gate: Arc::new(RequestGate::default()),
            open_order_ids: Arc::new(tokio::sync::Mutex::new(HashSet::new())),
            orders_response_override: Arc::new(tokio::sync::Mutex::new(None)),
            orders_response_status: Arc::new(tokio::sync::Mutex::new(StatusCode::OK)),
            orders_get_count: Arc::new(AtomicUsize::new(0)),
            single_order_responses: Arc::new(tokio::sync::Mutex::new(VecDeque::new())),
            single_order_response: Arc::new(tokio::sync::Mutex::new(None)),
            single_order_get_count: Arc::new(AtomicUsize::new(0)),
            trades_response_override: Arc::new(tokio::sync::Mutex::new(None)),
            positions_response_override: Arc::new(tokio::sync::Mutex::new(None)),
            book_response: Arc::new(tokio::sync::Mutex::new(Some(json!({
                "bids": [
                    {"price": "0.48", "size": "100.00"},
                    {"price": "0.49", "size": "200.00"},
                    {"price": "0.50", "size": "150.00"}
                ],
                "asks": [
                    {"price": "0.51", "size": "120.00"},
                    {"price": "0.52", "size": "80.00"},
                    {"price": "0.53", "size": "90.00"}
                ]
            })))),
            user_frames,
            user_socket_count: Arc::new(AtomicUsize::new(0)),
        }
    }
}

impl TestServerState {
    pub(super) async fn configure_default_order_success(&self) {
        self.balance_response.lock().await["balance"] = Value::String("1000000000".to_string());

        let mut order_response = load_json("http_order_response_ok.json");
        order_response["orderID"] = Value::String(DEFAULT_ACCEPTED_ORDER_ID.to_string());
        order_response["status"] = Value::String("live".to_string());
        *self.order_response.lock().await = Some(order_response);

        let mut cancel_response = load_json("http_cancel_response_ok.json");
        cancel_response["canceled"][0] = Value::String(DEFAULT_ACCEPTED_ORDER_ID.to_string());
        *self.cancel_response.lock().await = Some(cancel_response);
    }

    pub(super) async fn send_user(&self, message: Value) {
        wait_until_async(
            || async { self.user_socket_count.load(Ordering::Acquire) > 0 },
            Duration::from_secs(5),
        )
        .await;
        self.user_frames
            .send(message.to_string())
            .expect("user WebSocket should be connected");
    }

    pub(super) async fn feed_user(&self, filename: &str) {
        let mut message = load_json(filename);
        let event_type = if message.get("type").and_then(Value::as_str) == Some("TRADE") {
            "trade"
        } else {
            "order"
        };
        message["event_type"] = Value::String(event_type.to_string());
        self.send_user(message).await;
    }
}

fn data_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test_data")
}

pub(super) fn load_json(filename: &str) -> Value {
    let content = std::fs::read_to_string(data_path().join(filename))
        .unwrap_or_else(|_| panic!("failed to read {filename}"));
    serde_json::from_str(&content).expect("invalid json")
}

async fn handle_get_orders(
    State(state): State<TestServerState>,
    uri: Uri,
    Query(query): Query<HashMap<String, String>>,
) -> Response {
    *state.last_path.lock().await = uri.path().to_string();
    *state.last_query.lock().await = query;
    state.orders_get_count.fetch_add(1, Ordering::AcqRel);
    let status = *state.orders_response_status.lock().await;
    let body = state
        .orders_response_override
        .lock()
        .await
        .clone()
        .unwrap_or_else(|| load_json("http_open_orders_page.json"));
    (status, Json(body)).into_response()
}

async fn handle_get_order(State(state): State<TestServerState>, uri: Uri) -> Response {
    *state.last_path.lock().await = uri.path().to_string();
    state.single_order_get_count.fetch_add(1, Ordering::AcqRel);
    if let Some(resp) = state.single_order_responses.lock().await.pop_front() {
        return Json(resp).into_response();
    }

    let resp = state.single_order_response.lock().await;
    match resp.as_ref() {
        Some(v) => Json(v.clone()).into_response(),
        None => Json(load_json("http_open_order.json")).into_response(),
    }
}

async fn handle_get_trades(
    State(state): State<TestServerState>,
    uri: Uri,
    Query(query): Query<HashMap<String, String>>,
) -> Response {
    *state.last_path.lock().await = uri.path().to_string();
    *state.last_query.lock().await = query;
    if let Some(override_value) = state.trades_response_override.lock().await.as_ref() {
        return Json(override_value.clone()).into_response();
    }
    Json(load_json("http_trades_page.json")).into_response()
}

async fn handle_get_balance(
    State(state): State<TestServerState>,
    uri: Uri,
    headers: HeaderMap,
) -> Response {
    *state.last_path.lock().await = uri.path().to_string();
    state
        .startup_request_paths
        .lock()
        .await
        .push(uri.path().to_string());
    *state.last_headers.lock().await = headers
        .iter()
        .map(|(k, v)| (k.as_str().to_string(), v.to_str().unwrap_or("").to_string()))
        .collect();
    let status = *state.balance_response_status.lock().await;
    let response = state.balance_response.lock().await.clone();
    (status, Json(response)).into_response()
}

async fn handle_get_version(State(state): State<TestServerState>, uri: Uri) -> Response {
    *state.last_path.lock().await = uri.path().to_string();
    state
        .startup_request_paths
        .lock()
        .await
        .push(uri.path().to_string());
    let status = *state.version_response_status.lock().await;
    let response = state.version_response.lock().await.clone();
    (status, Json(response)).into_response()
}

async fn handle_post_order(
    State(state): State<TestServerState>,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    *state.last_path.lock().await = uri.path().to_string();
    *state.last_headers.lock().await = headers
        .iter()
        .map(|(k, v)| (k.as_str().to_string(), v.to_str().unwrap_or("").to_string()))
        .collect();
    *state.order_post_count.lock().await += 1;

    let request = serde_json::from_slice::<Value>(&body).ok();

    if let Some(request) = &request {
        *state.last_body.lock().await = Some(request.clone());
    }

    state.order_request_gate.wait().await;

    let mut remaining_500 = state.order_post_500_remaining.lock().await;
    if *remaining_500 > 0 {
        *remaining_500 -= 1;
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": "transient server error"})),
        )
            .into_response();
    }
    drop(remaining_500);

    let status = *state.order_response_status.lock().await;
    let resp = state.order_response.lock().await;
    let mut body = resp
        .clone()
        .unwrap_or_else(|| load_json("http_order_response_ok.json"));

    if state
        .order_response_uses_request_hash
        .load(Ordering::Acquire)
    {
        let signed_order: PolymarketOrder = serde_json::from_value(
            request
                .as_ref()
                .and_then(|request| request.get("order"))
                .cloned()
                .expect("order request body"),
        )
        .expect("valid signed order");
        body["orderID"] = Value::String(format!(
            "{:#x}",
            order_hash(&signed_order, false).expect("valid order hash")
        ));
    }

    record_open_order_ids(&state, std::slice::from_ref(&body)).await;
    let mut response = (status, Json(body)).into_response();
    response
        .headers_mut()
        .extend(state.order_response_headers.lock().await.clone());
    response
}

async fn handle_post_orders(
    State(state): State<TestServerState>,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    *state.last_path.lock().await = uri.path().to_string();
    *state.last_headers.lock().await = headers
        .iter()
        .map(|(k, v)| (k.as_str().to_string(), v.to_str().unwrap_or("").to_string()))
        .collect();
    let post_count = {
        let mut count = state.batch_order_post_count.lock().await;
        *count += 1;
        *count
    };

    let parsed = serde_json::from_slice::<Value>(&body).ok();
    let request_count = parsed
        .as_ref()
        .and_then(Value::as_array)
        .map_or(0, Vec::len);

    if let Some(v) = parsed {
        *state.last_body.lock().await = Some(v);
    }

    state.batch_order_request_gate.wait().await;

    let status = *state.batch_order_response_status.lock().await;
    let resp = state.batch_order_response.lock().await;
    let body = resp.clone().unwrap_or_else(|| {
        // Namespace by POST count so order IDs are globally unique across chunks, matching the
        // venue (each order receives a distinct ID); a per-chunk index alone would collide.
        let entries: Vec<Value> = (0..request_count.max(1))
            .map(|i| {
                json!({
                    "success": true,
                    "orderID": format!("0xauto-{post_count}-{i}"),
                    "errorMsg": ""
                })
            })
            .collect();
        Value::Array(entries)
    });

    if let Some(responses) = body.as_array() {
        record_open_order_ids(&state, responses).await;
    }
    (status, Json(body)).into_response()
}

async fn handle_delete_order(
    State(state): State<TestServerState>,
    uri: Uri,
    body: Bytes,
) -> Response {
    *state.last_path.lock().await = uri.path().to_string();
    *state.cancel_delete_count.lock().await += 1;

    if let Ok(v) = serde_json::from_slice::<Value>(&body) {
        *state.last_body.lock().await = Some(v);
    }

    state.cancel_request_gate.wait().await;

    let status = *state.cancel_response_status.lock().await;
    let resp = state.cancel_response.lock().await;
    let body = resp
        .clone()
        .unwrap_or_else(|| load_json("http_cancel_response_ok.json"));
    record_canceled_order_ids(&state, &body).await;
    (status, Json(body)).into_response()
}

async fn handle_delete_orders(
    State(state): State<TestServerState>,
    uri: Uri,
    body: Bytes,
) -> Response {
    *state.last_path.lock().await = uri.path().to_string();
    *state.batch_cancel_delete_count.lock().await += 1;

    let request = serde_json::from_slice::<Value>(&body).ok();
    if let Some(request) = &request {
        *state.last_body.lock().await = Some(request.clone());
        state.batch_cancel_bodies.lock().await.push(request.clone());
    }

    state.batch_cancel_request_gate.wait().await;

    let status = if let Some(status) = state
        .batch_cancel_response_statuses
        .lock()
        .await
        .pop_front()
    {
        status
    } else {
        *state.batch_cancel_response_status.lock().await
    };
    let body = if state.batch_cancel_echo_rejections.load(Ordering::Acquire) {
        let not_canceled = request
            .as_ref()
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(|order_id| (order_id.to_string(), json!("order not found")))
            .collect::<serde_json::Map<_, _>>();
        json!({"canceled": [], "not_canceled": not_canceled})
    } else {
        state
            .batch_cancel_response
            .lock()
            .await
            .clone()
            .unwrap_or_else(|| load_json("http_batch_cancel_response.json"))
    };
    record_canceled_order_ids(&state, &body).await;
    let mut response = (status, Json(body)).into_response();
    response.headers_mut().extend(
        state
            .batch_cancel_response_headers
            .lock()
            .await
            .pop_front()
            .unwrap_or_default(),
    );
    response
}

async fn record_open_order_ids(state: &TestServerState, responses: &[Value]) {
    let mut open_order_ids = state.open_order_ids.lock().await;

    for response in responses {
        if response.get("success").and_then(Value::as_bool) != Some(true) {
            continue;
        }

        if let Some(order_id) = response.get("orderID").and_then(Value::as_str)
            && !order_id.is_empty()
        {
            open_order_ids.insert(order_id.to_string());
        }
    }
}

async fn record_canceled_order_ids(state: &TestServerState, response: &Value) {
    let Some(canceled) = response.get("canceled").and_then(Value::as_array) else {
        return;
    };

    let mut open_order_ids = state.open_order_ids.lock().await;
    for order_id in canceled.iter().filter_map(Value::as_str) {
        open_order_ids.remove(order_id);
    }
}

async fn handle_user_upgrade(
    State(state): State<TestServerState>,
    ws: WebSocketUpgrade,
) -> Response {
    state
        .startup_request_paths
        .lock()
        .await
        .push("/ws".to_string());
    ws.on_upgrade(move |socket| handle_user_socket(socket, state))
}

async fn handle_user_socket(mut socket: WebSocket, state: TestServerState) {
    let mut user_frames = state.user_frames.subscribe();
    state.user_socket_count.fetch_add(1, Ordering::AcqRel);

    loop {
        tokio::select! {
            inbound = socket.next() => {
                if inbound.is_none() {
                    break;
                }
            }
            frame = user_frames.recv() => {
                match frame {
                    Ok(frame) => {
                        if socket.send(Message::Text(frame.into())).await.is_err() {
                            break;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
        }
    }

    state.user_socket_count.fetch_sub(1, Ordering::AcqRel);
}

async fn handle_cancel_all(State(state): State<TestServerState>, uri: Uri) -> Response {
    *state.last_path.lock().await = uri.path().to_string();
    Json(load_json("http_batch_cancel_response.json")).into_response()
}

async fn handle_cancel_market_orders(
    State(state): State<TestServerState>,
    uri: Uri,
    body: Bytes,
) -> Response {
    *state.last_path.lock().await = uri.path().to_string();
    *state.market_cancel_delete_count.lock().await += 1;

    if let Ok(value) = serde_json::from_slice::<Value>(&body) {
        *state.last_body.lock().await = Some(value);
    }

    state.market_cancel_request_gate.wait().await;

    let status = *state.market_cancel_response_status.lock().await;
    let body = state
        .market_cancel_response
        .lock()
        .await
        .clone()
        .unwrap_or_else(|| load_json("http_batch_cancel_response.json"));
    record_canceled_order_ids(&state, &body).await;
    (status, Json(body)).into_response()
}

async fn handle_gamma_markets(State(state): State<TestServerState>) -> Response {
    let resp = state.gamma_response.lock().await;
    match resp.as_ref() {
        Some(v) => Json(v.clone()).into_response(),
        None => Json(json!([])).into_response(),
    }
}

async fn handle_get_book(State(state): State<TestServerState>, uri: Uri) -> Response {
    *state.last_path.lock().await = uri.path().to_string();
    let resp = state.book_response.lock().await;
    match resp.as_ref() {
        Some(v) => Json(v.clone()).into_response(),
        None => (StatusCode::OK, Json(json!({"bids": [], "asks": []}))).into_response(),
    }
}

async fn handle_get_fee_rate(
    State(state): State<TestServerState>,
    Query(params): Query<HashMap<String, String>>,
) -> Response {
    *state.fee_rate_fetch_count.lock().await += 1;

    let token_id = params.get("token_id").cloned().unwrap_or_default();
    let override_entry = state
        .fee_rate_overrides
        .lock()
        .await
        .get(&token_id)
        .cloned();

    if let Some((status, body)) = override_entry {
        return (status, Json(body)).into_response();
    }

    let status = *state.fee_rate_response_status.lock().await;
    let resp = state.fee_rate_response.lock().await;
    let body = resp.clone().unwrap_or_else(|| json!({"base_fee": "0"}));
    (status, Json(body)).into_response()
}

async fn handle_heartbeat(
    State(state): State<TestServerState>,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    *state.last_path.lock().await = uri.path().to_string();
    *state.last_headers.lock().await = headers
        .iter()
        .map(|(key, value)| {
            (
                key.as_str().to_string(),
                value.to_str().unwrap_or("").to_string(),
            )
        })
        .collect();

    if let Ok(value) = serde_json::from_slice::<Value>(&body) {
        *state.last_body.lock().await = Some(value);
    }
    state
        .heartbeat_post_times
        .lock()
        .await
        .push(tokio::time::Instant::now());
    state.heartbeat_post_count.fetch_add(1, Ordering::AcqRel);
    state.heartbeat_request_gate.wait().await;

    if state
        .heartbeat_resynchronize_remaining
        .try_update(Ordering::AcqRel, Ordering::Acquire, |remaining| {
            remaining.checked_sub(1)
        })
        .is_ok()
    {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"heartbeat_id": "heartbeat-resynchronized"})),
        )
            .into_response();
    }

    let status = if let Some(status) = state.heartbeat_response_statuses.lock().await.pop_front() {
        status
    } else {
        *state.heartbeat_response_status.lock().await
    };
    let response = state.heartbeat_response.lock().await.clone();
    let mut response = (status, Json(response)).into_response();
    response
        .headers_mut()
        .extend(state.heartbeat_response_headers.lock().await.clone());
    response
}

async fn handle_health() -> impl IntoResponse {
    StatusCode::OK
}

async fn handle_get_positions(State(state): State<TestServerState>) -> impl IntoResponse {
    Json(
        state
            .positions_response_override
            .lock()
            .await
            .clone()
            .unwrap_or_else(|| json!([])),
    )
}

fn create_test_router(state: TestServerState) -> Router {
    Router::new()
        .route("/data/orders", get(handle_get_orders))
        .route("/data/order/{id}", get(handle_get_order))
        .route("/data/trades", get(handle_get_trades))
        .route("/version", get(handle_get_version))
        .route("/balance-allowance", get(handle_get_balance))
        .route(
            "/order",
            post(handle_post_order).delete(handle_delete_order),
        )
        .route(
            "/orders",
            post(handle_post_orders).delete(handle_delete_orders),
        )
        .route("/cancel-all", delete(handle_cancel_all))
        .route("/cancel-market-orders", delete(handle_cancel_market_orders))
        .route("/markets", get(handle_gamma_markets))
        .route("/book", get(handle_get_book))
        .route("/fee-rate", get(handle_get_fee_rate))
        .route("/v1/heartbeats", post(handle_heartbeat))
        .route("/health", get(handle_health))
        .route("/positions", get(handle_get_positions))
        .route("/ws", get(handle_user_upgrade))
        .with_state(state)
}

pub(super) async fn start_mock_server(state: TestServerState) -> SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let router = create_test_router(state);
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });

    wait_until_async(
        || async move {
            HttpClient::builder()
                .build()
                .unwrap()
                .get(format!("http://{addr}/health"), None, None, Some(1), None)
                .await
                .is_ok()
        },
        Duration::from_secs(5),
    )
    .await;

    addr
}
