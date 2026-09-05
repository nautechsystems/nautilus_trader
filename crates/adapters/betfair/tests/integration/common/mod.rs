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

//! Shared test infrastructure for Betfair integration tests.

use std::{
    collections::{HashMap, VecDeque},
    fmt::Debug,
    net::SocketAddr,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use axum::{
    Router,
    body::Bytes,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use nautilus_betfair::{
    common::{
        consts::{
            METHOD_CANCEL_ORDERS, METHOD_LIST_MARKET_CATALOGUE, METHOD_PLACE_ORDERS,
            METHOD_REPLACE_ORDERS,
        },
        credential::BetfairCredential,
    },
    http::client::BetfairHttpClient,
    stream::config::BetfairStreamConfig,
};
use nautilus_common::{
    messages::{SystemEvent, system::SocketStateChange},
    testing::wait_until_async,
};
use nautilus_network::http::HttpClient;
use parking_lot::Mutex;
use serde_json::Value;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::TcpListener,
    sync::{Semaphore, watch},
};

/// Circuit breaker for a logical phase, not routine synchronization.
///
/// Event-driven waits are unbounded on their own; this bound makes a genuine
/// regression fail before the harness kills the process. It also bounds polling
/// where no suitable notification exists. The duration is deliberately well below
/// the harness slow-test thresholds and far above the two-second scheduling gaps seen
/// under load.
#[allow(dead_code)]
pub(crate) const PHASE_TIMEOUT: Duration = Duration::from_secs(15);

#[allow(dead_code)]
pub(crate) async fn wait_for_watch<T, F>(mut rx: watch::Receiver<T>, expected: &str, predicate: F)
where
    T: Debug,
    F: Fn(&T) -> bool,
{
    let wait = async {
        while !predicate(&rx.borrow_and_update()) {
            rx.changed().await.expect("test signal sender dropped");
        }
    };
    tokio::time::timeout(PHASE_TIMEOUT, wait)
        .await
        .unwrap_or_else(|_| {
            panic!(
                "deadline elapsed waiting for {expected}; state after deadline: {:?}",
                *rx.borrow()
            )
        });
}

pub(crate) fn data_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test_data")
}

pub(crate) fn load_fixture(path: &str) -> String {
    std::fs::read_to_string(data_path().join(path))
        .unwrap_or_else(|_| panic!("failed to read {path}"))
}

#[allow(dead_code)]
pub(crate) fn load_json_fixture(path: &str) -> Value {
    serde_json::from_str(&load_fixture(path))
        .unwrap_or_else(|_| panic!("failed to deserialize {path}"))
}

#[allow(dead_code)]
pub(crate) fn betting_api_error(error_code: &str) -> Value {
    let mut response = load_json_fixture("rest/betting_jsonrpc_error_invalid_session_live.json");
    let value = response
        .pointer_mut("/error/data/APINGException/errorCode")
        .expect("live API error fixture must contain APINGException.errorCode");
    assert!(value.is_string());
    *value = Value::String(error_code.to_string());
    response
}

#[allow(dead_code)]
pub(crate) fn jsonrpc_error(code: i64, message: &str) -> Value {
    let mut response = load_json_fixture("rest/betting_jsonrpc_error_invalid_params_live.json");
    response["error"]["code"] = Value::from(code);
    response["error"]["message"] = Value::String(message.to_string());
    response
}

pub(crate) fn test_credential() -> BetfairCredential {
    BetfairCredential::new(
        "testuser".to_string(),
        "testpass".to_string(),
        "test-app-key".to_string(),
    )
}

pub(crate) fn plain_stream_config(port: u16) -> BetfairStreamConfig {
    BetfairStreamConfig {
        host: "127.0.0.1".to_string(),
        port,
        heartbeat_secs: None,
        heartbeat_timeout_secs: Some(60),
        reconnect_delay_initial_ms: 200,
        reconnect_delay_max_ms: 1_000,
        use_tls: false,
    }
}

#[derive(Clone)]
pub(crate) struct MockResponseGate {
    pub method: String,
    pub waiters: Arc<AtomicUsize>,
    pub semaphore: Arc<Semaphore>,
}

#[derive(Clone)]
pub(crate) struct MockState {
    pub login_count: Arc<AtomicUsize>,
    pub keep_alive_count: Arc<AtomicUsize>,
    pub betting_request_count: Arc<AtomicUsize>,
    pub betting_overrides: Arc<Mutex<HashMap<String, Value>>>,
    pub betting_response_sequences: Arc<Mutex<HashMap<String, VecDeque<Value>>>>,
    /// Forces the betting endpoint to return a complete JSON-RPC error response for a method.
    pub betting_error_overrides: Arc<Mutex<HashMap<String, Value>>>,
    /// Like `betting_error_overrides` but consumed on first hit; subsequent
    /// requests for the same method fall through to the default success path.
    /// Used to exercise session-recovery flows where the venue returns
    /// `NO_SESSION` once and accepts the retry.
    pub betting_error_one_shot_overrides: Arc<Mutex<HashMap<String, Value>>>,
    /// Forces the betting endpoint to return a non-2xx HTTP status for a method.
    pub betting_status_overrides: Arc<Mutex<HashMap<String, u16>>>,
    /// Like `betting_status_overrides` but consumed on first hit.
    pub betting_status_one_shot_overrides: Arc<Mutex<HashMap<String, u16>>>,
    /// Records the request as applied, then returns the configured HTTP status once.
    /// Retries with the same `customerRef` fall through without applying again.
    pub betting_apply_then_status_one_shot_overrides: Arc<Mutex<HashMap<String, u16>>>,
    /// Mutating request params applied by `betting_apply_then_status_one_shot_overrides`.
    pub betting_applied_request_params: Arc<Mutex<Vec<(String, Value)>>>,
    pub betting_methods: Arc<Mutex<Vec<String>>>,
    /// Records the `params` payload of each betting request, indexed by call order.
    pub betting_request_params: Arc<Mutex<Vec<(String, Value)>>>,
    /// Per-method response delay; lets tests widen reconciliation windows.
    pub betting_response_delays: Arc<Mutex<HashMap<String, Duration>>>,
    pub betting_response_gate: Arc<Mutex<Option<MockResponseGate>>>,
    pub accounts_response_gate: Arc<Mutex<Option<MockResponseGate>>>,
    pub accounts_overrides: Arc<Mutex<HashMap<String, Value>>>,
    pub accounts_error_overrides: Arc<Mutex<HashMap<String, Value>>>,
    pub login_response_override: Arc<Mutex<Option<String>>>,
    pub keep_alive_response_override: Arc<Mutex<Option<String>>>,
    pub keep_alive_status_override: Arc<Mutex<Option<u16>>>,
    state_tx: watch::Sender<u64>,
}

impl Debug for MockState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let betting_response_gate_waiters = self
            .betting_response_gate
            .lock()
            .as_ref()
            .map_or(0, |gate| gate.waiters.load(Ordering::Relaxed));
        let accounts_response_gate_waiters = self
            .accounts_response_gate
            .lock()
            .as_ref()
            .map_or(0, |gate| gate.waiters.load(Ordering::Relaxed));
        f.debug_struct(stringify!(MockState))
            .field("login_count", &self.login_count.load(Ordering::Relaxed))
            .field(
                "keep_alive_count",
                &self.keep_alive_count.load(Ordering::Relaxed),
            )
            .field(
                "betting_request_count",
                &self.betting_request_count.load(Ordering::Relaxed),
            )
            .field("betting_methods", &*self.betting_methods.lock())
            .field(
                "betting_request_params",
                &*self.betting_request_params.lock(),
            )
            .field(
                "betting_response_gate_waiters",
                &betting_response_gate_waiters,
            )
            .field(
                "accounts_response_gate_waiters",
                &accounts_response_gate_waiters,
            )
            .finish_non_exhaustive()
    }
}

impl Default for MockState {
    fn default() -> Self {
        let (state_tx, _) = watch::channel(0);
        Self {
            login_count: Arc::default(),
            keep_alive_count: Arc::default(),
            betting_request_count: Arc::default(),
            betting_overrides: Arc::default(),
            betting_response_sequences: Arc::default(),
            betting_error_overrides: Arc::default(),
            betting_error_one_shot_overrides: Arc::default(),
            betting_status_overrides: Arc::default(),
            betting_status_one_shot_overrides: Arc::default(),
            betting_apply_then_status_one_shot_overrides: Arc::default(),
            betting_applied_request_params: Arc::default(),
            betting_methods: Arc::default(),
            betting_request_params: Arc::default(),
            betting_response_delays: Arc::default(),
            betting_response_gate: Arc::default(),
            accounts_response_gate: Arc::default(),
            accounts_overrides: Arc::default(),
            accounts_error_overrides: Arc::default(),
            login_response_override: Arc::default(),
            keep_alive_response_override: Arc::default(),
            keep_alive_status_override: Arc::default(),
            state_tx,
        }
    }
}

impl MockState {
    #[allow(dead_code)]
    pub(crate) fn subscribe(&self) -> watch::Receiver<u64> {
        self.state_tx.subscribe()
    }

    fn publish(&self) {
        self.state_tx.send_modify(|generation| {
            *generation = generation.wrapping_add(1);
        });
    }
}

#[allow(dead_code)]
pub(crate) async fn wait_for_mock_state<F>(state: &MockState, expected: &str, predicate: F)
where
    F: Fn(&MockState) -> bool,
{
    let mut changed_rx = state.subscribe();
    let wait = async {
        loop {
            if predicate(state) {
                return;
            }
            changed_rx
                .changed()
                .await
                .expect("test signal sender dropped");
        }
    };
    tokio::time::timeout(PHASE_TIMEOUT, wait)
        .await
        .unwrap_or_else(|_| {
            panic!("deadline elapsed waiting for {expected}; state after deadline: {state:?}")
        });
}

async fn handle_login(State(state): State<MockState>) -> impl IntoResponse {
    state.login_count.fetch_add(1, Ordering::Relaxed);
    state.publish();
    let body = state
        .login_response_override
        .lock()
        .clone()
        .unwrap_or_else(|| load_fixture("rest/login_success.json"));
    (
        [(axum::http::header::CONTENT_TYPE, "application/json")],
        body,
    )
}

async fn handle_keep_alive(State(state): State<MockState>) -> Response {
    state.keep_alive_count.fetch_add(1, Ordering::Relaxed);
    state.publish();
    let body = state
        .keep_alive_response_override
        .lock()
        .clone()
        .unwrap_or_else(|| load_fixture("rest/login_success.json"));
    let status = state
        .keep_alive_status_override
        .lock()
        .map(|status| StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR))
        .unwrap_or(StatusCode::OK);
    (
        status,
        [(axum::http::header::CONTENT_TYPE, "application/json")],
        body,
    )
        .into_response()
}

async fn handle_navigation() -> impl IntoResponse {
    let body = load_fixture("rest/navigation_list_navigation.json");
    (
        [(axum::http::header::CONTENT_TYPE, "application/json")],
        body,
    )
}

async fn handle_betting(State(state): State<MockState>, body: Bytes) -> Response {
    state.betting_request_count.fetch_add(1, Ordering::Relaxed);
    state.publish();
    let request: Value = serde_json::from_slice(&body).unwrap_or_default();
    let method = request.get("method").and_then(|m| m.as_str()).unwrap_or("");
    let id = request.get("id").and_then(|i| i.as_u64()).unwrap_or(0);
    let params = request.get("params").cloned().unwrap_or(Value::Null);

    if !method.is_empty() {
        state.betting_methods.lock().push(method.to_string());
        state.publish();
        state
            .betting_request_params
            .lock()
            .push((method.to_string(), params.clone()));
        state.publish();
    }

    let response_gate = state.betting_response_gate.lock().clone();
    if let Some(gate) = response_gate
        && gate.method == method
    {
        gate.waiters.fetch_add(1, Ordering::Relaxed);
        state.publish();
        gate.semaphore
            .acquire()
            .await
            .expect("betting response gate must remain open")
            .forget();
    }

    let delay = state.betting_response_delays.lock().get(method).copied();

    if let Some(delay) = delay {
        tokio::time::sleep(delay).await;
    }

    if let Some(status) = state
        .betting_apply_then_status_one_shot_overrides
        .lock()
        .remove(method)
    {
        state.publish();
        state
            .betting_applied_request_params
            .lock()
            .push((method.to_string(), params));
        state.publish();
        let code = StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        return (code, "").into_response();
    }

    if let Some(status) = state
        .betting_status_one_shot_overrides
        .lock()
        .remove(method)
    {
        state.publish();
        let code = StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        return (code, "").into_response();
    }

    if let Some(status) = state.betting_status_overrides.lock().get(method).copied() {
        let code = StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        return (code, "").into_response();
    }

    let one_shot_error = state.betting_error_one_shot_overrides.lock().remove(method);

    if one_shot_error.is_some() {
        state.publish();
    }
    let error_response =
        one_shot_error.or_else(|| state.betting_error_overrides.lock().get(method).cloned());

    if let Some(mut response) = error_response {
        response["id"] = Value::from(id);
        return axum::Json(response).into_response();
    }

    let sequence_result = state
        .betting_response_sequences
        .lock()
        .get_mut(method)
        .and_then(VecDeque::pop_front);

    if sequence_result.is_some() {
        state.publish();
    }
    let override_result = state.betting_overrides.lock().get(method).cloned();

    let result = if let Some(value) = sequence_result.or(override_result) {
        value
    } else {
        match method {
            METHOD_LIST_MARKET_CATALOGUE => {
                let fixture = load_fixture("rest/betting_list_market_catalogue.json");
                serde_json::from_str::<Value>(&fixture).unwrap()
            }
            METHOD_PLACE_ORDERS => {
                let fixture = load_fixture("rest/betting_place_order_success.json");
                let v: Value = serde_json::from_str(&fixture).unwrap();
                v["result"].clone()
            }
            METHOD_CANCEL_ORDERS => {
                let fixture = load_fixture("rest/betting_cancel_orders_success.json");
                let v: Value = serde_json::from_str(&fixture).unwrap();
                v["result"].clone()
            }
            METHOD_REPLACE_ORDERS => {
                let fixture = load_fixture("rest/betting_replace_orders_success.json");
                let v: Value = serde_json::from_str(&fixture).unwrap();
                v["result"].clone()
            }
            _ => serde_json::json!(null),
        }
    };

    let response = serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": result,
    });
    axum::Json(response).into_response()
}

async fn handle_accounts(State(state): State<MockState>, body: Bytes) -> impl IntoResponse {
    let request: Value = serde_json::from_slice(&body).unwrap_or_default();
    let method = request.get("method").and_then(|m| m.as_str()).unwrap_or("");
    let id = request.get("id").and_then(|i| i.as_u64()).unwrap_or(0);

    let response_gate = state.accounts_response_gate.lock().clone();
    if let Some(gate) = response_gate
        && gate.method == method
    {
        gate.waiters.fetch_add(1, Ordering::Relaxed);
        state.publish();
        gate.semaphore
            .acquire()
            .await
            .expect("accounts response gate must remain open")
            .forget();
    }

    if let Some(mut response) = state.accounts_error_overrides.lock().get(method).cloned() {
        response["id"] = Value::from(id);
        return axum::Json(response);
    }

    let override_result = state.accounts_overrides.lock().get(method).cloned();

    let result = if let Some(value) = override_result {
        value
    } else {
        let fixture = load_fixture("rest/account_funds_no_exposure.json");
        let v: Value = serde_json::from_str(&fixture).unwrap();
        v["result"].clone()
    };

    let response = serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": result,
    });
    axum::Json(response)
}

#[allow(dead_code)]
pub(crate) async fn assert_mock_state_notifier_contract() {
    fn generation(state: &MockState) -> u64 {
        *state.subscribe().borrow_and_update()
    }

    fn betting_body(method: &str) -> Bytes {
        Bytes::from(serde_json::json!({"method": method, "id": 1, "params": {}}).to_string())
    }

    async fn betting_publications(state: &MockState, method: &str) -> u64 {
        let before = generation(state);
        let _ = handle_betting(State(state.clone()), betting_body(method)).await;
        generation(state).wrapping_sub(before)
    }

    async fn assert_branch_adds_publications(
        state: MockState,
        method: &str,
        normal: u64,
        added: u64,
    ) {
        let observed = betting_publications(&state, method).await;
        assert!(
            observed >= normal.wrapping_add(added),
            "branch published {observed}, expected at least {added} more than baseline {normal}"
        );
    }

    // Every handler-side MockState mutation must appear in this matrix. These checks are
    // relative to freshly measured baselines because extra publications are harmless wakeups.
    let state = MockState::default();
    let before = generation(&state);
    let _ = handle_login(State(state.clone())).await;
    assert!(generation(&state) != before, "login count did not publish");

    let state = MockState::default();
    let before = generation(&state);
    let _ = handle_keep_alive(State(state.clone())).await;
    assert!(
        generation(&state) != before,
        "keep-alive count did not publish"
    );

    let state = MockState::default();
    let before = generation(&state);
    let _ = handle_betting(State(state.clone()), Bytes::from_static(b"{}")).await;
    let malformed = generation(&state).wrapping_sub(before);
    assert!(
        malformed > 0,
        "malformed betting request count did not publish"
    );

    let state = MockState::default();
    let normal = betting_publications(&state, METHOD_PLACE_ORDERS).await;
    assert!(
        normal >= malformed.wrapping_add(2),
        "method and params published {normal}, expected two more than malformed baseline {malformed}"
    );

    let state = MockState::default();
    *state.betting_response_gate.lock() = Some(MockResponseGate {
        method: METHOD_PLACE_ORDERS.to_string(),
        waiters: Arc::default(),
        semaphore: Arc::new(Semaphore::new(1)),
    });
    assert_branch_adds_publications(state, METHOD_PLACE_ORDERS, normal, 1).await;

    let state = MockState::default();
    state
        .betting_apply_then_status_one_shot_overrides
        .lock()
        .insert(METHOD_PLACE_ORDERS.to_string(), 500);
    assert_branch_adds_publications(state, METHOD_PLACE_ORDERS, normal, 2).await;

    let state = MockState::default();
    state
        .betting_status_one_shot_overrides
        .lock()
        .insert(METHOD_PLACE_ORDERS.to_string(), 500);
    assert_branch_adds_publications(state, METHOD_PLACE_ORDERS, normal, 1).await;

    let state = MockState::default();
    state
        .betting_error_one_shot_overrides
        .lock()
        .insert(METHOD_PLACE_ORDERS.to_string(), serde_json::json!({}));
    assert_branch_adds_publications(state, METHOD_PLACE_ORDERS, normal, 1).await;

    let state = MockState::default();
    state.betting_response_sequences.lock().insert(
        METHOD_PLACE_ORDERS.to_string(),
        VecDeque::from([Value::Null]),
    );
    assert_branch_adds_publications(state, METHOD_PLACE_ORDERS, normal, 1).await;

    let state = MockState::default();
    *state.accounts_response_gate.lock() = Some(MockResponseGate {
        method: "AccountAPING/v1.0/getAccountFunds".to_string(),
        waiters: Arc::default(),
        semaphore: Arc::new(Semaphore::new(1)),
    });
    let before = generation(&state);
    let _ = handle_accounts(
        State(state.clone()),
        betting_body("AccountAPING/v1.0/getAccountFunds"),
    )
    .await;
    assert!(
        generation(&state) != before,
        "accounts response-gate waiters did not publish"
    );
}

pub(crate) async fn start_mock_http() -> (SocketAddr, MockState) {
    let state = MockState::default();

    let router = Router::new()
        .route("/login", post(handle_login))
        .route("/keepAlive", post(handle_keep_alive))
        .route("/betting", post(handle_betting))
        .route("/accounts", post(handle_accounts))
        .route("/navigation", get(handle_navigation))
        .route("/health", get(|| async { "OK" }))
        .with_state(state.clone());

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });

    let health_client = HttpClient::builder()
        .headers(std::collections::HashMap::new())
        .build()
        .unwrap();

    wait_until_async(
        || {
            let url = format!("http://{addr}/health");
            let client = health_client.clone();
            async move { client.get(url, None, None, Some(1), None).await.is_ok() }
        },
        Duration::from_secs(5),
    )
    .await;

    (addr, state)
}

pub(crate) async fn start_mock_stream() -> (u16, TcpListener) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    (port, listener)
}

pub(crate) async fn accept_and_auth(
    listener: &TcpListener,
) -> (
    BufReader<tokio::net::tcp::OwnedReadHalf>,
    tokio::net::tcp::OwnedWriteHalf,
) {
    let (reader, write_half, _) = accept_and_capture_auth(listener).await;
    (reader, write_half)
}

#[allow(dead_code)]
pub(crate) async fn accept_and_activate(
    listener: &TcpListener,
) -> (
    BufReader<tokio::net::tcp::OwnedReadHalf>,
    tokio::net::tcp::OwnedWriteHalf,
) {
    let (mut reader, mut write_half) = accept_and_auth(listener).await;
    let mut line = String::new();
    reader.read_line(&mut line).await.unwrap();
    if line.is_empty() {
        return (reader, write_half);
    }

    let subscription: Value = serde_json::from_str(line.trim()).unwrap();
    let id = subscription["id"].as_u64().unwrap();
    let change = match subscription["op"].as_str() {
        Some("marketSubscription") => {
            format!("{{\"op\":\"mcm\",\"id\":{id},\"pt\":1000,\"ct\":\"SUB_IMAGE\",\"mc\":[]}}\r\n",)
        }
        Some("orderSubscription") => {
            format!("{{\"op\":\"ocm\",\"id\":{id},\"pt\":1000,\"ct\":\"SUB_IMAGE\",\"oc\":[]}}\r\n",)
        }
        other => panic!("unexpected stream subscription: {other:?}"),
    };
    write_half
        .write_all(
            format!(
                "{{\"op\":\"status\",\"id\":{id},\"statusCode\":\"SUCCESS\",\"connectionClosed\":false}}\r\n{change}",
            )
            .as_bytes(),
        )
        .await
        .unwrap();

    (reader, write_half)
}

pub(crate) async fn accept_and_capture_auth(
    listener: &TcpListener,
) -> (
    BufReader<tokio::net::tcp::OwnedReadHalf>,
    tokio::net::tcp::OwnedWriteHalf,
    String,
) {
    let (socket, _) = listener.accept().await.unwrap();
    let (read_half, mut write_half) = socket.into_split();
    let mut reader = BufReader::new(read_half);

    write_half
        .write_all(b"{\"op\":\"connection\",\"connectionId\":\"test\"}\r\n")
        .await
        .unwrap();

    let mut line = String::new();
    reader.read_line(&mut line).await.unwrap();
    if line.is_empty() {
        return (reader, write_half, line);
    }
    let auth: Value = serde_json::from_str(line.trim()).unwrap();
    if let Some(id) = auth["id"].as_u64() {
        write_half
            .write_all(
                format!(
                    "{{\"op\":\"status\",\"id\":{id},\"statusCode\":\"SUCCESS\",\"connectionClosed\":false}}\r\n",
                )
                .as_bytes(),
            )
            .await
            .unwrap();
    }

    (reader, write_half, line)
}

#[allow(dead_code)]
pub(crate) async fn next_socket_state(
    receiver: &mut tokio::sync::mpsc::UnboundedReceiver<SystemEvent>,
) -> SocketStateChange {
    let event = tokio::time::timeout(Duration::from_secs(5), receiver.recv())
        .await
        .expect("timed out waiting for socket state change")
        .expect("system event channel closed");
    let SystemEvent::SocketState(change) = event;

    change
}

pub(crate) fn create_test_http_client(addr: SocketAddr) -> BetfairHttpClient {
    BetfairHttpClient::new(
        test_credential(),
        Some(10),
        Some(1),
        Some(100),
        None,
        None,
        None,
    )
    .unwrap()
    .with_urls(
        format!("http://{addr}/login"),
        format!("http://{addr}/betting"),
        format!("http://{addr}/accounts"),
        format!("http://{addr}/navigation"),
    )
}
