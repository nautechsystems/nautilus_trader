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

//! Integration tests for the Polymarket execution client.

use std::{
    cell::RefCell,
    collections::{HashMap, HashSet, VecDeque},
    net::SocketAddr,
    path::PathBuf,
    rc::Rc,
    sync::{
        Arc, Mutex, Once,
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
    routing::{delete, get, post},
};
use futures_util::StreamExt;
use nautilus_common::{
    cache::{Cache, INSTRUMENT_NOT_FOUND, InstrumentLookupError},
    clients::ExecutionClient,
    enums::LogLevel,
    live::runner::set_exec_event_sender,
    messages::{
        ExecutionEvent, ExecutionReport,
        execution::{
            BatchCancelOrders, CancelAllOrders, CancelOrder, GenerateFillReports,
            GenerateOrderStatusReport, GenerateOrderStatusReports, GeneratePositionStatusReports,
            ModifyOrder, QueryAccount, QueryOrder, SubmitOrder, SubmitOrderList,
        },
    },
    testing::wait_until_async,
};
use nautilus_core::{UUID4, UnixNanos};
use nautilus_live::ExecutionClientCore;
use nautilus_model::{
    accounts::{AccountAny, cash::CashAccount},
    enums::{
        AccountType, AssetClass, LiquiditySide, OmsType, OrderSide, OrderStatus, OrderType,
        TimeInForce, TriggerType,
    },
    events::{AccountState, OrderEventAny, OrderPendingCancel},
    identifiers::{
        AccountId, ClientOrderId, InstrumentId, OrderListId, StrategyId, Symbol, TradeId, TraderId,
        VenueOrderId,
    },
    instruments::{BinaryOption, Instrument, InstrumentAny},
    orders::{
        LimitOrder, MarketOrder, Order, OrderAny, OrderList, OrderTestBuilder,
        stubs::TestOrderEventStubs,
    },
    types::{AccountBalance, Currency, Money, Price, Quantity},
};
use nautilus_network::http::HttpClient;
use nautilus_polymarket::{
    common::{
        consts::{POLYMARKET_CLIENT_ID, POLYMARKET_VENUE},
        enums::SignatureType,
    },
    config::PolymarketExecClientConfig,
    execution::PolymarketExecutionClient,
    http::models::PolymarketOrder,
    signing::eip712::order_hash,
};
use rstest::rstest;
use rust_decimal_macros::dec;
use serde_json::{Value, json};

const TEST_PRIVATE_KEY: &str = "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef";
const TEST_CHUNK_PRIVATE_KEY: &str =
    "0x2234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef";
const TEST_CANCEL_ALL_PRIVATE_KEY: &str =
    "0x3234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef";
const TEST_CHUNK_FAILURE_PRIVATE_KEY: &str =
    "0x4234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef";
const TEST_CHUNK_DOWNGRADE_PRIVATE_KEY: &str =
    "0x5234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef";
const TEST_SIGNER_ADDRESS: &str = "0x1be31a94361a391bbafb2a4ccd704f57dc04d4bb";
const TEST_API_SECRET_B64: &str = "dGVzdF9zZWNyZXRfa2V5XzMyYnl0ZXNfcGFkMTIzNDU=";
const DEFAULT_ACCEPTED_ORDER_ID: &str =
    "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef12";
const CANCEL_ALREADY_DONE_ORDER_ID: &str =
    "0xb816482a1234567890abcdef1234567890abcdef1234567890abcdef12345678";

#[derive(Debug)]
struct CaptureLogger {
    records: Mutex<Vec<(log::Level, String)>>,
}

impl log::Log for CaptureLogger {
    fn enabled(&self, _metadata: &log::Metadata<'_>) -> bool {
        true
    }

    fn log(&self, record: &log::Record<'_>) {
        self.records
            .lock()
            .expect("capture logger mutex poisoned")
            .push((record.level(), record.args().to_string()));
    }

    fn flush(&self) {}
}

static CAPTURE_LOGGER: CaptureLogger = CaptureLogger {
    records: Mutex::new(Vec::new()),
};
static CAPTURE_LOGGER_INIT: Once = Once::new();

fn capture_log_start() -> usize {
    CAPTURE_LOGGER_INIT.call_once(|| {
        log::set_logger(&CAPTURE_LOGGER).expect("test logger already installed");
        log::set_max_level(log::LevelFilter::Debug);
    });
    CAPTURE_LOGGER
        .records
        .lock()
        .expect("capture logger mutex poisoned")
        .len()
}

fn captured_logs_since(start: usize) -> Vec<(log::Level, String)> {
    CAPTURE_LOGGER
        .records
        .lock()
        .expect("capture logger mutex poisoned")[start..]
        .to_vec()
}

#[derive(Clone, Copy, Debug)]
enum ShutdownCancelMode {
    Individual,
    CancelAll,
    Batch,
}

#[derive(Clone, Copy, Debug)]
enum CancelChunkRetry {
    None,
    Succeeds,
    Exhausts,
    Downgrades,
}

#[derive(Clone, Copy, Debug)]
enum ShutdownAction {
    Stop,
    Disconnect,
}

#[derive(Debug)]
struct RequestGate {
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
    fn enable(&self) {
        self.enabled.store(true, Ordering::Release);
    }

    fn release(&self) {
        self.permits.add_permits(1);
    }

    fn started(&self) -> usize {
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
struct TestServerState {
    last_body: Arc<tokio::sync::Mutex<Option<Value>>>,
    last_headers: Arc<tokio::sync::Mutex<HashMap<String, String>>>,
    last_path: Arc<tokio::sync::Mutex<String>>,
    last_query: Arc<tokio::sync::Mutex<HashMap<String, String>>>,
    gamma_response: Arc<tokio::sync::Mutex<Option<Value>>>,
    balance_response_status: Arc<tokio::sync::Mutex<StatusCode>>,
    order_response: Arc<tokio::sync::Mutex<Option<Value>>>,
    order_response_status: Arc<tokio::sync::Mutex<StatusCode>>,
    order_post_count: Arc<tokio::sync::Mutex<usize>>,
    /// When > 0, `handle_post_order` returns 500 on this many calls before
    /// reverting to the configured `order_response_status`. Used by retry tests.
    order_post_500_remaining: Arc<tokio::sync::Mutex<usize>>,
    batch_order_response: Arc<tokio::sync::Mutex<Option<Value>>>,
    batch_order_response_status: Arc<tokio::sync::Mutex<StatusCode>>,
    batch_order_post_count: Arc<tokio::sync::Mutex<usize>>,
    fee_rate_response: Arc<tokio::sync::Mutex<Option<Value>>>,
    fee_rate_response_status: Arc<tokio::sync::Mutex<StatusCode>>,
    fee_rate_fetch_count: Arc<tokio::sync::Mutex<usize>>,
    fee_rate_overrides: Arc<tokio::sync::Mutex<HashMap<String, (StatusCode, Value)>>>,
    heartbeat_response: Arc<tokio::sync::Mutex<Value>>,
    heartbeat_response_status: Arc<tokio::sync::Mutex<StatusCode>>,
    heartbeat_response_statuses: Arc<tokio::sync::Mutex<VecDeque<StatusCode>>>,
    heartbeat_post_count: Arc<AtomicUsize>,
    heartbeat_resynchronize_remaining: Arc<AtomicUsize>,
    heartbeat_request_gate: Arc<RequestGate>,
    cancel_response: Arc<tokio::sync::Mutex<Option<Value>>>,
    cancel_response_status: Arc<tokio::sync::Mutex<StatusCode>>,
    cancel_delete_count: Arc<tokio::sync::Mutex<usize>>,
    cancel_request_gate: Arc<RequestGate>,
    batch_cancel_response: Arc<tokio::sync::Mutex<Option<Value>>>,
    batch_cancel_response_status: Arc<tokio::sync::Mutex<StatusCode>>,
    batch_cancel_response_statuses: Arc<tokio::sync::Mutex<VecDeque<StatusCode>>>,
    batch_cancel_response_headers: Arc<tokio::sync::Mutex<VecDeque<HeaderMap>>>,
    batch_cancel_bodies: Arc<tokio::sync::Mutex<Vec<Value>>>,
    batch_cancel_echo_rejections: Arc<AtomicBool>,
    batch_cancel_delete_count: Arc<tokio::sync::Mutex<usize>>,
    batch_cancel_request_gate: Arc<RequestGate>,
    order_request_gate: Arc<RequestGate>,
    batch_order_request_gate: Arc<RequestGate>,
    open_order_ids: Arc<tokio::sync::Mutex<HashSet<String>>>,
    orders_response_override: Arc<tokio::sync::Mutex<Option<Value>>>,
    book_response: Arc<tokio::sync::Mutex<Option<Value>>>,
    single_order_response: Arc<tokio::sync::Mutex<Option<Value>>>,
    trades_response_override: Arc<tokio::sync::Mutex<Option<Value>>>,
    ws_outbound: Arc<tokio::sync::Mutex<Vec<Value>>>,
    ws_sent_count: Arc<tokio::sync::Mutex<usize>>,
}

impl Default for TestServerState {
    fn default() -> Self {
        Self {
            last_body: Arc::new(tokio::sync::Mutex::new(None)),
            last_headers: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            last_path: Arc::new(tokio::sync::Mutex::new(String::new())),
            last_query: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            gamma_response: Arc::new(tokio::sync::Mutex::new(None)),
            balance_response_status: Arc::new(tokio::sync::Mutex::new(StatusCode::OK)),
            order_response: Arc::new(tokio::sync::Mutex::new(None)),
            order_response_status: Arc::new(tokio::sync::Mutex::new(StatusCode::OK)),
            order_post_count: Arc::new(tokio::sync::Mutex::new(0)),
            order_post_500_remaining: Arc::new(tokio::sync::Mutex::new(0)),
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
            heartbeat_post_count: Arc::new(AtomicUsize::new(0)),
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
            order_request_gate: Arc::new(RequestGate::default()),
            batch_order_request_gate: Arc::new(RequestGate::default()),
            open_order_ids: Arc::new(tokio::sync::Mutex::new(HashSet::new())),
            orders_response_override: Arc::new(tokio::sync::Mutex::new(None)),
            single_order_response: Arc::new(tokio::sync::Mutex::new(None)),
            trades_response_override: Arc::new(tokio::sync::Mutex::new(None)),
            ws_outbound: Arc::new(tokio::sync::Mutex::new(Vec::new())),
            ws_sent_count: Arc::new(tokio::sync::Mutex::new(0)),
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
        }
    }
}

fn data_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test_data")
}

fn load_json(filename: &str) -> Value {
    let content = std::fs::read_to_string(data_path().join(filename))
        .unwrap_or_else(|_| panic!("failed to read {filename}"));
    serde_json::from_str(&content).expect("invalid json")
}

fn create_test_exec_config(addr: SocketAddr) -> PolymarketExecClientConfig {
    create_test_exec_config_with_retries(addr, 0)
}

fn create_test_exec_config_with_retries(
    addr: SocketAddr,
    max_retries: u32,
) -> PolymarketExecClientConfig {
    PolymarketExecClientConfig {
        private_key: Some(TEST_PRIVATE_KEY.to_string()),
        api_key: Some("test_api_key".to_string()),
        api_secret: Some(TEST_API_SECRET_B64.to_string()),
        passphrase: Some("test_pass".to_string()),
        funder: None,
        base_url_http: Some(format!("http://{addr}")),
        base_url_ws: Some(format!("ws://{addr}/ws")),
        base_url_data_api: Some(format!("http://{addr}")),
        http_timeout_secs: 5,
        max_retries,
        // Tiny retry delays so tests cover retry counts without paying
        // production backoff (defaults are 1000ms / 10000ms).
        retry_delay_initial_ms: 1,
        retry_delay_max_ms: 10,
        ..PolymarketExecClientConfig::default()
    }
}

fn create_test_execution_client(
    addr: SocketAddr,
) -> (
    PolymarketExecutionClient,
    tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
    Rc<RefCell<Cache>>,
) {
    create_test_execution_client_from_config(create_test_exec_config(addr))
}

fn create_test_execution_client_with_heartbeat(
    addr: SocketAddr,
    heartbeat_enabled: bool,
) -> (
    PolymarketExecutionClient,
    tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
    Rc<RefCell<Cache>>,
) {
    let mut config = create_test_exec_config(addr);
    config.heartbeat_enabled = heartbeat_enabled;
    create_test_execution_client_from_config(config)
}

fn create_test_execution_client_from_config(
    config: PolymarketExecClientConfig,
) -> (
    PolymarketExecutionClient,
    tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
    Rc<RefCell<Cache>>,
) {
    let trader_id = TraderId::from("TESTER-001");
    let account_id = AccountId::from("POLYMARKET-001");
    let client_id = *POLYMARKET_CLIENT_ID;

    let cache = Rc::new(RefCell::new(Cache::default()));

    let core = ExecutionClientCore::new(
        trader_id,
        client_id,
        *POLYMARKET_VENUE,
        OmsType::Netting,
        account_id,
        AccountType::Cash,
        None,
        cache.clone(),
    );

    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    set_exec_event_sender(tx);

    let client = PolymarketExecutionClient::new(core, config).unwrap();

    (client, rx, cache)
}

fn create_test_execution_client_with_retries(
    addr: SocketAddr,
    max_retries: u32,
) -> (
    PolymarketExecutionClient,
    tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
    Rc<RefCell<Cache>>,
) {
    let config = create_test_exec_config_with_retries(addr, max_retries);
    create_test_execution_client_from_config(config)
}

fn add_test_account_to_cache(cache: &Rc<RefCell<Cache>>, account_id: AccountId) {
    let account_state = AccountState::new(
        account_id,
        AccountType::Cash,
        vec![AccountBalance::new(
            Money::from("1000.0 USDC"),
            Money::from("0 USDC"),
            Money::from("1000.0 USDC"),
        )],
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

async fn handle_get_orders(State(state): State<TestServerState>) -> Response {
    *state.last_path.lock().await = "/data/orders".to_string();
    if let Some(override_value) = state.orders_response_override.lock().await.as_ref() {
        return Json(override_value.clone()).into_response();
    }
    Json(load_json("http_open_orders_page.json")).into_response()
}

async fn handle_get_order(State(state): State<TestServerState>) -> Response {
    *state.last_path.lock().await = "/data/order".to_string();
    let resp = state.single_order_response.lock().await;
    match resp.as_ref() {
        Some(v) => Json(v.clone()).into_response(),
        None => Json(load_json("http_open_order.json")).into_response(),
    }
}

async fn handle_get_trades(
    State(state): State<TestServerState>,
    Query(query): Query<HashMap<String, String>>,
) -> Response {
    *state.last_path.lock().await = "/data/trades".to_string();
    *state.last_query.lock().await = query;
    if let Some(override_value) = state.trades_response_override.lock().await.as_ref() {
        return Json(override_value.clone()).into_response();
    }
    Json(load_json("http_trades_page.json")).into_response()
}

async fn handle_get_balance(State(state): State<TestServerState>, headers: HeaderMap) -> Response {
    *state.last_path.lock().await = "/balance-allowance".to_string();
    *state.last_headers.lock().await = headers
        .iter()
        .map(|(k, v)| (k.as_str().to_string(), v.to_str().unwrap_or("").to_string()))
        .collect();
    let status = *state.balance_response_status.lock().await;
    (
        status,
        Json(load_json("http_balance_allowance_collateral.json")),
    )
        .into_response()
}

async fn handle_post_order(
    State(state): State<TestServerState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    *state.last_path.lock().await = "/order".to_string();
    *state.last_headers.lock().await = headers
        .iter()
        .map(|(k, v)| (k.as_str().to_string(), v.to_str().unwrap_or("").to_string()))
        .collect();
    *state.order_post_count.lock().await += 1;

    if let Ok(v) = serde_json::from_slice::<Value>(&body) {
        *state.last_body.lock().await = Some(v);
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
    let body = resp
        .clone()
        .unwrap_or_else(|| load_json("http_order_response_ok.json"));
    record_open_order_ids(&state, std::slice::from_ref(&body)).await;
    (status, Json(body)).into_response()
}

async fn handle_post_orders(
    State(state): State<TestServerState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    *state.last_path.lock().await = "/orders".to_string();
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

async fn handle_delete_order(State(state): State<TestServerState>, body: Bytes) -> Response {
    *state.last_path.lock().await = "/order".to_string();
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

async fn handle_delete_orders(State(state): State<TestServerState>, body: Bytes) -> Response {
    *state.last_path.lock().await = "/orders".to_string();
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
    ws: WebSocketUpgrade,
    State(state): State<TestServerState>,
) -> Response {
    ws.on_upgrade(move |socket| handle_user_socket(socket, state))
}

async fn handle_user_socket(mut socket: WebSocket, state: TestServerState) {
    let mut sent = 0usize;

    loop {
        tokio::select! {
            inbound = socket.next() => {
                if inbound.is_none() {
                    return;
                }
            }
            () = tokio::time::sleep(Duration::from_millis(5)) => {
                let outbound = state.ws_outbound.lock().await.clone();

                while sent < outbound.len() {
                    let payload = outbound[sent].to_string();

                    if socket.send(Message::Text(payload.into())).await.is_err() {
                        return;
                    }

                    sent += 1;
                    *state.ws_sent_count.lock().await = sent;
                }
            }
        }
    }
}

async fn handle_cancel_all(State(state): State<TestServerState>) -> Response {
    *state.last_path.lock().await = "/cancel-all".to_string();
    Json(load_json("http_batch_cancel_response.json")).into_response()
}

async fn handle_gamma_markets(State(state): State<TestServerState>) -> Response {
    let resp = state.gamma_response.lock().await;
    match resp.as_ref() {
        Some(v) => Json(v.clone()).into_response(),
        None => Json(json!([])).into_response(),
    }
}

async fn handle_get_book(State(state): State<TestServerState>) -> Response {
    *state.last_path.lock().await = "/book".to_string();
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
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    *state.last_path.lock().await = "/heartbeats".to_string();
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
    (status, Json(response)).into_response()
}

async fn handle_health() -> impl IntoResponse {
    StatusCode::OK
}

async fn handle_get_positions() -> impl IntoResponse {
    Json(serde_json::json!([]))
}

fn create_test_router(state: TestServerState) -> Router {
    Router::new()
        .route("/data/orders", get(handle_get_orders))
        .route("/data/order/{id}", get(handle_get_order))
        .route("/data/trades", get(handle_get_trades))
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
        .route("/markets", get(handle_gamma_markets))
        .route("/book", get(handle_get_book))
        .route("/fee-rate", get(handle_get_fee_rate))
        .route("/heartbeats", post(handle_heartbeat))
        .route("/health", get(handle_health))
        .route("/positions", get(handle_get_positions))
        .route("/ws", get(handle_user_upgrade))
        .with_state(state)
}

async fn start_mock_server(state: TestServerState) -> SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let router = create_test_router(state);
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });

    wait_until_async(
        || async move {
            HttpClient::new(HashMap::new(), vec![], vec![], None, None, None)
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

async fn submit_tracked_order(
    client: &PolymarketExecutionClient,
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
    cache: &Rc<RefCell<Cache>>,
    instrument_id: InstrumentId,
    client_order_id: &str,
) {
    let order = make_limit_order(
        client_order_id,
        instrument_id,
        OrderSide::Buy,
        false,
        false,
        false,
        TimeInForce::Gtc,
    );
    cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .unwrap();
    client
        .submit_order(make_submit_cmd(&order, instrument_id))
        .unwrap();
    assert_order_event(recv_order_execution_event(rx).await, "Submitted");
    assert_order_event(recv_order_execution_event(rx).await, "Accepted");
}

async fn recv_order_execution_event(
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
) -> ExecutionEvent {
    loop {
        let event = recv_execution_event(rx).await;

        if matches!(event, ExecutionEvent::Order(_)) {
            return event;
        }
    }
}

fn maker_recovery_trade(venue_order_id: &str, trade_id: &str, quantity: &str) -> Value {
    let mut trade = load_json("http_trade_report.json");
    let mut maker_order = trade["maker_orders"][0].clone();
    maker_order["order_id"] = json!(venue_order_id);
    maker_order["maker_address"] = json!("0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266");
    maker_order["owner"] = json!("test_api_key");
    maker_order["matched_amount"] = json!(quantity);
    trade["id"] = json!(trade_id);
    trade["trader_side"] = json!("MAKER");
    trade["maker_orders"] = json!([maker_order]);
    trade
}

fn cache_legacy_maker_fill(
    cache: &Rc<RefCell<Cache>>,
    instrument: &InstrumentAny,
    venue_order_id: &str,
    client_order_id: &str,
    legacy_trade_id: TradeId,
) {
    let mut order = make_limit_order_at_price_and_quantity(
        client_order_id,
        instrument.id(),
        OrderSide::Buy,
        false,
        false,
        false,
        TimeInForce::Gtc,
        Price::from("0.5000"),
        Quantity::from("10.0000"),
    );
    cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .unwrap();
    submit_and_accept_order(cache, &mut order, venue_order_id);
    let filled = TestOrderEventStubs::filled(
        &order,
        instrument,
        Some(legacy_trade_id),
        None,
        Some(Price::from("0.5000")),
        Some(Quantity::from("4.0000")),
        Some(LiquiditySide::Maker),
        None,
        None,
        Some(AccountId::from("POLYMARKET-001")),
    );
    cache.borrow_mut().update_order(&filled).unwrap();
}

#[rstest]
#[tokio::test]
async fn test_mass_status_skips_legacy_applied_maker_fill_but_keeps_new_fill() {
    let venue_order_id = "0xmaker01maker01maker01maker01maker01maker01maker01maker01maker01maker01";
    let legacy_trade_id = TradeId::from("123456789012345678901234567-1maker01");
    let first_trade_id = "123456789012345678901234567FIRST";
    let second_trade_id = "223456789012345678901234567OTHER";
    let mut open_order = load_json("http_open_order.json");
    open_order["id"] = json!(venue_order_id);
    open_order["original_size"] = json!("10.0000");
    open_order["size_matched"] = json!("6.0000");
    let state = TestServerState::default();
    *state.orders_response_override.lock().await = Some(json!({
        "data": [open_order],
        "next_cursor": "LTE="
    }));
    *state.trades_response_override.lock().await = Some(json!({
        "data": [
            maker_recovery_trade(venue_order_id, first_trade_id, "4.0000"),
            maker_recovery_trade(venue_order_id, second_trade_id, "2.0000")
        ],
        "next_cursor": "LTE="
    }));
    let addr = start_mock_server(state).await;
    let (mut client, _rx, cache) = create_test_execution_client(addr);
    let instrument_id = InstrumentId::from("TEST-TOKEN.POLYMARKET");
    add_instrument_to_cache_with_size_precision(&cache, instrument_id, 4);
    let instrument = cache.borrow().instrument(&instrument_id).unwrap().clone();
    client.on_instrument(instrument.clone());
    cache_legacy_maker_fill(
        &cache,
        &instrument,
        venue_order_id,
        "O-LEGACY-MASS",
        legacy_trade_id,
    );

    let mass_status = client
        .generate_mass_status(None)
        .await
        .unwrap()
        .expect("mass status payload");
    let fills: Vec<_> = mass_status.fill_reports().into_values().flatten().collect();

    assert_eq!(fills.len(), 1);
    assert_eq!(fills[0].last_qty, Quantity::from("2.0000"));
}

#[rstest]
#[tokio::test]
async fn test_query_order_skips_legacy_applied_maker_fill_but_emits_new_fill() {
    let venue_order_id = "0xmaker01maker01maker01maker01maker01maker01maker01maker01maker01maker01";
    let legacy_trade_id = TradeId::from("123456789012345678901234567-1maker01");
    let first_trade_id = "123456789012345678901234567FIRST";
    let second_trade_id = "223456789012345678901234567OTHER";
    let mut open_order = load_json("http_open_order.json");
    open_order["id"] = json!(venue_order_id);
    open_order["original_size"] = json!("10.0000");
    open_order["size_matched"] = json!("6.0000");
    let state = TestServerState::default();
    *state.single_order_response.lock().await = Some(open_order);
    *state.trades_response_override.lock().await = Some(json!({
        "data": [
            maker_recovery_trade(venue_order_id, first_trade_id, "4.0000"),
            maker_recovery_trade(venue_order_id, second_trade_id, "2.0000")
        ],
        "next_cursor": "LTE="
    }));
    let addr = start_mock_server(state).await;
    let (mut client, mut rx, cache) = create_test_execution_client(addr);
    client.start().unwrap();
    let instrument_id = InstrumentId::from("TEST-TOKEN.POLYMARKET");
    add_instrument_to_cache_with_size_precision(&cache, instrument_id, 4);
    let instrument = cache.borrow().instrument(&instrument_id).unwrap().clone();
    client.on_instrument(instrument.clone());
    let client_order_id = ClientOrderId::from("O-LEGACY-QUERY");
    cache_legacy_maker_fill(
        &cache,
        &instrument,
        venue_order_id,
        client_order_id.as_str(),
        legacy_trade_id,
    );
    let cmd = QueryOrder::new(
        TraderId::from("TESTER-001"),
        Some(*POLYMARKET_CLIENT_ID),
        StrategyId::from("S-001"),
        instrument_id,
        client_order_id,
        Some(VenueOrderId::from(venue_order_id)),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );

    client.query_order(cmd).unwrap();

    let fill = match recv_execution_event(&mut rx).await {
        ExecutionEvent::Report(ExecutionReport::Fill(fill)) => fill,
        other => panic!("Expected new Fill report, was {other:?}"),
    };
    assert_eq!(fill.last_qty, Quantity::from("2.0000"));
    assert_order_status_report(recv_execution_event(&mut rx).await, OrderStatus::Accepted);
}

#[rstest]
#[tokio::test]
async fn test_query_order_emits_and_counts_only_fills_for_queried_order() {
    let queried_venue_order_id =
        "0xmaker01maker01maker01maker01maker01maker01maker01maker01maker01maker01";
    let other_venue_order_id =
        "0xmaker02maker02maker02maker02maker02maker02maker02maker02maker02maker02";
    let queried_legacy_trade_id = TradeId::from("123456789012345678901234567-1maker01");
    let other_legacy_trade_id = TradeId::from("323456789012345678901234567-2maker02");
    let queried_applied_trade_id = "123456789012345678901234567FIRST";
    let queried_new_trade_id = "223456789012345678901234567SECOND";
    let other_trade_id = "323456789012345678901234567OTHER";
    let mut open_order = load_json("http_open_order.json");
    open_order["id"] = json!(queried_venue_order_id);
    open_order["original_size"] = json!("10.0000");
    open_order["size_matched"] = json!("6.0000");
    let state = TestServerState::default();
    *state.single_order_response.lock().await = Some(open_order);
    *state.trades_response_override.lock().await = Some(json!({
        "data": [
            maker_recovery_trade(
                queried_venue_order_id,
                queried_applied_trade_id,
                "4.0000",
            ),
            maker_recovery_trade(queried_venue_order_id, queried_new_trade_id, "2.0000"),
            maker_recovery_trade(other_venue_order_id, other_trade_id, "3.0000"),
        ],
        "next_cursor": "LTE="
    }));
    let addr = start_mock_server(state).await;
    let (mut client, mut rx, cache) = create_test_execution_client(addr);
    client.start().unwrap();
    let instrument_id = InstrumentId::from("TEST-TOKEN.POLYMARKET");
    add_instrument_to_cache_with_size_precision(&cache, instrument_id, 4);
    let instrument = cache.borrow().instrument(&instrument_id).unwrap().clone();
    client.on_instrument(instrument.clone());
    let queried_client_order_id = ClientOrderId::from("O-QUERY-ONLY-A");
    cache_legacy_maker_fill(
        &cache,
        &instrument,
        queried_venue_order_id,
        queried_client_order_id.as_str(),
        queried_legacy_trade_id,
    );
    cache_legacy_maker_fill(
        &cache,
        &instrument,
        other_venue_order_id,
        "O-QUERY-OTHER-B",
        other_legacy_trade_id,
    );
    let log_start = capture_log_start();
    let cmd = QueryOrder::new(
        TraderId::from("TESTER-001"),
        Some(*POLYMARKET_CLIENT_ID),
        StrategyId::from("S-001"),
        instrument_id,
        queried_client_order_id,
        Some(VenueOrderId::from(queried_venue_order_id)),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );

    client.query_order(cmd).unwrap();

    let fill = match recv_execution_event(&mut rx).await {
        ExecutionEvent::Report(ExecutionReport::Fill(fill)) => fill,
        other => panic!("expected queried-order fill report, was {other:?}"),
    };
    assert_eq!(
        fill.venue_order_id,
        VenueOrderId::from(queried_venue_order_id)
    );
    assert_eq!(fill.last_qty, Quantity::from("2.0000"));
    assert_order_status_report(recv_execution_event(&mut rx).await, OrderStatus::Accepted);
    assert!(
        rx.try_recv().is_err(),
        "other-order fill must not be emitted"
    );

    let skipped_message =
        format!("Skipped 1 REST fill report(s) already applied to order {queried_venue_order_id}",);
    let matching_logs = captured_logs_since(log_start)
        .into_iter()
        .filter(|(_, message)| message == &skipped_message)
        .collect::<Vec<_>>();
    assert_eq!(matching_logs, vec![(log::Level::Debug, skipped_message)]);
}

#[rstest]
#[tokio::test]
async fn test_generate_mass_status_withholds_pending_fills_for_a_failed_trade() {
    let venue_order_id = "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef12";
    let trade_id = "trade-0xabcdef1234";
    let now_secs = nautilus_core::time::get_atomic_clock_realtime()
        .get_time_ns()
        .as_u64()
        / 1_000_000_000;

    let mut matched_trade_msg = load_json("ws_user_trade_msg.json");
    matched_trade_msg["status"] = json!("MATCHED");
    let mut failed_trade_msg = matched_trade_msg.clone();
    failed_trade_msg["status"] = json!("FAILED");

    let mut open_order = load_json("http_open_order.json");
    open_order["id"] = json!(venue_order_id);
    open_order["created_at"] = json!(now_secs);
    open_order["size_matched"] = json!("25.0000");

    let mut pending_trade = load_json("http_trade_report.json");
    pending_trade["id"] = json!(trade_id);
    pending_trade["status"] = json!("MATCHED");
    pending_trade["taker_order_id"] = json!(venue_order_id);
    pending_trade["match_time"] = json!(now_secs.to_string());
    pending_trade["maker_orders"] = json!([]);

    let state = TestServerState::default();
    *state.order_response.lock().await = Some(load_json("http_order_response_ok.json"));
    *state.orders_response_override.lock().await = Some(json!({
        "data": [open_order],
        "next_cursor": "LTE="
    }));
    *state.trades_response_override.lock().await = Some(json!({
        "data": [pending_trade],
        "next_cursor": "LTE="
    }));
    let addr = start_mock_server(state.clone()).await;
    let (mut client, mut rx, cache) = create_test_execution_client(addr);
    let instrument_id = InstrumentId::from("TEST-TOKEN.POLYMARKET");
    add_instrument_to_cache_with_size_precision(&cache, instrument_id, 4);
    let instrument = cache.borrow().instrument(&instrument_id).unwrap().clone();
    client.on_instrument(instrument);
    add_test_account_to_cache(&cache, AccountId::from("POLYMARKET-001"));
    client.start().unwrap();
    client.connect().await.unwrap();

    submit_tracked_order(&client, &mut rx, &cache, instrument_id, "O-WITHHELD-TRADE").await;

    state
        .ws_outbound
        .lock()
        .await
        .extend([matched_trade_msg, failed_trade_msg]);

    loop {
        if let ExecutionEvent::Order(OrderEventAny::FillVoided(voided)) =
            recv_order_execution_event(&mut rx).await
        {
            assert_eq!(voided.venue_order_id, VenueOrderId::from(venue_order_id));
            break;
        }
    }

    let mass_status = client
        .generate_mass_status(Some(60))
        .await
        .unwrap()
        .expect("mass status payload");
    let fill_count = mass_status.fill_reports().into_values().flatten().count();
    let order_reports = mass_status.order_reports();
    let order_report = order_reports
        .get(&VenueOrderId::from(venue_order_id))
        .expect("order report for the failed trade's order");

    assert_eq!(fill_count, 0);
    assert!(order_report.filled_qty.is_zero());
}

#[rstest]
#[tokio::test]
async fn test_generate_mass_status_snaps_pending_fills_to_the_submitted_quantity() {
    let venue_order_id = "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef12";
    let now_secs = nautilus_core::time::get_atomic_clock_realtime()
        .get_time_ns()
        .as_u64()
        / 1_000_000_000;

    let mut open_order = load_json("http_open_order.json");
    open_order["id"] = json!(venue_order_id);
    open_order["created_at"] = json!(now_secs);
    open_order["size_matched"] = json!("10.0001");

    let mut pending_trade = load_json("http_trade_report.json");
    pending_trade["id"] = json!("trade-snap");
    pending_trade["status"] = json!("MATCHED");
    pending_trade["taker_order_id"] = json!(venue_order_id);
    pending_trade["match_time"] = json!(now_secs.to_string());
    pending_trade["size"] = json!("10.0001");
    pending_trade["maker_orders"] = json!([]);

    let state = TestServerState::default();
    *state.order_response.lock().await = Some(load_json("http_order_response_ok.json"));
    *state.orders_response_override.lock().await = Some(json!({
        "data": [open_order],
        "next_cursor": "LTE="
    }));
    *state.trades_response_override.lock().await = Some(json!({
        "data": [pending_trade],
        "next_cursor": "LTE="
    }));
    let addr = start_mock_server(state).await;
    let (mut client, mut rx, cache) = create_test_execution_client(addr);
    let instrument_id = InstrumentId::from("TEST-TOKEN.POLYMARKET");
    add_instrument_to_cache_with_size_precision(&cache, instrument_id, 4);
    let instrument = cache.borrow().instrument(&instrument_id).unwrap().clone();
    client.on_instrument(instrument);
    add_test_account_to_cache(&cache, AccountId::from("POLYMARKET-001"));
    client.start().unwrap();

    submit_tracked_order(&client, &mut rx, &cache, instrument_id, "O-SNAP-PENDING").await;

    let mass_status = client
        .generate_mass_status(Some(60))
        .await
        .unwrap()
        .expect("mass status payload");
    let fill_reports: Vec<_> = mass_status.fill_reports().into_values().flatten().collect();

    assert_eq!(fill_reports.len(), 1);
    assert_eq!(fill_reports[0].last_qty, Quantity::new(10.0, 4));
}

#[rstest]
#[tokio::test]
async fn test_failed_trade_voids_the_snapped_quantity_recorded_as_evidence() {
    let venue_order_id = "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef12";
    let now_secs = nautilus_core::time::get_atomic_clock_realtime()
        .get_time_ns()
        .as_u64()
        / 1_000_000_000;

    let mut open_order = load_json("http_open_order.json");
    open_order["id"] = json!(venue_order_id);
    open_order["created_at"] = json!(now_secs);
    open_order["size_matched"] = json!("10.0001");

    let mut pending_trade = load_json("http_trade_report.json");
    pending_trade["id"] = json!("trade-0xabcdef1234");
    pending_trade["status"] = json!("MATCHED");
    pending_trade["taker_order_id"] = json!(venue_order_id);
    pending_trade["match_time"] = json!(now_secs.to_string());
    pending_trade["size"] = json!("10.0001");
    pending_trade["maker_orders"] = json!([]);

    let mut failed_trade_msg = load_json("ws_user_trade_msg.json");
    failed_trade_msg["status"] = json!("FAILED");

    let state = TestServerState::default();
    *state.order_response.lock().await = Some(load_json("http_order_response_ok.json"));
    *state.orders_response_override.lock().await = Some(json!({
        "data": [open_order],
        "next_cursor": "LTE="
    }));
    *state.trades_response_override.lock().await = Some(json!({
        "data": [pending_trade],
        "next_cursor": "LTE="
    }));
    let addr = start_mock_server(state.clone()).await;
    let (mut client, mut rx, cache) = create_test_execution_client(addr);
    let instrument_id = InstrumentId::from("TEST-TOKEN.POLYMARKET");
    add_instrument_to_cache_with_size_precision(&cache, instrument_id, 4);
    let instrument = cache.borrow().instrument(&instrument_id).unwrap().clone();
    client.on_instrument(instrument);
    add_test_account_to_cache(&cache, AccountId::from("POLYMARKET-001"));
    client.start().unwrap();
    client.connect().await.unwrap();

    submit_tracked_order(
        &client,
        &mut rx,
        &cache,
        instrument_id,
        "O-SNAPPED-EVIDENCE",
    )
    .await;

    client
        .generate_mass_status(Some(60))
        .await
        .unwrap()
        .expect("mass status payload");

    state.ws_outbound.lock().await.push(failed_trade_msg);

    let voided = loop {
        if let ExecutionEvent::Order(OrderEventAny::FillVoided(voided)) =
            recv_order_execution_event(&mut rx).await
        {
            break voided;
        }
    };

    assert_eq!(voided.venue_order_id, VenueOrderId::from(venue_order_id));
    assert_eq!(voided.voided_qty, Quantity::new(10.0, 4));
}

#[rstest]
#[tokio::test]
async fn test_exec_client_creation() {
    let state = TestServerState::default();
    let addr = start_mock_server(state).await;
    let (client, _rx, _cache) = create_test_execution_client(addr);

    assert_eq!(client.client_id(), *POLYMARKET_CLIENT_ID);
    assert_eq!(client.account_id(), AccountId::from("POLYMARKET-001"));
    assert_eq!(client.venue(), *POLYMARKET_VENUE);
    assert_eq!(client.oms_type(), OmsType::Netting);
}

#[rstest]
#[tokio::test]
async fn test_exec_client_poly1271_requires_distinct_funder() {
    let state = TestServerState::default();
    let addr = start_mock_server(state).await;
    let trader_id = TraderId::from("TESTER-001");
    let account_id = AccountId::from("POLYMARKET-001");
    let cache = Rc::new(RefCell::new(Cache::default()));
    let core = ExecutionClientCore::new(
        trader_id,
        *POLYMARKET_CLIENT_ID,
        *POLYMARKET_VENUE,
        OmsType::Netting,
        account_id,
        AccountType::Cash,
        None,
        cache,
    );
    let mut config = create_test_exec_config(addr);
    config.signature_type = SignatureType::Poly1271;
    config.funder = Some(TEST_SIGNER_ADDRESS.to_string());

    let error = PolymarketExecutionClient::new(core, config).unwrap_err();

    assert!(
        error.to_string().contains(
            "Poly1271 signature type requires a funder distinct from the signing address"
        )
    );
}

#[rstest]
#[tokio::test]
async fn test_exec_client_poly1271_uses_signer_for_api_auth() {
    let state = TestServerState::default();
    let addr = start_mock_server(state.clone()).await;
    let trader_id = TraderId::from("TESTER-001");
    let account_id = AccountId::from("POLYMARKET-001");
    let cache = Rc::new(RefCell::new(Cache::default()));
    let core = ExecutionClientCore::new(
        trader_id,
        *POLYMARKET_CLIENT_ID,
        *POLYMARKET_VENUE,
        OmsType::Netting,
        account_id,
        AccountType::Cash,
        None,
        cache,
    );
    let mut config = create_test_exec_config(addr);
    let funder = "0x1111111111111111111111111111111111111111".to_string();
    config.signature_type = SignatureType::Poly1271;
    config.funder = Some(funder.clone());

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    set_exec_event_sender(tx);
    let mut client = PolymarketExecutionClient::new(core, config).unwrap();
    client.start().unwrap();

    let cmd = QueryAccount::new(
        TraderId::from("TESTER-001"),
        Some(*POLYMARKET_CLIENT_ID),
        AccountId::from("POLYMARKET-001"),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );

    client.query_account(cmd).unwrap();

    let event = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .unwrap()
        .unwrap();
    let headers = state.last_headers.lock().await;

    assert!(
        matches!(event, ExecutionEvent::Account(_)),
        "Expected Account event, was {event:?}"
    );
    assert_eq!(
        headers.get("poly_address").map(String::as_str),
        Some(TEST_SIGNER_ADDRESS),
    );
}

#[rstest]
#[tokio::test]
async fn test_exec_client_not_connected_initially() {
    let state = TestServerState::default();
    let addr = start_mock_server(state).await;
    let (client, _rx, _cache) = create_test_execution_client(addr);

    assert!(!client.is_connected());
}

#[rstest]
#[tokio::test]
async fn test_heartbeat_disabled_preserves_connection_behavior() {
    let state = TestServerState::default();
    let addr = start_mock_server(state.clone()).await;
    let (mut client, _rx, cache) = create_test_execution_client(addr);
    add_test_account_to_cache(&cache, AccountId::from("POLYMARKET-001"));
    client.start().unwrap();

    client.connect().await.unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;

    assert!(client.is_connected());
    assert_eq!(state.heartbeat_post_count.load(Ordering::Acquire), 0);

    client.disconnect().await.unwrap();
    assert!(!client.is_connected());
}

#[rstest]
#[tokio::test]
async fn test_heartbeat_starts_after_readiness_and_prevents_duplicate_tasks() {
    let state = TestServerState::default();
    let addr = start_mock_server(state.clone()).await;
    let (mut client, _rx, cache) = create_test_execution_client_with_heartbeat(addr, true);
    client.start().unwrap();

    let mut connect = Box::pin(client.connect());
    assert!(
        tokio::time::timeout(Duration::from_millis(100), connect.as_mut())
            .await
            .is_err()
    );
    assert_eq!(state.heartbeat_post_count.load(Ordering::Acquire), 0);

    add_test_account_to_cache(&cache, AccountId::from("POLYMARKET-001"));
    connect.await.unwrap();
    await_heartbeat_posts(&state, 1, Duration::from_secs(1)).await;
    assert!(client.is_connected());
    assert_eq!(
        state.last_body.lock().await.as_ref(),
        Some(&json!({"heartbeat_id": ""})),
    );

    client.connect().await.unwrap();
    tokio::time::sleep(Duration::from_secs(1)).await;
    assert_eq!(state.heartbeat_post_count.load(Ordering::Acquire), 1);

    await_heartbeat_posts(&state, 2, Duration::from_secs(5)).await;
    assert_eq!(state.heartbeat_post_count.load(Ordering::Acquire), 2);
    assert_eq!(
        state.last_body.lock().await.as_ref(),
        Some(&json!({"heartbeat_id": "heartbeat-next"})),
    );

    client.disconnect().await.unwrap();
    assert!(!client.is_connected());

    client.connect().await.unwrap();
    await_heartbeat_posts(&state, 3, Duration::from_secs(1)).await;
    assert!(client.is_connected());
    assert_eq!(state.heartbeat_post_count.load(Ordering::Acquire), 3);

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_heartbeat_resynchronizes_immediately_with_replacement_id() {
    let state = TestServerState::default();
    state
        .heartbeat_resynchronize_remaining
        .store(1, Ordering::Release);
    let addr = start_mock_server(state.clone()).await;
    let (mut client, _rx, cache) = create_test_execution_client_with_heartbeat(addr, true);
    add_test_account_to_cache(&cache, AccountId::from("POLYMARKET-001"));
    client.start().unwrap();

    client.connect().await.unwrap();
    await_heartbeat_posts(&state, 2, Duration::from_secs(1)).await;

    assert!(client.is_connected());
    assert_eq!(state.heartbeat_post_count.load(Ordering::Acquire), 2);
    assert_eq!(
        state.last_body.lock().await.as_ref(),
        Some(&json!({"heartbeat_id": "heartbeat-resynchronized"})),
    );

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_repeated_heartbeat_resynchronization_marks_execution_unhealthy() {
    let state = TestServerState::default();
    state
        .heartbeat_resynchronize_remaining
        .store(2, Ordering::Release);
    let addr = start_mock_server(state.clone()).await;
    let (mut client, _rx, cache) = create_test_execution_client_with_heartbeat(addr, true);
    add_test_account_to_cache(&cache, AccountId::from("POLYMARKET-001"));
    client.start().unwrap();

    client.connect().await.unwrap();
    await_heartbeat_posts(&state, 2, Duration::from_secs(1)).await;
    await_execution_unhealthy(&client, Duration::from_secs(1)).await;

    assert_eq!(state.heartbeat_post_count.load(Ordering::Acquire), 2);

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_disconnect_cancels_and_awaits_in_flight_heartbeat() {
    let state = TestServerState::default();
    state.heartbeat_request_gate.enable();
    let addr = start_mock_server(state.clone()).await;
    let (mut client, _rx, cache) = create_test_execution_client_with_heartbeat(addr, true);
    add_test_account_to_cache(&cache, AccountId::from("POLYMARKET-001"));
    client.start().unwrap();
    client.connect().await.unwrap();
    wait_until_async(
        || {
            let state = state.clone();
            async move { state.heartbeat_request_gate.started() == 1 }
        },
        Duration::from_secs(1),
    )
    .await;

    let result = tokio::time::timeout(Duration::from_secs(1), client.disconnect()).await;
    state.heartbeat_request_gate.release();

    assert!(result.is_ok());
    assert!(result.unwrap().is_ok());
    assert!(!client.is_connected());
    assert_eq!(state.heartbeat_post_count.load(Ordering::Acquire), 1);
}

#[rstest]
#[tokio::test]
async fn test_heartbeat_authentication_failure_marks_execution_unhealthy() {
    let state = TestServerState::default();
    *state.heartbeat_response_status.lock().await = StatusCode::UNAUTHORIZED;
    let addr = start_mock_server(state.clone()).await;
    let (mut client, _rx, cache) = create_test_execution_client_with_heartbeat(addr, true);
    add_test_account_to_cache(&cache, AccountId::from("POLYMARKET-001"));
    client.start().unwrap();

    client.connect().await.unwrap();
    await_execution_unhealthy(&client, Duration::from_secs(1)).await;

    assert_eq!(state.heartbeat_post_count.load(Ordering::Acquire), 1);

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_heartbeat_venue_rejection_marks_execution_unhealthy() {
    let state = TestServerState::default();
    *state.heartbeat_response.lock().await = json!({"status": "rejected"});
    let addr = start_mock_server(state.clone()).await;
    let (mut client, _rx, cache) = create_test_execution_client_with_heartbeat(addr, true);
    add_test_account_to_cache(&cache, AccountId::from("POLYMARKET-001"));
    client.start().unwrap();

    client.connect().await.unwrap();
    await_execution_unhealthy(&client, Duration::from_secs(1)).await;

    assert_eq!(state.heartbeat_post_count.load(Ordering::Acquire), 1);

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_repeated_heartbeat_request_failure_marks_execution_unhealthy() {
    let state = TestServerState::default();
    *state.heartbeat_response_status.lock().await = StatusCode::INTERNAL_SERVER_ERROR;
    let addr = start_mock_server(state.clone()).await;
    let (mut client, _rx, cache) = create_test_execution_client_with_heartbeat(addr, true);
    add_test_account_to_cache(&cache, AccountId::from("POLYMARKET-001"));
    client.start().unwrap();

    client.connect().await.unwrap();
    await_heartbeat_posts(&state, 2, Duration::from_secs(6)).await;
    await_execution_unhealthy(&client, Duration::from_secs(1)).await;

    assert_eq!(state.heartbeat_post_count.load(Ordering::Acquire), 2);

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_heartbeat_success_resets_consecutive_request_failures() {
    let state = TestServerState::default();
    state.heartbeat_response_statuses.lock().await.extend([
        StatusCode::INTERNAL_SERVER_ERROR,
        StatusCode::OK,
        StatusCode::INTERNAL_SERVER_ERROR,
    ]);
    let addr = start_mock_server(state.clone()).await;
    let (mut client, _rx, cache) = create_test_execution_client_with_heartbeat(addr, true);
    add_test_account_to_cache(&cache, AccountId::from("POLYMARKET-001"));
    client.start().unwrap();

    client.connect().await.unwrap();
    await_heartbeat_posts(&state, 3, Duration::from_secs(11)).await;
    tokio::time::sleep(Duration::from_millis(100)).await;

    assert!(client.is_connected());
    assert_eq!(state.heartbeat_post_count.load(Ordering::Acquire), 3);

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_repeated_heartbeat_request_timeout_marks_execution_unhealthy() {
    let state = TestServerState::default();
    state.heartbeat_request_gate.enable();
    let addr = start_mock_server(state.clone()).await;
    let (mut client, _rx, cache) = create_test_execution_client_with_heartbeat(addr, true);
    add_test_account_to_cache(&cache, AccountId::from("POLYMARKET-001"));
    client.start().unwrap();

    client.connect().await.unwrap();
    await_execution_unhealthy(&client, Duration::from_secs(10)).await;

    let started = state.heartbeat_request_gate.started();
    for _ in 0..started {
        state.heartbeat_request_gate.release();
    }
    assert_eq!(started, 2);
    assert_eq!(state.heartbeat_post_count.load(Ordering::Acquire), 2);

    client.disconnect().await.unwrap();
}

async fn await_heartbeat_posts(state: &TestServerState, expected: usize, timeout: Duration) {
    wait_until_async(
        || {
            let state = state.clone();
            async move { state.heartbeat_post_count.load(Ordering::Acquire) >= expected }
        },
        timeout,
    )
    .await;
}

async fn await_execution_unhealthy(client: &PolymarketExecutionClient, timeout: Duration) {
    wait_until_async(|| async { !client.is_connected() }, timeout).await;
}

#[rstest]
#[tokio::test]
async fn test_exec_client_get_account_none_initially() {
    let state = TestServerState::default();
    let addr = start_mock_server(state).await;
    let (client, _rx, _cache) = create_test_execution_client(addr);

    assert!(client.get_account().is_none());
}

#[rstest]
#[tokio::test]
async fn test_exec_client_get_account_after_cache_add() {
    let state = TestServerState::default();
    let addr = start_mock_server(state).await;
    let (client, _rx, cache) = create_test_execution_client(addr);

    add_test_account_to_cache(&cache, AccountId::from("POLYMARKET-001"));

    assert!(client.get_account().is_some());
}

#[rstest]
#[tokio::test]
async fn test_generate_order_status_reports_empty_without_instruments() {
    let state = TestServerState::default();
    let addr = start_mock_server(state).await;
    let (client, _rx, _cache) = create_test_execution_client(addr);

    let cmd = GenerateOrderStatusReports {
        command_id: UUID4::new(),
        ts_init: UnixNanos::default(),
        open_only: false,
        instrument_id: None,
        start: None,
        end: None,
        params: None,
        log_receipt_level: LogLevel::Info,
        correlation_id: None,
        causation_id: None,
    };

    let reports = client.generate_order_status_reports(&cmd).await.unwrap();

    // Without loaded instruments, orders cannot be resolved to instrument IDs
    assert!(reports.is_empty());
}

#[rstest]
#[tokio::test]
async fn test_generate_order_status_reports_caps_from_rest_fill_without_emitting_it() {
    let venue_order_id_str = "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef12";
    let state = TestServerState::default();
    let mut order = load_json("http_open_orders_page.json")["data"][0].clone();
    order["id"] = Value::String(venue_order_id_str.to_string());
    order["status"] = Value::String("MATCHED".to_string());
    order["original_size"] = Value::String("10.0000".to_string());
    order["size_matched"] = Value::String("10.0000".to_string());
    *state.orders_response_override.lock().await = Some(json!({
        "data": [order],
        "next_cursor": "LTE=",
    }));
    *state.trades_response_override.lock().await = Some(recovery_trades_response(
        venue_order_id_str,
        "10.0000",
        "0.5000",
    ));
    let addr = start_mock_server(state.clone()).await;
    let (mut client, mut rx, cache) = create_test_execution_client(addr);
    client.start().unwrap();

    let instrument_id = InstrumentId::from("TEST-TOKEN.POLYMARKET");
    add_instrument_to_cache_with_size_precision(&cache, instrument_id, 4);
    let instrument = cache.borrow().instrument(&instrument_id).unwrap().clone();
    client.on_instrument(instrument);
    let mut cached_order = make_limit_order(
        "O-OPEN-CHECK-CONFIRMED",
        instrument_id,
        OrderSide::Buy,
        false,
        false,
        false,
        TimeInForce::Gtc,
    );
    cache
        .borrow_mut()
        .add_order(cached_order.clone(), None, None, false)
        .unwrap();
    submit_and_accept_order(&cache, &mut cached_order, venue_order_id_str);
    let cmd = GenerateOrderStatusReports {
        command_id: UUID4::new(),
        ts_init: UnixNanos::default(),
        open_only: false,
        instrument_id: Some(instrument_id),
        start: Some(UnixNanos::from(2_000_000_000_000_000_000u64)),
        end: Some(UnixNanos::from(2_000_000_100_000_000_000u64)),
        params: None,
        log_receipt_level: LogLevel::Info,
        correlation_id: None,
        causation_id: None,
    };

    let reports = client.generate_order_status_reports(&cmd).await.unwrap();

    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].order_status, OrderStatus::Filled);
    assert_eq!(reports[0].filled_qty, Quantity::from("10.0000"));
    assert!(rx.try_recv().is_err());
    let query = state.last_query.lock().await;
    assert!(!query.contains_key("after"));
    assert!(!query.contains_key("before"));
}

#[rstest]
#[tokio::test]
async fn test_generate_fill_reports_empty_without_instruments() {
    let state = TestServerState::default();
    let addr = start_mock_server(state).await;
    let (client, _rx, _cache) = create_test_execution_client(addr);

    let cmd = GenerateFillReports {
        command_id: UUID4::new(),
        ts_init: UnixNanos::default(),
        instrument_id: None,
        venue_order_id: None,
        start: None,
        end: None,
        params: None,
        log_receipt_level: LogLevel::Info,
        correlation_id: None,
        causation_id: None,
    };

    let reports = client.generate_fill_reports(cmd).await.unwrap();

    assert!(reports.is_empty());
}

#[rstest]
#[tokio::test]
async fn test_generate_position_status_reports_always_empty() {
    let state = TestServerState::default();
    let addr = start_mock_server(state).await;
    let (client, _rx, _cache) = create_test_execution_client(addr);

    let cmd = GeneratePositionStatusReports {
        command_id: UUID4::new(),
        ts_init: UnixNanos::default(),
        instrument_id: None,
        start: None,
        end: None,
        params: None,
        log_receipt_level: LogLevel::Info,
        correlation_id: None,
        causation_id: None,
    };

    let reports = client.generate_position_status_reports(&cmd).await.unwrap();

    // Polymarket has no position endpoint
    assert!(reports.is_empty());
}

#[rstest]
#[tokio::test]
async fn test_generate_order_status_report_single_requires_venue_order_id() {
    let state = TestServerState::default();
    let addr = start_mock_server(state).await;
    let (client, _rx, _cache) = create_test_execution_client(addr);

    let cmd = GenerateOrderStatusReport {
        command_id: UUID4::new(),
        ts_init: UnixNanos::default(),
        instrument_id: None,
        client_order_id: None,
        venue_order_id: None,
        params: None,
        correlation_id: None,
        causation_id: None,
    };

    let result = client.generate_order_status_report(&cmd).await.unwrap();

    assert!(result.is_none());
}

#[rstest]
#[tokio::test]
async fn test_generate_order_status_report_single_requires_instrument_id() {
    let state = TestServerState::default();
    let addr = start_mock_server(state).await;
    let (client, _rx, _cache) = create_test_execution_client(addr);

    let cmd = GenerateOrderStatusReport {
        command_id: UUID4::new(),
        ts_init: UnixNanos::default(),
        instrument_id: None,
        client_order_id: None,
        venue_order_id: Some(VenueOrderId::from("0x123")),
        params: None,
        correlation_id: None,
        causation_id: None,
    };

    let result = client.generate_order_status_report(&cmd).await.unwrap();

    assert!(result.is_none());
}

#[rstest]
#[tokio::test]
async fn test_generate_order_status_report_single_returns_report() {
    let state = TestServerState::default();
    let addr = start_mock_server(state).await;
    let (client, _rx, _cache) = create_test_execution_client(addr);

    let instrument_id = InstrumentId::from("TEST-TOKEN.POLYMARKET");
    let cmd = GenerateOrderStatusReport {
        command_id: UUID4::new(),
        ts_init: UnixNanos::default(),
        instrument_id: Some(instrument_id),
        client_order_id: None,
        venue_order_id: Some(VenueOrderId::from("0x123")),
        params: None,
        correlation_id: None,
        causation_id: None,
    };

    let result = client.generate_order_status_report(&cmd).await.unwrap();

    let report = result.unwrap();
    assert_eq!(report.instrument_id, instrument_id);
    assert_eq!(report.account_id, AccountId::from("POLYMARKET-001"));
    assert_eq!(report.order_side, OrderSide::Buy,);
    assert_eq!(report.order_type, OrderType::Limit,);
    assert_eq!(report.filled_qty, Quantity::zero(4));
    assert!(report.price.is_some());
}

#[rstest]
#[tokio::test]
async fn test_generate_order_status_report_defers_while_trade_is_unconfirmed() {
    let venue_order_id_str = "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef12";
    let state = TestServerState::default();
    *state.single_order_response.lock().await = Some(Value::Null);
    let mut trades = recovery_trades_response(venue_order_id_str, "10.0000", "0.5000");
    trades["data"][0]["status"] = Value::String("MINED".to_string());
    *state.trades_response_override.lock().await = Some(trades);
    let addr = start_mock_server(state).await;
    let (mut client, _rx, cache) = create_test_execution_client(addr);

    let instrument_id = InstrumentId::from("TEST-TOKEN.POLYMARKET");
    add_instrument_to_cache_with_size_precision(&cache, instrument_id, 4);
    let instrument = cache.borrow().instrument(&instrument_id).unwrap().clone();
    client.on_instrument(instrument);

    let venue_order_id = VenueOrderId::from(venue_order_id_str);
    let client_order_id = ClientOrderId::from("O-RECOVERY-PENDING");
    let mut order = make_limit_order(
        client_order_id.as_str(),
        instrument_id,
        OrderSide::Buy,
        false,
        false,
        false,
        TimeInForce::Gtc,
    );
    cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .unwrap();
    submit_and_accept_order(&cache, &mut order, venue_order_id_str);
    let cmd = GenerateOrderStatusReport {
        command_id: UUID4::new(),
        ts_init: UnixNanos::default(),
        instrument_id: Some(instrument_id),
        client_order_id: Some(client_order_id),
        venue_order_id: Some(venue_order_id),
        params: None,
        correlation_id: None,
        causation_id: None,
    };

    let report = client
        .generate_order_status_report(&cmd)
        .await
        .unwrap()
        .unwrap();

    assert_eq!(report.order_status, OrderStatus::Accepted);
    assert_eq!(report.filled_qty, Quantity::zero(4));
}

#[rstest]
#[tokio::test]
async fn test_generate_active_order_report_recovers_confirmed_rest_fill() {
    let venue_order_id_str = "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef12";
    let state = TestServerState::default();
    let mut order = load_json("http_open_order.json");
    order["id"] = Value::String(venue_order_id_str.to_string());
    order["status"] = Value::String("MATCHED".to_string());
    order["original_size"] = Value::String("10.0000".to_string());
    order["size_matched"] = Value::String("10.0000".to_string());
    *state.single_order_response.lock().await = Some(order);
    *state.trades_response_override.lock().await = Some(recovery_trades_response(
        venue_order_id_str,
        "10.0000",
        "0.5000",
    ));
    let addr = start_mock_server(state).await;
    let (mut client, _rx, cache) = create_test_execution_client(addr);

    let instrument_id = InstrumentId::from("TEST-TOKEN.POLYMARKET");
    add_instrument_to_cache_with_size_precision(&cache, instrument_id, 4);
    let instrument = cache.borrow().instrument(&instrument_id).unwrap().clone();
    client.on_instrument(instrument);
    let venue_order_id = VenueOrderId::from(venue_order_id_str);
    let client_order_id = ClientOrderId::from("O-ACTIVE-CONFIRMED");
    let mut cached_order = make_limit_order(
        client_order_id.as_str(),
        instrument_id,
        OrderSide::Buy,
        false,
        false,
        false,
        TimeInForce::Gtc,
    );
    cache
        .borrow_mut()
        .add_order(cached_order.clone(), None, None, false)
        .unwrap();
    submit_and_accept_order(&cache, &mut cached_order, venue_order_id_str);
    let cmd = GenerateOrderStatusReport {
        command_id: UUID4::new(),
        ts_init: UnixNanos::default(),
        instrument_id: Some(instrument_id),
        client_order_id: Some(client_order_id),
        venue_order_id: Some(venue_order_id),
        params: None,
        correlation_id: None,
        causation_id: None,
    };

    let report = client
        .generate_order_status_report(&cmd)
        .await
        .unwrap()
        .unwrap();

    assert_eq!(report.order_status, OrderStatus::Filled);
    assert_eq!(report.filled_qty, Quantity::from("10.0000"));
}

#[rstest]
#[tokio::test]
async fn test_generate_order_status_report_recovers_filled_from_trades() {
    // A terminal order can be absent from an individual lookup after a fill,
    // so trade history must resolve the local `ACCEPTED` state to `Filled`.
    let state = TestServerState::default();
    *state.single_order_response.lock().await = Some(Value::Null);
    let addr = start_mock_server(state).await;
    let (mut client, _rx, cache) = create_test_execution_client(addr);

    let instrument_id = InstrumentId::from("TEST-TOKEN.POLYMARKET");
    add_instrument_to_cache_with_size_precision(&cache, instrument_id, 4);
    let instrument = cache.borrow().instrument(&instrument_id).unwrap().clone();
    client.on_instrument(instrument);

    let venue_order_id =
        VenueOrderId::from("0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef12");
    let client_order_id = ClientOrderId::from("O-RECOVERY-FILLED");
    let mut order = OrderAny::Limit(LimitOrder::new(
        TraderId::from("TESTER-001"),
        StrategyId::from("S-001"),
        instrument_id,
        client_order_id,
        OrderSide::Buy,
        Quantity::new(10.0, 4),
        Price::from("0.5000"),
        TimeInForce::Gtc,
        None,
        false,
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
    cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .unwrap();
    submit_and_accept_order(&cache, &mut order, venue_order_id.as_str());

    let cmd = GenerateOrderStatusReport {
        command_id: UUID4::new(),
        ts_init: UnixNanos::default(),
        instrument_id: Some(instrument_id),
        client_order_id: Some(client_order_id),
        venue_order_id: Some(venue_order_id),
        params: None,
        correlation_id: None,
        causation_id: None,
    };

    let report = client
        .generate_order_status_report(&cmd)
        .await
        .unwrap()
        .expect("recovery should produce a report");

    assert_eq!(report.order_status, OrderStatus::Filled);
    assert_eq!(report.venue_order_id, venue_order_id);
    assert_eq!(report.filled_qty, Quantity::new(10.0, 4));
    assert_eq!(report.quantity, Quantity::new(10.0, 4));
    assert_eq!(report.order_side, OrderSide::Buy);
    assert_eq!(report.avg_px, Some(dec!(0.5)));
}

#[rstest]
#[tokio::test]
async fn test_generate_order_status_report_recovers_canceled_when_no_trades() {
    // When the venue has no record of the order and no trades exist for it,
    // surface `Canceled` (not `Rejected`) so the engine retires the local entry
    // gracefully instead of dropping it via the not-found-at-venue path.
    let state = TestServerState::default();
    *state.single_order_response.lock().await = Some(Value::Null);
    let addr = start_mock_server(state).await;
    let (mut client, _rx, cache) = create_test_execution_client(addr);

    let instrument_id = InstrumentId::from("TEST-TOKEN.POLYMARKET");
    add_instrument_to_cache_with_size_precision(&cache, instrument_id, 4);
    let instrument = cache.borrow().instrument(&instrument_id).unwrap().clone();
    client.on_instrument(instrument);

    let venue_order_id =
        VenueOrderId::from("0xnotrade000000000000000000000000000000000000000000000000000000ff");
    let client_order_id = ClientOrderId::from("O-RECOVERY-CANCELED");
    let mut order = OrderAny::Limit(LimitOrder::new(
        TraderId::from("TESTER-001"),
        StrategyId::from("S-001"),
        instrument_id,
        client_order_id,
        OrderSide::Buy,
        Quantity::new(10.0, 4),
        Price::from("0.5000"),
        TimeInForce::Gtc,
        None,
        false,
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
    cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .unwrap();
    submit_and_accept_order(&cache, &mut order, venue_order_id.as_str());

    let cmd = GenerateOrderStatusReport {
        command_id: UUID4::new(),
        ts_init: UnixNanos::default(),
        instrument_id: Some(instrument_id),
        client_order_id: Some(client_order_id),
        venue_order_id: Some(venue_order_id),
        params: None,
        correlation_id: None,
        causation_id: None,
    };

    let report = client
        .generate_order_status_report(&cmd)
        .await
        .unwrap()
        .expect("recovery should produce a report");

    assert_eq!(report.order_status, OrderStatus::Canceled);
    assert_eq!(report.venue_order_id, venue_order_id);
    assert_eq!(
        report.cancel_reason.as_deref(),
        Some("ORDER_NOT_FOUND_AT_VENUE"),
    );
}

#[rstest]
#[tokio::test]
async fn test_generate_order_status_report_returns_none_without_cached_order() {
    // No trades, no cached order: nothing to recover. Defer to the engine's
    // existing not-found-at-venue path (matches docs and Python behavior).
    let state = TestServerState::default();
    *state.single_order_response.lock().await = Some(Value::Null);
    let addr = start_mock_server(state).await;
    let (mut client, _rx, cache) = create_test_execution_client(addr);

    let instrument_id = InstrumentId::from("TEST-TOKEN.POLYMARKET");
    add_instrument_to_cache_with_size_precision(&cache, instrument_id, 4);
    let instrument = cache.borrow().instrument(&instrument_id).unwrap().clone();
    client.on_instrument(instrument);

    let venue_order_id =
        VenueOrderId::from("0xnocache000000000000000000000000000000000000000000000000000000ff");
    let cmd = GenerateOrderStatusReport {
        command_id: UUID4::new(),
        ts_init: UnixNanos::default(),
        instrument_id: Some(instrument_id),
        client_order_id: None,
        venue_order_id: Some(venue_order_id),
        params: None,
        correlation_id: None,
        causation_id: None,
    };

    let result = client.generate_order_status_report(&cmd).await.unwrap();
    assert!(result.is_none());
}

fn recovery_trades_response(venue_order_id: &str, size: &str, price: &str) -> Value {
    json!({
        "data": [{
            "id": "trade-recovery",
            "taker_order_id": venue_order_id,
            "market": "0xdd22472e552920b8438158ea7238bfadfa4f736aa4cee91a6b86c39ead110917",
            "asset_id": "71321045679252212594626385532706912750332728571942532289631379312455583992563",
            "side": "BUY",
            "size": size,
            "fee_rate_bps": "0",
            "price": price,
            "status": "CONFIRMED",
            "match_time": "2024-01-01T00:00:00Z",
            "last_update": "2024-01-01T00:00:10Z",
            "outcome": "Yes",
            "bucket_index": 0,
            "owner": "00000000-0000-0000-0000-000000000001",
            "maker_address": "0x70997970c51812dc3a010c7d01b50e0d17dc79c8",
            "transaction_hash": "0xabc123",
            "maker_orders": [],
            "trader_side": "TAKER",
        }],
        "next_cursor": "LTE=",
    })
}

#[rstest]
#[tokio::test]
async fn test_generate_order_status_report_recovers_filled_with_dust_snap() {
    // CLOB cent-tick truncation leaves the confirmed trade within DUST_SNAP_THRESHOLD below the
    // cached quantity. Recovery must preserve the economic fill and normalize only order quantity.
    let venue_order_id_str = "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef12";
    let state = TestServerState::default();
    *state.single_order_response.lock().await = Some(Value::Null);
    *state.trades_response_override.lock().await = Some(recovery_trades_response(
        venue_order_id_str,
        "9.9950",
        "0.5000",
    ));
    let addr = start_mock_server(state).await;
    let (mut client, _rx, cache) = create_test_execution_client(addr);

    let instrument_id = InstrumentId::from("TEST-TOKEN.POLYMARKET");
    add_instrument_to_cache_with_size_precision(&cache, instrument_id, 4);
    let instrument = cache.borrow().instrument(&instrument_id).unwrap().clone();
    client.on_instrument(instrument);

    let venue_order_id = VenueOrderId::from(venue_order_id_str);
    let client_order_id = ClientOrderId::from("O-RECOVERY-DUST");
    let mut order = OrderAny::Limit(LimitOrder::new(
        TraderId::from("TESTER-001"),
        StrategyId::from("S-001"),
        instrument_id,
        client_order_id,
        OrderSide::Buy,
        Quantity::new(10.0, 4),
        Price::from("0.5000"),
        TimeInForce::Gtc,
        None,
        false,
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
    cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .unwrap();
    submit_and_accept_order(&cache, &mut order, venue_order_id_str);

    let cmd = GenerateOrderStatusReport {
        command_id: UUID4::new(),
        ts_init: UnixNanos::default(),
        instrument_id: Some(instrument_id),
        client_order_id: Some(client_order_id),
        venue_order_id: Some(venue_order_id),
        params: None,
        correlation_id: None,
        causation_id: None,
    };

    let report = client
        .generate_order_status_report(&cmd)
        .await
        .unwrap()
        .expect("recovery should produce a report");

    assert_eq!(report.order_status, OrderStatus::Filled);
    assert_eq!(report.filled_qty, Quantity::from("9.9950"));
    assert_eq!(report.quantity, Quantity::from("9.9950"));
}

#[rstest]
#[tokio::test]
async fn test_generate_order_status_report_recovers_canceled_with_partial_fill() {
    // Recovered fills fall short of cached quantity by more than dust:
    // surface Canceled with the partial filled_qty preserved.
    let venue_order_id_str = "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef12";
    let state = TestServerState::default();
    *state.single_order_response.lock().await = Some(Value::Null);
    *state.trades_response_override.lock().await = Some(recovery_trades_response(
        venue_order_id_str,
        "5.0000",
        "0.5000",
    ));
    let addr = start_mock_server(state).await;
    let (mut client, _rx, cache) = create_test_execution_client(addr);

    let instrument_id = InstrumentId::from("TEST-TOKEN.POLYMARKET");
    add_instrument_to_cache_with_size_precision(&cache, instrument_id, 4);
    let instrument = cache.borrow().instrument(&instrument_id).unwrap().clone();
    client.on_instrument(instrument);

    let venue_order_id = VenueOrderId::from(venue_order_id_str);
    let client_order_id = ClientOrderId::from("O-RECOVERY-PARTIAL");
    let mut order = OrderAny::Limit(LimitOrder::new(
        TraderId::from("TESTER-001"),
        StrategyId::from("S-001"),
        instrument_id,
        client_order_id,
        OrderSide::Buy,
        Quantity::new(10.0, 4),
        Price::from("0.5000"),
        TimeInForce::Gtc,
        None,
        false,
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
    cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .unwrap();
    submit_and_accept_order(&cache, &mut order, venue_order_id_str);

    let cmd = GenerateOrderStatusReport {
        command_id: UUID4::new(),
        ts_init: UnixNanos::default(),
        instrument_id: Some(instrument_id),
        client_order_id: Some(client_order_id),
        venue_order_id: Some(venue_order_id),
        params: None,
        correlation_id: None,
        causation_id: None,
    };

    let report = client
        .generate_order_status_report(&cmd)
        .await
        .unwrap()
        .expect("recovery should produce a report");

    assert_eq!(report.order_status, OrderStatus::Canceled);
    assert_eq!(report.filled_qty, Quantity::new(5.0, 4));
    assert_eq!(report.quantity, Quantity::new(10.0, 4));
    assert_eq!(report.avg_px, Some(dec!(0.5)));
    assert!(report.cancel_reason.is_none());
}

#[rstest]
#[tokio::test]
async fn test_generate_order_status_report_resolves_via_venue_order_id_index() {
    // Command supplies only `venue_order_id`; recovery must look up the
    // cached order through the cache's venue->client index instead of
    // synthesizing an external order or returning None.
    let venue_order_id_str = "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef12";
    let state = TestServerState::default();
    *state.single_order_response.lock().await = Some(Value::Null);
    *state.trades_response_override.lock().await = Some(recovery_trades_response(
        venue_order_id_str,
        "10.0000",
        "0.5000",
    ));
    let addr = start_mock_server(state).await;
    let (mut client, _rx, cache) = create_test_execution_client(addr);

    let instrument_id = InstrumentId::from("TEST-TOKEN.POLYMARKET");
    add_instrument_to_cache_with_size_precision(&cache, instrument_id, 4);
    let instrument = cache.borrow().instrument(&instrument_id).unwrap().clone();
    client.on_instrument(instrument);

    let venue_order_id = VenueOrderId::from(venue_order_id_str);
    let client_order_id = ClientOrderId::from("O-RECOVERY-VENUE-ONLY");
    let mut order = OrderAny::Limit(LimitOrder::new(
        TraderId::from("TESTER-001"),
        StrategyId::from("S-001"),
        instrument_id,
        client_order_id,
        OrderSide::Buy,
        Quantity::new(10.0, 4),
        Price::from("0.5000"),
        TimeInForce::Gtc,
        None,
        false,
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
    cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .unwrap();
    submit_and_accept_order(&cache, &mut order, venue_order_id_str);

    let cmd = GenerateOrderStatusReport {
        command_id: UUID4::new(),
        ts_init: UnixNanos::default(),
        instrument_id: Some(instrument_id),
        client_order_id: None,
        venue_order_id: Some(venue_order_id),
        params: None,
        correlation_id: None,
        causation_id: None,
    };

    let report = client
        .generate_order_status_report(&cmd)
        .await
        .unwrap()
        .expect("recovery should produce a report");

    assert_eq!(report.order_status, OrderStatus::Filled);
    assert_eq!(report.client_order_id, Some(client_order_id));
    assert_eq!(report.quantity, Quantity::new(10.0, 4));
    assert_eq!(report.filled_qty, Quantity::new(10.0, 4));
}

#[rstest]
#[tokio::test]
async fn test_generate_account_state_emits_event() {
    let state = TestServerState::default();
    let addr = start_mock_server(state).await;
    let (mut client, mut rx, _cache) = create_test_execution_client(addr);

    client.start().unwrap();

    let pusd = Currency::pUSD();
    let balances = vec![AccountBalance::new(
        Money::new(1000.0, pusd),
        Money::new(0.0, pusd),
        Money::new(1000.0, pusd),
    )];
    client
        .generate_account_state(balances, vec![], true, UnixNanos::default())
        .unwrap();

    let event = rx.try_recv().unwrap();
    assert!(matches!(event, ExecutionEvent::Account(_)));
}

#[rstest]
#[tokio::test]
async fn test_modify_order_emits_rejection() {
    let state = TestServerState::default();
    let addr = start_mock_server(state).await;
    let (mut client, mut rx, cache) = create_test_execution_client(addr);

    client.start().unwrap();
    add_test_account_to_cache(&cache, AccountId::from("POLYMARKET-001"));

    // Add a test order to cache so modify can find it
    let instrument_id = InstrumentId::from("TEST-TOKEN.POLYMARKET");
    let client_order_id = ClientOrderId::from("O-001");
    let order = OrderAny::Limit(LimitOrder::new(
        TraderId::from("TESTER-001"),
        StrategyId::from("S-001"),
        instrument_id,
        client_order_id,
        OrderSide::Buy,
        Quantity::from("100"),
        Price::from("0.50"),
        TimeInForce::Gtc,
        None,  // expire_time
        false, // post_only
        false, // reduce_only
        false, // quote_quantity
        None,  // display_qty
        None,  // emulation_trigger
        None,  // trigger_instrument_id
        None,  // contingency_type
        None,  // order_list_id
        None,  // linked_order_ids
        None,  // parent_order_id
        None,  // exec_algorithm_id
        None,  // exec_algorithm_params
        None,  // exec_spawn_id
        None,  // tags
        UUID4::new(),
        UnixNanos::default(),
    ));
    cache
        .borrow_mut()
        .add_order(order, None, None, false)
        .unwrap();

    let cmd = ModifyOrder {
        trader_id: TraderId::from("TESTER-001"),
        client_id: Some(*POLYMARKET_CLIENT_ID),
        strategy_id: StrategyId::from("S-001"),
        instrument_id,
        client_order_id,
        venue_order_id: None,
        quantity: Some(Quantity::from("50")),
        price: None,
        trigger_price: None,
        command_id: UUID4::new(),
        ts_init: UnixNanos::default(),
        params: None,
        correlation_id: None,
        causation_id: None,
    };

    client.modify_order(cmd).unwrap();

    // Should receive an order modify rejected event
    let event = rx.try_recv().unwrap();
    match event {
        ExecutionEvent::Order(order_event) => {
            assert!(
                matches!(order_event, OrderEventAny::ModifyRejected(_)),
                "Expected ModifyRejected, was {order_event:?}"
            );
        }
        other => panic!("Expected Order event, was {other:?}"),
    }
}

#[rstest]
#[tokio::test]
async fn test_submit_market_order_denied_buy_without_quote_quantity() {
    let state = TestServerState::default();
    let addr = start_mock_server(state).await;
    let (mut client, mut rx, cache) = create_test_execution_client(addr);

    client.start().unwrap();

    let instrument_id = InstrumentId::from("TEST-TOKEN.POLYMARKET");
    let client_order_id = ClientOrderId::from("O-002");
    let order = OrderAny::Market(MarketOrder::new(
        TraderId::from("TESTER-001"),
        StrategyId::from("S-001"),
        instrument_id,
        client_order_id,
        OrderSide::Buy,
        Quantity::from("100"),
        TimeInForce::Ioc,
        UUID4::new(),
        UnixNanos::default(),
        false, // reduce_only
        false, // quote_quantity - BUY requires true
        None,  // contingency_type
        None,  // order_list_id
        None,  // linked_order_ids
        None,  // parent_order_id
        None,  // exec_algorithm_id
        None,  // exec_algorithm_params
        None,  // exec_spawn_id
        None,  // tags
    ));

    let init_event = order.init_event().clone();
    cache
        .borrow_mut()
        .add_order(order, None, None, false)
        .unwrap();

    let cmd = SubmitOrder::new(
        TraderId::from("TESTER-001"),
        Some(*POLYMARKET_CLIENT_ID),
        StrategyId::from("S-001"),
        instrument_id,
        client_order_id,
        init_event,
        None, // exec_algorithm_id
        None, // position_id
        None, // params
        UUID4::new(),
        UnixNanos::default(),
        None, // correlation_id
    );

    client.submit_order(cmd).unwrap();

    let event = rx.try_recv().unwrap();
    assert_order_event(event, "Denied");
}

#[rstest]
#[tokio::test]
async fn test_submit_market_order_denied_sell_with_quote_quantity() {
    let state = TestServerState::default();
    let addr = start_mock_server(state).await;
    let (mut client, mut rx, cache) = create_test_execution_client(addr);

    client.start().unwrap();

    let instrument_id = InstrumentId::from("TEST-TOKEN.POLYMARKET");
    let client_order_id = ClientOrderId::from("O-MKT-SELL-QQ");
    let order = OrderAny::Market(MarketOrder::new(
        TraderId::from("TESTER-001"),
        StrategyId::from("S-001"),
        instrument_id,
        client_order_id,
        OrderSide::Sell,
        Quantity::from("100"),
        TimeInForce::Ioc,
        UUID4::new(),
        UnixNanos::default(),
        false, // reduce_only
        true,  // quote_quantity - SELL requires false
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
    ));

    let init_event = order.init_event().clone();
    cache
        .borrow_mut()
        .add_order(order, None, None, false)
        .unwrap();

    let cmd = SubmitOrder::new(
        TraderId::from("TESTER-001"),
        Some(*POLYMARKET_CLIENT_ID),
        StrategyId::from("S-001"),
        instrument_id,
        client_order_id,
        init_event,
        None,
        None,
        None,
        UUID4::new(),
        UnixNanos::default(),
        None, // correlation_id
    );

    client.submit_order(cmd).unwrap();

    let event = rx.try_recv().unwrap();
    assert_order_event(event, "Denied");
}

fn make_market_order(
    client_order_id: &str,
    instrument_id: InstrumentId,
    side: OrderSide,
    quote_quantity: bool,
) -> OrderAny {
    make_market_order_with_time_in_force(
        client_order_id,
        instrument_id,
        side,
        quote_quantity,
        TimeInForce::Ioc,
    )
}

fn make_market_order_with_time_in_force(
    client_order_id: &str,
    instrument_id: InstrumentId,
    side: OrderSide,
    quote_quantity: bool,
    time_in_force: TimeInForce,
) -> OrderAny {
    OrderTestBuilder::new(OrderType::Market)
        .trader_id(TraderId::from("TESTER-001"))
        .strategy_id(StrategyId::from("S-001"))
        .instrument_id(instrument_id)
        .client_order_id(ClientOrderId::from(client_order_id))
        .side(side)
        .quantity(Quantity::new(10.0, 0))
        .time_in_force(time_in_force)
        .quote_quantity(quote_quantity)
        .build()
}

#[rstest]
#[tokio::test]
async fn test_submit_market_order_denied_unsupported_time_in_force() {
    let state = TestServerState::default();
    let addr = start_mock_server(state.clone()).await;
    let (mut client, mut rx, cache) = create_test_execution_client(addr);
    client.start().unwrap();

    let instrument_id = InstrumentId::from("TEST-TOKEN.POLYMARKET");
    let order = make_market_order_with_time_in_force(
        "O-MKT-GTC",
        instrument_id,
        OrderSide::Sell,
        false,
        TimeInForce::Gtc,
    );
    cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .unwrap();
    let cmd = make_submit_cmd(&order, instrument_id);

    client.submit_order(cmd).unwrap();

    let event = rx.try_recv().unwrap();
    assert_order_event(event, "Denied");
    assert_eq!(*state.order_post_count.lock().await, 0);
}

#[rstest]
#[case::ioc(TimeInForce::Ioc, "FAK")]
#[case::fok(TimeInForce::Fok, "FOK")]
#[tokio::test]
async fn test_submit_market_order_posts_order_type_from_time_in_force(
    #[case] time_in_force: TimeInForce,
    #[case] expected_order_type: &str,
) {
    let state = TestServerState::default();
    let addr = start_mock_server(state.clone()).await;
    let (mut client, mut rx, cache) = create_test_execution_client(addr);
    client.start().unwrap();

    let instrument_id = InstrumentId::from("TEST-TOKEN.POLYMARKET");
    add_instrument_to_cache(&cache, instrument_id);

    let order = make_market_order_with_time_in_force(
        "O-MKT-TIF",
        instrument_id,
        OrderSide::Sell,
        false,
        time_in_force,
    );
    cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .unwrap();
    let cmd = make_submit_cmd(&order, instrument_id);

    client.submit_order(cmd).unwrap();

    assert_order_event(recv_execution_event(&mut rx).await, "Submitted");
    assert_order_event(recv_execution_event(&mut rx).await, "Accepted");

    let body = state.last_body.lock().await.clone().unwrap();
    assert_eq!(
        body.get("orderType").and_then(Value::as_str),
        Some(expected_order_type),
    );
}

#[rstest]
#[tokio::test]
async fn test_submit_market_order_buy_accepted() {
    let state = TestServerState::default();
    let addr = start_mock_server(state.clone()).await;
    let (mut client, mut rx, cache) = create_test_execution_client(addr);
    client.start().unwrap();

    let instrument_id = InstrumentId::from("TEST-TOKEN.POLYMARKET");
    add_instrument_to_cache(&cache, instrument_id);

    let order = make_market_order("O-MKT-BUY", instrument_id, OrderSide::Buy, true);
    cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .unwrap();
    let cmd = make_submit_cmd(&order, instrument_id);

    client.submit_order(cmd).unwrap();

    // Market orders: Submitted comes from the async task (after book fetch)
    let event = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .unwrap()
        .unwrap();
    assert_order_event(event, "Submitted");

    // Updated (quote-to-base conversion for BUY quote_quantity orders)
    let event = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .unwrap()
        .unwrap();
    assert_order_event(event, "Updated");

    // Accepted (async, after HTTP post)
    let event = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .unwrap()
        .unwrap();
    assert_order_event(event, "Accepted");
}

#[rstest]
#[tokio::test]
async fn test_submit_market_order_balance_failure_is_denied_before_submission() {
    let state = TestServerState::default();
    *state.balance_response_status.lock().await = StatusCode::INTERNAL_SERVER_ERROR;
    let addr = start_mock_server(state.clone()).await;
    let (mut client, mut rx, cache) = create_test_execution_client(addr);
    client.start().unwrap();

    let instrument_id = InstrumentId::from("TEST-TOKEN.POLYMARKET");
    add_instrument_to_cache(&cache, instrument_id);
    let order = make_market_order("O-MKT-BALANCE", instrument_id, OrderSide::Buy, true);
    cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .unwrap();

    client
        .submit_order(make_submit_cmd(&order, instrument_id))
        .unwrap();

    assert_order_event(recv_execution_event(&mut rx).await, "Denied");
    assert_eq!(*state.order_post_count.lock().await, 0);
}

#[rstest]
#[tokio::test]
async fn test_submit_market_order_buy_quote_to_base_conversion() {
    let state = TestServerState::default();
    // Book with a single ask at 0.50 so crossing price is exactly 0.50
    *state.book_response.lock().await = Some(json!({
        "bids": [{"price": "0.48", "size": "100.00"}],
        "asks": [{"price": "0.50", "size": "100.00"}]
    }));
    let addr = start_mock_server(state.clone()).await;
    let (mut client, mut rx, cache) = create_test_execution_client(addr);
    client.start().unwrap();

    let instrument_id = InstrumentId::from("TEST-TOKEN.POLYMARKET");
    add_instrument_to_cache(&cache, instrument_id);

    // BUY 10 USDC worth with quote_quantity=true
    let order = make_market_order("O-MKT-QTY", instrument_id, OrderSide::Buy, true);
    cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .unwrap();
    let cmd = make_submit_cmd(&order, instrument_id);

    client.submit_order(cmd).unwrap();

    // Submitted
    let event = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .unwrap()
        .unwrap();
    assert_order_event(event, "Submitted");

    // Updated: quote-to-base conversion
    let event = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .unwrap()
        .unwrap();
    let updated = assert_order_event(event, "Updated");

    // Verify the Updated event has the correct base quantity and is_quote_quantity=false
    if let OrderEventAny::Updated(ref u) = updated {
        // 10 USDC / 0.50 price = 20 shares (instrument has size_precision=0)
        assert_eq!(u.quantity, Quantity::from(20));
        assert!(
            !u.is_quote_quantity,
            "is_quote_quantity should be false after conversion"
        );
    } else {
        panic!("Expected Updated event");
    }

    // Accepted
    let event = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .unwrap()
        .unwrap();
    assert_order_event(event, "Accepted");
}

#[rstest]
#[tokio::test]
async fn test_submit_market_buy_quote_to_base_uses_signed_taker_amount() {
    // Regression: a multi-level book walk produces a larger total than the
    // signed taker_amount (which divides at a single crossing price). The
    // OrderUpdated must reflect what the venue can actually fill, i.e. the
    // signed amount, otherwise the order is over-stated for callers and the
    // fill tracker.
    //
    // 10 pUSD BUY into asks [(0.50, 10 shares), (0.99, 100 shares)]:
    //   Book walk: 10 @ 0.50 (5 pUSD) + 5/0.99 = 5.05 @ 0.99 -> 15.05 shares
    //   Signed:    10 / 0.99 = 10.10 shares
    // size_precision=0 truncates: book walk = 15, signed = 10.
    let state = TestServerState::default();
    *state.book_response.lock().await = Some(json!({
        "bids": [{"price": "0.48", "size": "100.00"}],
        "asks": [
            {"price": "0.50", "size": "10.00"},
            {"price": "0.99", "size": "100.00"},
        ]
    }));
    let addr = start_mock_server(state.clone()).await;
    let (mut client, mut rx, cache) = create_test_execution_client(addr);
    client.start().unwrap();

    let instrument_id = InstrumentId::from("TEST-TOKEN.POLYMARKET");
    add_instrument_to_cache(&cache, instrument_id);

    let order = make_market_order("O-MKT-MULTI", instrument_id, OrderSide::Buy, true);
    cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .unwrap();
    let cmd = make_submit_cmd(&order, instrument_id);

    client.submit_order(cmd).unwrap();

    let event = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .unwrap()
        .unwrap();
    assert_order_event(event, "Submitted");

    let event = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .unwrap()
        .unwrap();
    let updated = assert_order_event(event, "Updated");

    if let OrderEventAny::Updated(ref u) = updated {
        // 10 pUSD / 0.99 crossing = 10.10 shares -> 10 at size_precision=0.
        // Book walk would have produced 15 shares; we must emit 10 since
        // that is what the signed order will fill against at the venue.
        assert_eq!(u.quantity, Quantity::from(10));
        assert!(!u.is_quote_quantity);
    } else {
        panic!("Expected Updated event");
    }
}

#[rstest]
#[tokio::test]
async fn test_submit_market_buy_quote_to_base_at_size_precision_two() {
    // Multi-precision regression for the signed-base-qty derivation.
    // size_precision=0 truncates everything to integers, so an off-by-one
    // rounding bug or a wrong precision argument to `from_decimal_dp` would
    // not be observable. Re-running the multi-level walk at size_precision=2
    // exercises decimal places that the integer-precision test cannot reach.
    //
    // 10 pUSD BUY into asks [(0.50, 10 shares), (0.55, 100 shares)]:
    //   Book walk: 10 @ 0.50 (5 pUSD) + 5/0.55 = 9.0909 @ 0.55 -> 19.0909 shares
    //   Signed:    10 / 0.55 = 18.181818 shares (truncated to 18.1818 by builder)
    // At size_precision=2: book walk = 19.09, signed = 18.18.
    let state = TestServerState::default();
    *state.book_response.lock().await = Some(json!({
        "bids": [{"price": "0.48", "size": "100.00"}],
        "asks": [
            {"price": "0.50", "size": "10.00"},
            {"price": "0.55", "size": "100.00"},
        ]
    }));
    let addr = start_mock_server(state.clone()).await;
    let (mut client, mut rx, cache) = create_test_execution_client(addr);
    client.start().unwrap();

    let instrument_id = InstrumentId::from("TEST-TOKEN-PREC2.POLYMARKET");
    add_instrument_to_cache_with_size_precision(&cache, instrument_id, 2);

    let order = make_market_order("O-MKT-PREC2", instrument_id, OrderSide::Buy, true);
    cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .unwrap();
    let cmd = make_submit_cmd(&order, instrument_id);

    client.submit_order(cmd).unwrap();

    let event = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .unwrap()
        .unwrap();
    assert_order_event(event, "Submitted");

    let event = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .unwrap()
        .unwrap();
    let updated = assert_order_event(event, "Updated");

    if let OrderEventAny::Updated(ref u) = updated {
        // Signed taker_amount = 10/0.55 truncated to (price_prec + lot_scale)=4
        // decimals = 18.1818, then expressed at size_precision=2 -> 18.18.
        assert_eq!(u.quantity, Quantity::from("18.18"));
        assert!(!u.is_quote_quantity);
    } else {
        panic!("Expected Updated event");
    }
}

#[rstest]
#[tokio::test]
async fn test_submit_market_order_sell_no_updated_event() {
    let state = TestServerState::default();
    let addr = start_mock_server(state.clone()).await;
    let (mut client, mut rx, cache) = create_test_execution_client(addr);
    client.start().unwrap();

    let instrument_id = InstrumentId::from("TEST-TOKEN.POLYMARKET");
    add_instrument_to_cache(&cache, instrument_id);

    // SELL 10 shares with quote_quantity=false (no conversion needed)
    let order = make_market_order("O-MKT-SELL", instrument_id, OrderSide::Sell, false);
    cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .unwrap();
    let cmd = make_submit_cmd(&order, instrument_id);

    client.submit_order(cmd).unwrap();

    // Submitted
    let event = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .unwrap()
        .unwrap();
    assert_order_event(event, "Submitted");

    // Accepted (no Updated event for SELL orders)
    let event = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .unwrap()
        .unwrap();
    assert_order_event(event, "Accepted");
}

#[rstest]
#[tokio::test]
async fn test_submit_market_order_http_5xx_submit_outcome_unknown() {
    let state = TestServerState::default();
    *state.order_response_status.lock().await = StatusCode::INTERNAL_SERVER_ERROR;
    *state.order_response.lock().await = Some(load_json("http_order_response_error_500.json"));
    let addr = start_mock_server(state.clone()).await;
    let (mut client, mut rx, cache) = create_test_execution_client(addr);
    client.start().unwrap();

    let instrument_id = InstrumentId::from("TEST-TOKEN.POLYMARKET");
    add_instrument_to_cache(&cache, instrument_id);

    let order = make_market_order("O-MKT-UNKNOWN", instrument_id, OrderSide::Buy, true);
    cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .unwrap();
    let cmd = make_submit_cmd(&order, instrument_id);

    client.submit_order(cmd).unwrap();

    assert_order_event(recv_execution_event(&mut rx).await, "Submitted");
    assert_order_event(recv_execution_event(&mut rx).await, "Updated");

    wait_until_async(
        || {
            let state = state.clone();
            async move { *state.order_post_count.lock().await == 1 }
        },
        Duration::from_secs(5),
    )
    .await;
    assert_no_execution_event(&mut rx).await;
}

#[rstest]
#[tokio::test]
async fn test_submit_market_order_ambiguous_retry_then_bad_request_remains_unknown() {
    let state = TestServerState::default();
    *state.order_post_500_remaining.lock().await = 1;
    *state.order_response_status.lock().await = StatusCode::BAD_REQUEST;
    *state.order_response.lock().await = Some(json!({"error": "order already exists"}));
    let addr = start_mock_server(state.clone()).await;
    let (mut client, mut rx, cache) = create_test_execution_client_with_retries(addr, 1);
    client.start().unwrap();

    let instrument_id = InstrumentId::from("TEST-TOKEN.POLYMARKET");
    add_instrument_to_cache(&cache, instrument_id);
    let order = make_market_order(
        "O-MKT-RETRY-AMBIGUOUS-THEN-DUPLICATE",
        instrument_id,
        OrderSide::Buy,
        true,
    );
    cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .unwrap();

    client
        .submit_order(make_submit_cmd(&order, instrument_id))
        .unwrap();
    assert_order_event(recv_execution_event(&mut rx).await, "Submitted");
    assert_order_event(recv_execution_event(&mut rx).await, "Updated");

    wait_until_async(
        || {
            let state = state.clone();
            async move { *state.order_post_count.lock().await == 2 }
        },
        Duration::from_secs(5),
    )
    .await;
    assert_no_execution_event(&mut rx).await;
}

#[rstest]
#[tokio::test]
async fn test_submit_market_order_rejected_empty_book() {
    let state = TestServerState::default();
    // Override book response with empty asks
    *state.book_response.lock().await = Some(json!({"bids": [], "asks": []}));
    let addr = start_mock_server(state).await;
    let (mut client, mut rx, cache) = create_test_execution_client(addr);
    client.start().unwrap();

    let instrument_id = InstrumentId::from("TEST-TOKEN.POLYMARKET");
    add_instrument_to_cache(&cache, instrument_id);

    let order = make_market_order("O-MKT-EMPTY", instrument_id, OrderSide::Buy, true);
    cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .unwrap();
    let cmd = make_submit_cmd(&order, instrument_id);

    client.submit_order(cmd).unwrap();

    // Empty book should cause rejection
    let event = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .unwrap()
        .unwrap();
    assert_order_event(event, "Rejected");
}

// The CLOB rejects a killed FOK with HTTP 400 and a structured body, so the strategy must receive
// the venue's own text without the JSON envelope or the `orderID` that body also carries.
#[rstest]
#[tokio::test]
async fn test_submit_market_order_rejected_reason_carries_venue_error_text() {
    let state = TestServerState::default();
    *state.order_response_status.lock().await = StatusCode::BAD_REQUEST;
    *state.order_response.lock().await = Some(json!({
        "error": "order couldn't be fully filled. FOK orders are fully filled or killed.",
        "orderID": "0x3776d59db9ea1e4bbedf33f6f79ca677cfa6c93c2a44801f5a10516d822cc502",
    }));
    let addr = start_mock_server(state).await;
    let (mut client, mut rx, cache) = create_test_execution_client(addr);
    client.start().unwrap();

    let instrument_id = InstrumentId::from("TEST-TOKEN.POLYMARKET");
    add_instrument_to_cache(&cache, instrument_id);

    let order = make_market_order_with_time_in_force(
        "O-FOK-KILLED",
        instrument_id,
        OrderSide::Buy,
        true,
        TimeInForce::Fok,
    );
    cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .unwrap();

    client
        .submit_order(make_submit_cmd(&order, instrument_id))
        .unwrap();

    let rejected = assert_order_event(recv_execution_event(&mut rx).await, "Rejected");

    assert_eq!(
        order_event_reason(&rejected),
        "bad request: HTTP 400: order couldn't be fully filled. FOK orders are fully filled or killed."
    );
}

fn assert_order_status_report(event: ExecutionEvent, expected_status: OrderStatus) {
    match event {
        ExecutionEvent::Report(report) => match report {
            ExecutionReport::Order(r) => {
                assert_eq!(
                    r.order_status, expected_status,
                    "Expected {expected_status:?}, was {:?}",
                    r.order_status
                );
            }
            other => panic!("Expected Order report, was {other:?}"),
        },
        other => panic!("Expected Report event, was {other:?}"),
    }
}

fn assert_recovery_fill_report(
    event: ExecutionEvent,
    venue_order_id: &str,
    last_qty: &str,
    last_px: &str,
) {
    match event {
        ExecutionEvent::Report(ExecutionReport::Fill(report)) => {
            assert_eq!(report.venue_order_id, VenueOrderId::from(venue_order_id));
            assert_eq!(report.trade_id, TradeId::from("trade-recovery"));
            assert_eq!(report.last_qty, Quantity::from(last_qty));
            assert_eq!(report.last_px, Price::from(last_px));
            assert!(report.commission.is_zero());
        }
        other => panic!("Expected Fill report, was {other:?}"),
    }
}

#[rstest]
#[case("UNMATCHED", "Rejected")]
#[case("CANCELED", "Canceled")]
#[case("CANCELED_MARKET_RESOLVED", "Expired")]
#[tokio::test]
async fn test_fok_deferred_check_emits_terminal_event(
    #[case] venue_status: &str,
    #[case] expected_event: &str,
) {
    let state = TestServerState::default();
    // REST resolves the unfilled FOK order to a terminal status for the deferred check.
    *state.single_order_response.lock().await = Some(json!({
        "associate_trades": [],
        "id": "test-fok-order-id",
        "status": venue_status,
        "market": "0xtest",
        "original_size": "10.0000",
        "outcome": "Yes",
        "maker_address": "0xtest",
        "owner": "test-owner",
        "price": "0.5100",
        "side": "BUY",
        "size_matched": "0.0000",
        "asset_id": "TEST-TOKEN",
        "expiration": null,
        "order_type": "FOK",
        "created_at": 1_703_875_200_000_i64
    }));
    let addr = start_mock_server(state.clone()).await;
    let (mut client, mut rx, cache) = create_test_execution_client(addr);
    client.start().unwrap();

    let instrument_id = InstrumentId::from("TEST-TOKEN.POLYMARKET");
    add_instrument_to_cache(&cache, instrument_id);

    let order = make_market_order_with_time_in_force(
        "O-FOK-UNMATCHED",
        instrument_id,
        OrderSide::Buy,
        true,
        TimeInForce::Fok,
    );
    cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .unwrap();
    let cmd = make_submit_cmd(&order, instrument_id);

    client.submit_order(cmd).unwrap();

    // Submitted
    let event = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .unwrap()
        .unwrap();
    assert_order_event(event, "Submitted");

    // Updated (quote-to-base conversion)
    let event = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .unwrap()
        .unwrap();
    assert_order_event(event, "Updated");

    // Accepted
    let event = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .unwrap()
        .unwrap();
    assert_order_event(event, "Accepted");

    // Deferred FOK check: after ~5s, the own order resolves via REST to a terminal state and
    // emits the matching order event (the order was submitted through this client, so it is
    // tracked).
    let event = tokio::time::timeout(Duration::from_secs(10), rx.recv())
        .await
        .unwrap()
        .unwrap();
    assert_order_event(event, expected_event);
}

// A MATCHED FOK report excludes provisional quantity until the trade confirms
#[rstest]
#[tokio::test]
async fn test_fok_deferred_check_filled_emits_report_for_reconciliation() {
    let state = TestServerState::default();
    *state.single_order_response.lock().await = Some(json!({
        "associate_trades": [],
        "id": "test-fok-order-id",
        "status": "MATCHED",
        "market": "0xtest",
        "original_size": "10.0000",
        "outcome": "Yes",
        "maker_address": "0xtest",
        "owner": "test-owner",
        "price": "0.5100",
        "side": "BUY",
        "size_matched": "10.0000",
        "asset_id": "TEST-TOKEN",
        "expiration": null,
        "order_type": "FOK",
        "created_at": 1_703_875_200_000_i64
    }));
    let addr = start_mock_server(state.clone()).await;
    let (mut client, mut rx, cache) = create_test_execution_client(addr);
    client.start().unwrap();

    let instrument_id = InstrumentId::from("TEST-TOKEN.POLYMARKET");
    add_instrument_to_cache(&cache, instrument_id);

    let order = make_market_order_with_time_in_force(
        "O-FOK-MATCHED",
        instrument_id,
        OrderSide::Buy,
        true,
        TimeInForce::Fok,
    );
    cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .unwrap();
    let cmd = make_submit_cmd(&order, instrument_id);

    client.submit_order(cmd).unwrap();

    for expected in ["Submitted", "Updated", "Accepted"] {
        let event = tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert_order_event(event, expected);
    }

    // Venue Filled with no confirmed local fills surfaces no fill quantity
    let event = tokio::time::timeout(Duration::from_secs(10), rx.recv())
        .await
        .unwrap()
        .unwrap();

    match event {
        ExecutionEvent::Report(ExecutionReport::Order(report)) => {
            assert_eq!(report.order_status, OrderStatus::Filled);
            assert_eq!(report.filled_qty, Quantity::zero(0));
        }
        other => panic!("Expected Order report, was {other:?}"),
    }
}

fn make_stop_market_order(
    client_order_id: &str,
    instrument_id: InstrumentId,
    side: OrderSide,
) -> OrderAny {
    OrderTestBuilder::new(OrderType::StopMarket)
        .trader_id(TraderId::from("TESTER-001"))
        .strategy_id(StrategyId::from("S-001"))
        .instrument_id(instrument_id)
        .client_order_id(ClientOrderId::from(client_order_id))
        .side(side)
        .quantity(Quantity::new(10.0, 0))
        .trigger_price(Price::new(0.50, 4))
        .trigger_type(TriggerType::LastPrice)
        .build()
}

fn make_closed_limit_order(
    client_order_id: &str,
    instrument_id: InstrumentId,
    side: OrderSide,
) -> OrderAny {
    let account_id = AccountId::from("POLYMARKET-001");
    let venue_order_id = VenueOrderId::from("V-CLOSED-1");
    let mut order = make_limit_order(
        client_order_id,
        instrument_id,
        side,
        false,
        false,
        false,
        TimeInForce::Gtc,
    );
    let submitted = TestOrderEventStubs::submitted(&order, account_id);
    order.apply(submitted).unwrap();
    let accepted = TestOrderEventStubs::accepted(&order, account_id, venue_order_id);
    order.apply(accepted).unwrap();
    let canceled = TestOrderEventStubs::canceled(&order, account_id, Some(venue_order_id));
    order.apply(canceled).unwrap();
    assert!(order.is_closed(), "helper must produce a closed order");
    order
}

fn make_limit_order(
    client_order_id: &str,
    instrument_id: InstrumentId,
    side: OrderSide,
    reduce_only: bool,
    quote_quantity: bool,
    post_only: bool,
    time_in_force: TimeInForce,
) -> OrderAny {
    make_limit_order_at_price(
        client_order_id,
        instrument_id,
        side,
        reduce_only,
        quote_quantity,
        post_only,
        time_in_force,
        Price::new(0.50, 4),
    )
}

#[expect(clippy::too_many_arguments)]
fn make_limit_order_at_price(
    client_order_id: &str,
    instrument_id: InstrumentId,
    side: OrderSide,
    reduce_only: bool,
    quote_quantity: bool,
    post_only: bool,
    time_in_force: TimeInForce,
    price: Price,
) -> OrderAny {
    make_limit_order_at_price_and_quantity(
        client_order_id,
        instrument_id,
        side,
        reduce_only,
        quote_quantity,
        post_only,
        time_in_force,
        price,
        Quantity::new(10.0, 0),
    )
}

#[expect(clippy::too_many_arguments)]
fn make_limit_order_at_price_and_quantity(
    client_order_id: &str,
    instrument_id: InstrumentId,
    side: OrderSide,
    reduce_only: bool,
    quote_quantity: bool,
    post_only: bool,
    time_in_force: TimeInForce,
    price: Price,
    quantity: Quantity,
) -> OrderAny {
    let expire_time = if time_in_force == TimeInForce::Gtd {
        Some(UnixNanos::from(2_000_000_000_000_000_000u64))
    } else {
        None
    };

    let mut builder = OrderTestBuilder::new(OrderType::Limit);
    builder
        .trader_id(TraderId::from("TESTER-001"))
        .strategy_id(StrategyId::from("S-001"))
        .instrument_id(instrument_id)
        .client_order_id(ClientOrderId::from(client_order_id))
        .side(side)
        .quantity(quantity)
        .price(price)
        .time_in_force(time_in_force)
        .post_only(post_only)
        .reduce_only(reduce_only)
        .quote_quantity(quote_quantity);

    if let Some(expire_time) = expire_time {
        builder.expire_time(expire_time);
    }

    builder.build()
}

fn make_submit_cmd(order: &OrderAny, instrument_id: InstrumentId) -> SubmitOrder {
    SubmitOrder::new(
        TraderId::from("TESTER-001"),
        Some(*POLYMARKET_CLIENT_ID),
        StrategyId::from("S-001"),
        instrument_id,
        order.client_order_id(),
        order.init_event().clone(),
        None, // exec_algorithm_id
        None, // position_id
        None, // params
        UUID4::new(),
        UnixNanos::default(),
        None, // correlation_id
    )
}

fn make_submit_order_list_cmd(instrument_id: InstrumentId, orders: &[OrderAny]) -> SubmitOrderList {
    let strategy_id = StrategyId::from("S-001");
    let order_list = OrderList::new(
        OrderListId::from("OL-001"),
        instrument_id,
        strategy_id,
        orders.iter().map(Order::client_order_id).collect(),
        UnixNanos::default(),
    );
    let order_inits = orders
        .iter()
        .map(|order| order.init_event().clone())
        .collect();

    SubmitOrderList::new(
        TraderId::from("TESTER-001"),
        Some(*POLYMARKET_CLIENT_ID),
        strategy_id,
        order_list,
        order_inits,
        None,
        None,
        None,
        UUID4::new(),
        UnixNanos::default(),
        None, // correlation_id
    )
}

fn make_cancel_cmd(client_order_id: &str, instrument_id: InstrumentId) -> CancelOrder {
    CancelOrder::new(
        TraderId::from("TESTER-001"),
        Some(*POLYMARKET_CLIENT_ID),
        StrategyId::from("S-001"),
        instrument_id,
        ClientOrderId::from(client_order_id),
        None, // venue_order_id
        UUID4::new(),
        UnixNanos::default(),
        None,
        None, // correlation_id
    )
}

fn add_instrument_to_cache(cache: &Rc<RefCell<Cache>>, instrument_id: InstrumentId) {
    add_instrument_to_cache_with_size_precision(cache, instrument_id, 0);
}

fn add_instrument_to_cache_with_size_precision(
    cache: &Rc<RefCell<Cache>>,
    instrument_id: InstrumentId,
    size_precision: u8,
) {
    add_instrument_to_cache_with_tick(cache, instrument_id, "0.0001", size_precision);
}

fn add_instrument_to_cache_with_tick(
    cache: &Rc<RefCell<Cache>>,
    instrument_id: InstrumentId,
    tick_size: &str,
    size_precision: u8,
) {
    let symbol = "71321045679252212594626385532706912750332728571942532289631379312455583992563";
    let price_increment = Price::from(tick_size);
    let size_increment = if size_precision == 0 {
        Quantity::from("1")
    } else {
        Quantity::from(format!(
            "0.{}1",
            "0".repeat((size_precision as usize).saturating_sub(1))
        ))
    };
    let raw_symbol = Symbol::from(symbol);

    let instrument = BinaryOption::new(
        instrument_id,
        raw_symbol,
        AssetClass::Alternative,
        Currency::pUSD(),
        UnixNanos::default(), // activation_ns
        UnixNanos::default(), // expiration_ns
        price_increment.precision,
        size_precision,
        price_increment,
        size_increment,
        None, // outcome
        None, // description
        None, // max_quantity
        None, // min_quantity
        None, // max_notional
        None, // min_notional
        None, // max_price
        None, // min_price
        None, // margin_init
        None, // margin_maint
        None, // maker_fee
        None, // taker_fee
        None, // tick_scheme
        None, // info
        UnixNanos::default(),
        UnixNanos::default(),
    );
    cache
        .borrow_mut()
        .add_instrument(InstrumentAny::BinaryOption(instrument))
        .unwrap();
}

fn submit_and_accept_order(cache: &Rc<RefCell<Cache>>, order: &mut OrderAny, venue_order_id: &str) {
    let account_id = AccountId::from("POLYMARKET-001");
    let vid = VenueOrderId::from(venue_order_id);
    let submitted = TestOrderEventStubs::submitted(order, account_id);
    *order = cache.borrow_mut().update_order(&submitted).unwrap();
    let accepted = TestOrderEventStubs::accepted(order, account_id, vid);
    *order = cache.borrow_mut().update_order(&accepted).unwrap();
}

fn assert_order_event(event: ExecutionEvent, expected: &str) -> OrderEventAny {
    match event {
        ExecutionEvent::Order(order_event) => {
            let variant = format!("{order_event:?}");
            assert!(
                variant.starts_with(expected),
                "Expected {expected}, was {variant}"
            );
            order_event
        }
        other => panic!("Expected Order event, was {other:?}"),
    }
}

fn order_event_reason(event: &OrderEventAny) -> String {
    match event {
        OrderEventAny::Rejected(e) => e.reason.to_string(),
        OrderEventAny::Denied(e) => e.reason.to_string(),
        OrderEventAny::ModifyRejected(e) => e.reason.to_string(),
        OrderEventAny::CancelRejected(e) => e.reason.to_string(),
        other => panic!("Expected rejection/denial event with a reason, was {other:?}"),
    }
}

async fn recv_execution_event(
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
) -> ExecutionEvent {
    tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .unwrap()
        .unwrap()
}

async fn assert_no_execution_event(rx: &mut tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>) {
    match tokio::time::timeout(Duration::from_millis(100), rx.recv()).await {
        Err(_) => {}
        Ok(Some(event)) => panic!("Expected no execution event, was {event:?}"),
        Ok(None) => panic!("Execution event channel closed"),
    }
}

#[rstest]
#[tokio::test]
async fn test_submit_order_denied_for_reduce_only() {
    let state = TestServerState::default();
    let addr = start_mock_server(state).await;
    let (mut client, mut rx, cache) = create_test_execution_client(addr);
    client.start().unwrap();

    let instrument_id = InstrumentId::from("TEST-TOKEN.POLYMARKET");
    let order = make_limit_order(
        "O-REDUCE",
        instrument_id,
        OrderSide::Buy,
        true,  // reduce_only
        false, // quote_quantity
        false, // post_only
        TimeInForce::Gtc,
    );
    cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .unwrap();
    let cmd = make_submit_cmd(&order, instrument_id);

    client.submit_order(cmd).unwrap();

    let event = rx.try_recv().unwrap();
    assert_order_event(event, "Denied");
}

#[rstest]
#[tokio::test]
async fn test_submit_order_denied_for_quote_quantity() {
    let state = TestServerState::default();
    let addr = start_mock_server(state).await;
    let (mut client, mut rx, cache) = create_test_execution_client(addr);
    client.start().unwrap();

    let instrument_id = InstrumentId::from("TEST-TOKEN.POLYMARKET");
    let order = make_limit_order(
        "O-QUOTE",
        instrument_id,
        OrderSide::Buy,
        false, // reduce_only
        true,  // quote_quantity
        false, // post_only
        TimeInForce::Gtc,
    );
    cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .unwrap();
    let cmd = make_submit_cmd(&order, instrument_id);

    client.submit_order(cmd).unwrap();

    let event = rx.try_recv().unwrap();
    assert_order_event(event, "Denied");
}

#[rstest]
#[tokio::test]
async fn test_submit_order_denied_for_post_only_with_ioc() {
    let state = TestServerState::default();
    let addr = start_mock_server(state).await;
    let (mut client, mut rx, cache) = create_test_execution_client(addr);
    client.start().unwrap();

    let instrument_id = InstrumentId::from("TEST-TOKEN.POLYMARKET");
    let order = make_limit_order(
        "O-POST-IOC",
        instrument_id,
        OrderSide::Buy,
        false, // reduce_only
        false, // quote_quantity
        true,  // post_only
        TimeInForce::Ioc,
    );
    cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .unwrap();
    let cmd = make_submit_cmd(&order, instrument_id);

    client.submit_order(cmd).unwrap();

    let event = rx.try_recv().unwrap();
    assert_order_event(event, "Denied");
}

#[rstest]
#[case("-0.01")]
#[case("1.01")]
#[tokio::test]
async fn test_submit_order_denied_for_price_out_of_range(#[case] price: &str) {
    let state = TestServerState::default();
    let addr = start_mock_server(state.clone()).await;
    let (mut client, mut rx, cache) = create_test_execution_client(addr);
    client.start().unwrap();

    let instrument_id = InstrumentId::from("TEST-TOKEN.POLYMARKET");
    add_instrument_to_cache(&cache, instrument_id);
    let order = make_limit_order_at_price(
        "O-PRICE-RANGE",
        instrument_id,
        OrderSide::Buy,
        false,
        false,
        false,
        TimeInForce::Gtc,
        Price::from(price),
    );
    cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .unwrap();

    client
        .submit_order(make_submit_cmd(&order, instrument_id))
        .unwrap();

    let denied = assert_order_event(rx.try_recv().unwrap(), "Denied");
    assert_eq!(
        order_event_reason(&denied),
        format!("Limit order price {price} outside Polymarket range [0.0001, 0.9999]")
    );
    assert_eq!(*state.order_post_count.lock().await, 0);
}

#[rstest]
#[case("0.005", "0.501")]
#[case("0.0025", "0.501")]
#[tokio::test]
async fn test_submit_order_denied_for_price_misaligned(
    #[case] tick_size: &str,
    #[case] price: &str,
) {
    let state = TestServerState::default();
    let addr = start_mock_server(state.clone()).await;
    let (mut client, mut rx, cache) = create_test_execution_client(addr);
    client.start().unwrap();

    let instrument_id = InstrumentId::from("TEST-TOKEN.POLYMARKET");
    add_instrument_to_cache_with_tick(&cache, instrument_id, tick_size, 2);
    let order = make_limit_order_at_price(
        "O-PRICE-MISALIGNED",
        instrument_id,
        OrderSide::Buy,
        false,
        false,
        false,
        TimeInForce::Gtc,
        Price::from(price),
    );
    cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .unwrap();

    client
        .submit_order(make_submit_cmd(&order, instrument_id))
        .unwrap();

    let denied = assert_order_event(rx.try_recv().unwrap(), "Denied");
    assert_eq!(
        order_event_reason(&denied),
        format!("Limit order price {price} does not conform to Polymarket tick size {tick_size}")
    );
    assert_eq!(*state.order_post_count.lock().await, 0);
}

#[rstest]
#[case("0.0001")]
#[case("0.9999")]
#[tokio::test]
async fn test_submit_order_accepts_price_at_tick_relative_bound(#[case] price: &str) {
    let state = TestServerState::default();
    let addr = start_mock_server(state.clone()).await;
    let (mut client, mut rx, cache) = create_test_execution_client(addr);
    client.start().unwrap();

    let instrument_id = InstrumentId::from("TEST-TOKEN.POLYMARKET");
    add_instrument_to_cache(&cache, instrument_id);
    let order = make_limit_order_at_price(
        "O-PRICE-BOUND",
        instrument_id,
        OrderSide::Buy,
        false,
        false,
        false,
        TimeInForce::Gtc,
        Price::from(price),
    );
    cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .unwrap();

    client
        .submit_order(make_submit_cmd(&order, instrument_id))
        .unwrap();

    // A price at the tick-relative bound (tick=0.0001 -> range [0.0001, 0.9999], the value a
    // consumer clamps to) is not locally denied; the single-submit path emits Submitted.
    assert_order_event(rx.try_recv().unwrap(), "Submitted");
}

#[rstest]
#[case::ioc(TimeInForce::Ioc, "FAK")]
#[case::fok(TimeInForce::Fok, "FOK")]
#[tokio::test]
async fn test_submit_immediate_limit_buy_denies_fractional_cent_maker_amount(
    #[case] time_in_force: TimeInForce,
    #[case] order_type: &str,
) {
    let state = TestServerState::default();
    let addr = start_mock_server(state.clone()).await;
    let (mut client, mut rx, cache) = create_test_execution_client(addr);
    client.start().unwrap();

    let instrument_id = InstrumentId::from("TEST-TOKEN.POLYMARKET");
    add_instrument_to_cache_with_tick(&cache, instrument_id, "0.001", 2);
    let order = make_limit_order_at_price_and_quantity(
        "O-IOC-FRACTIONAL-CENT",
        instrument_id,
        OrderSide::Buy,
        false,
        false,
        false,
        time_in_force,
        Price::from("0.961"),
        Quantity::from("5.00"),
    );
    cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .unwrap();

    client
        .submit_order(make_submit_cmd(&order, instrument_id))
        .unwrap();

    let denied = assert_order_event(rx.try_recv().unwrap(), "Denied");
    assert_eq!(
        order_event_reason(&denied),
        format!(
            "Polymarket {order_type} BUY maker amount 4.805 pUSD exceeds 2 decimal places for price 0.961 and quantity 5"
        ),
    );
    assert_eq!(*state.order_post_count.lock().await, 0);
}

#[rstest]
#[case::ioc(TimeInForce::Ioc, "FAK")]
#[case::fok(TimeInForce::Fok, "FOK")]
#[tokio::test]
async fn test_submit_immediate_limit_sell_preserves_fractional_cent_taker_amount(
    #[case] time_in_force: TimeInForce,
    #[case] order_type: &str,
) {
    let state = TestServerState::default();
    let addr = start_mock_server(state.clone()).await;
    let (mut client, _rx, cache) = create_test_execution_client(addr);
    client.start().unwrap();

    let instrument_id = InstrumentId::from("TEST-TOKEN.POLYMARKET");
    add_instrument_to_cache_with_tick(&cache, instrument_id, "0.001", 2);
    let order = make_limit_order_at_price_and_quantity(
        "O-SELL-FRACTIONAL-CENT",
        instrument_id,
        OrderSide::Sell,
        false,
        false,
        false,
        time_in_force,
        Price::from("0.961"),
        Quantity::from("5.00"),
    );
    cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .unwrap();

    client
        .submit_order(make_submit_cmd(&order, instrument_id))
        .unwrap();
    wait_until_async(
        || {
            let state = state.clone();
            async move { *state.order_post_count.lock().await == 1 }
        },
        Duration::from_secs(5),
    )
    .await;

    let body = state.last_body.lock().await.clone().unwrap();
    let signed_order = body.get("order").unwrap();
    assert_eq!(
        signed_order.get("makerAmount").and_then(Value::as_str),
        Some("5000000"),
    );
    assert_eq!(
        signed_order.get("takerAmount").and_then(Value::as_str),
        Some("4805000"),
    );
    assert_eq!(
        body.get("orderType").and_then(Value::as_str),
        Some(order_type)
    );
}

#[rstest]
#[case::tick_tenth("0.1", "0.5", "10", "5000000", "10000000")]
#[case::tick_hundredth("0.01", "0.56", "10", "5600000", "10000000")]
#[case::tick_half_cent("0.005", "0.505", "10", "5050000", "10000000")]
#[case::tick_quarter_cent("0.0025", "0.5025", "20", "10050000", "20000000")]
#[case::tick_thousandth("0.001", "0.961", "10", "9610000", "10000000")]
#[case::tick_ten_thousandth("0.0001", "0.9612", "25", "24030000", "25000000")]
#[tokio::test]
async fn test_submit_limit_order_serializes_amount_matrix(
    #[case] tick_size: &str,
    #[case] price: &str,
    #[case] quantity: &str,
    #[case] notional_amount: &str,
    #[case] quantity_amount: &str,
    #[values(OrderSide::Buy, OrderSide::Sell)] side: OrderSide,
    #[values(
        TimeInForce::Gtc,
        TimeInForce::Gtd,
        TimeInForce::Ioc,
        TimeInForce::Fok
    )]
    time_in_force: TimeInForce,
) {
    let state = TestServerState::default();
    let addr = start_mock_server(state.clone()).await;
    let (mut client, _rx, cache) = create_test_execution_client(addr);
    client.start().unwrap();

    let instrument_id = InstrumentId::from("TEST-TOKEN.POLYMARKET");
    add_instrument_to_cache_with_tick(&cache, instrument_id, tick_size, 2);
    let order = make_limit_order_at_price_and_quantity(
        "O-AMOUNT-MATRIX",
        instrument_id,
        side,
        false,
        false,
        false,
        time_in_force,
        Price::from(price),
        Quantity::from(quantity),
    );
    cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .unwrap();

    client
        .submit_order(make_submit_cmd(&order, instrument_id))
        .unwrap();
    wait_until_async(
        || {
            let state = state.clone();
            async move { *state.order_post_count.lock().await == 1 }
        },
        Duration::from_secs(5),
    )
    .await;

    let body = state.last_body.lock().await.clone().unwrap();
    let signed_order = body.get("order").unwrap();
    let (expected_maker, expected_taker) = match side {
        OrderSide::Buy => (notional_amount, quantity_amount),
        OrderSide::Sell => (quantity_amount, notional_amount),
        _ => unreachable!(),
    };
    assert_eq!(
        signed_order.get("makerAmount").and_then(Value::as_str),
        Some(expected_maker),
    );
    assert_eq!(
        signed_order.get("takerAmount").and_then(Value::as_str),
        Some(expected_taker),
    );
    assert_eq!(
        body.get("orderType").and_then(Value::as_str),
        Some(polymarket_order_type(time_in_force)),
    );
}

fn polymarket_order_type(time_in_force: TimeInForce) -> &'static str {
    match time_in_force {
        TimeInForce::Gtc => "GTC",
        TimeInForce::Gtd => "GTD",
        TimeInForce::Ioc => "FAK",
        TimeInForce::Fok => "FOK",
        _ => unreachable!(),
    }
}

#[rstest]
#[case("0.0001")]
#[case("0.9999")]
#[tokio::test]
async fn test_submit_order_allows_tick_relative_price_boundary(#[case] price: &str) {
    let state = TestServerState::default();
    let addr = start_mock_server(state).await;
    let (mut client, mut rx, cache) = create_test_execution_client(addr);
    client.start().unwrap();

    let instrument_id = InstrumentId::from("TEST-TOKEN.POLYMARKET");
    add_instrument_to_cache(&cache, instrument_id);
    let order = make_limit_order_at_price(
        "O-PRICE-BOUNDARY",
        instrument_id,
        OrderSide::Buy,
        false,
        false,
        false,
        TimeInForce::Gtc,
        Price::from(price),
    );
    cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .unwrap();

    client
        .submit_order(make_submit_cmd(&order, instrument_id))
        .unwrap();

    assert_order_event(rx.try_recv().unwrap(), "Submitted");
}

#[rstest]
#[tokio::test]
async fn test_submit_order_denied_for_missing_cached_instrument() {
    let state = TestServerState::default();
    let addr = start_mock_server(state).await;
    let (mut client, mut rx, cache) = create_test_execution_client(addr);
    client.start().unwrap();

    // Order references an instrument that is never loaded into the cache.
    let instrument_id = InstrumentId::from("MISSING-TOKEN.POLYMARKET");
    let order = make_limit_order(
        "O-MISSING",
        instrument_id,
        OrderSide::Buy,
        false, // reduce_only
        false, // quote_quantity
        false, // post_only
        TimeInForce::Gtc,
    );
    cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .unwrap();
    let cmd = make_submit_cmd(&order, instrument_id);

    client.submit_order(cmd).unwrap();

    let event = rx.try_recv().unwrap();
    let denied = assert_order_event(event, "Denied");
    let reason = order_event_reason(&denied);

    assert_eq!(
        reason,
        InstrumentLookupError::not_found(instrument_id).to_string()
    );
    assert_eq!(reason, format!("{INSTRUMENT_NOT_FOUND}: {instrument_id}"));
}

#[rstest]
#[tokio::test]
async fn test_submit_order_post_only_with_gtc_allowed() {
    let state = TestServerState::default();
    let addr = start_mock_server(state).await;
    let (mut client, mut rx, cache) = create_test_execution_client(addr);
    client.start().unwrap();

    let instrument_id = InstrumentId::from("TEST-TOKEN.POLYMARKET");
    add_instrument_to_cache(&cache, instrument_id);

    let order = make_limit_order(
        "O-POST-GTC",
        instrument_id,
        OrderSide::Buy,
        false, // reduce_only
        false, // quote_quantity
        true,  // post_only
        TimeInForce::Gtc,
    );
    cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .unwrap();
    let cmd = make_submit_cmd(&order, instrument_id);

    client.submit_order(cmd).unwrap();

    // First event should be Submitted (not Denied)
    let event = rx.try_recv().unwrap();
    assert_order_event(event, "Submitted");
}

#[rstest]
#[tokio::test]
async fn test_submit_order_accepted_on_http_success() {
    let state = TestServerState::default();
    let addr = start_mock_server(state).await;
    let (mut client, mut rx, cache) = create_test_execution_client(addr);
    client.start().unwrap();

    let instrument_id = InstrumentId::from("TEST-TOKEN.POLYMARKET");
    add_instrument_to_cache(&cache, instrument_id);

    let order = make_limit_order(
        "O-ACCEPT",
        instrument_id,
        OrderSide::Buy,
        false,
        false,
        false,
        TimeInForce::Gtc,
    );
    cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .unwrap();
    let cmd = make_submit_cmd(&order, instrument_id);

    client.submit_order(cmd).unwrap();

    // Submitted event
    let event = rx.try_recv().unwrap();
    assert_order_event(event, "Submitted");

    // Accepted event (async, need to wait)
    let event = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .unwrap()
        .unwrap();
    assert_order_event(event, "Accepted");
}

#[rstest]
#[tokio::test]
async fn test_submit_order_rejected_on_http_failure_response() {
    let state = TestServerState::default();
    *state.order_response.lock().await = Some(load_json("http_order_response_failed.json"));
    let addr = start_mock_server(state).await;
    let (mut client, mut rx, cache) = create_test_execution_client(addr);
    client.start().unwrap();

    let instrument_id = InstrumentId::from("TEST-TOKEN.POLYMARKET");
    add_instrument_to_cache(&cache, instrument_id);

    let order = make_limit_order(
        "O-REJECT-RESP",
        instrument_id,
        OrderSide::Buy,
        false,
        false,
        false,
        TimeInForce::Gtc,
    );
    cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .unwrap();
    let cmd = make_submit_cmd(&order, instrument_id);

    client.submit_order(cmd).unwrap();

    // Submitted
    let event = rx.try_recv().unwrap();
    assert_order_event(event, "Submitted");

    // Rejected (async)
    let event = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .unwrap()
        .unwrap();
    assert_order_event(event, "Rejected");
}

#[rstest]
#[tokio::test]
async fn test_submit_order_http_5xx_submit_outcome_unknown() {
    let state = TestServerState::default();
    *state.order_response_status.lock().await = StatusCode::INTERNAL_SERVER_ERROR;
    *state.order_response.lock().await = Some(load_json("http_order_response_error_500.json"));
    let addr = start_mock_server(state.clone()).await;
    let (mut client, mut rx, cache) = create_test_execution_client(addr);
    client.start().unwrap();

    let instrument_id = InstrumentId::from("TEST-TOKEN.POLYMARKET");
    add_instrument_to_cache(&cache, instrument_id);

    let order = make_limit_order(
        "O-REJECT-500",
        instrument_id,
        OrderSide::Buy,
        false,
        false,
        false,
        TimeInForce::Gtc,
    );
    cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .unwrap();
    let cmd = make_submit_cmd(&order, instrument_id);

    client.submit_order(cmd).unwrap();

    // Submitted
    let event = rx.try_recv().unwrap();
    assert_order_event(event, "Submitted");

    wait_until_async(
        || {
            let state = state.clone();
            async move { *state.order_post_count.lock().await == 1 }
        },
        Duration::from_secs(5),
    )
    .await;
    assert_no_execution_event(&mut rx).await;
}

#[rstest]
#[tokio::test]
async fn test_submit_order_retries_5xx_and_accepts_when_recovered() {
    // Server returns 500 twice, then 200 on the third attempt. With
    // max_retries=2 the submitter should consume both retries and accept
    // on the third call.
    let state = TestServerState::default();
    *state.order_post_500_remaining.lock().await = 2;
    let addr = start_mock_server(state.clone()).await;
    let (mut client, mut rx, cache) = create_test_execution_client_with_retries(addr, 2);
    client.start().unwrap();

    let instrument_id = InstrumentId::from("TEST-TOKEN.POLYMARKET");
    add_instrument_to_cache(&cache, instrument_id);

    let order = make_limit_order(
        "O-RETRY-RECOVER",
        instrument_id,
        OrderSide::Buy,
        false,
        false,
        false,
        TimeInForce::Gtc,
    );
    cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .unwrap();
    let cmd = make_submit_cmd(&order, instrument_id);

    client.submit_order(cmd).unwrap();

    // Submitted (synchronous before the HTTP roundtrip).
    let event = rx.try_recv().unwrap();
    assert_order_event(event, "Submitted");

    // Accepted after the retries succeed.
    let event = tokio::time::timeout(Duration::from_secs(10), rx.recv())
        .await
        .expect("expected accept within timeout")
        .unwrap();
    assert_order_event(event, "Accepted");

    // Three POSTs total: two failed retries plus the recovered call.
    assert_eq!(*state.order_post_count.lock().await, 3);
}

#[rstest]
#[tokio::test]
async fn test_submit_order_ambiguous_retry_then_bad_request_remains_unknown() {
    let state = TestServerState::default();
    *state.order_post_500_remaining.lock().await = 1;
    *state.order_response_status.lock().await = StatusCode::BAD_REQUEST;
    *state.order_response.lock().await = Some(json!({"error": "order already exists"}));
    let addr = start_mock_server(state.clone()).await;
    let (mut client, mut rx, cache) = create_test_execution_client_with_retries(addr, 1);
    client.start().unwrap();

    let instrument_id = InstrumentId::from("TEST-TOKEN.POLYMARKET");
    add_instrument_to_cache(&cache, instrument_id);
    let order = make_limit_order(
        "O-RETRY-AMBIGUOUS-THEN-DUPLICATE",
        instrument_id,
        OrderSide::Buy,
        false,
        false,
        false,
        TimeInForce::Gtc,
    );
    cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .unwrap();

    client
        .submit_order(make_submit_cmd(&order, instrument_id))
        .unwrap();
    assert_order_event(rx.try_recv().unwrap(), "Submitted");

    wait_until_async(
        || {
            let state = state.clone();
            async move { *state.order_post_count.lock().await == 2 }
        },
        Duration::from_secs(5),
    )
    .await;
    assert_no_execution_event(&mut rx).await;
}

#[rstest]
#[tokio::test]
async fn test_submit_order_5xx_exhausts_retries_submit_outcome_unknown() {
    // Server returns 500 three times. With max_retries=2 the submitter
    // exhausts retries on the third attempt and leaves the submit outcome unknown.
    let state = TestServerState::default();
    *state.order_post_500_remaining.lock().await = 3;
    let addr = start_mock_server(state.clone()).await;
    let (mut client, mut rx, cache) = create_test_execution_client_with_retries(addr, 2);
    client.start().unwrap();

    let instrument_id = InstrumentId::from("TEST-TOKEN.POLYMARKET");
    add_instrument_to_cache(&cache, instrument_id);

    let order = make_limit_order(
        "O-RETRY-EXHAUST",
        instrument_id,
        OrderSide::Buy,
        false,
        false,
        false,
        TimeInForce::Gtc,
    );
    cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .unwrap();
    let cmd = make_submit_cmd(&order, instrument_id);

    client.submit_order(cmd).unwrap();

    let event = rx.try_recv().unwrap();
    assert_order_event(event, "Submitted");

    wait_until_async(
        || {
            let state = state.clone();
            async move { *state.order_post_count.lock().await == 3 }
        },
        Duration::from_secs(5),
    )
    .await;
    assert_no_execution_event(&mut rx).await;

    // Initial attempt + 2 retries = 3 POSTs, then give up.
    assert_eq!(*state.order_post_count.lock().await, 3);
}

#[rstest]
#[case::tick_tenth("0.1", "0.5", "10", "5000000", "10000000")]
#[case::tick_hundredth("0.01", "0.56", "10", "5600000", "10000000")]
#[case::tick_half_cent("0.005", "0.505", "10", "5050000", "10000000")]
#[case::tick_quarter_cent("0.0025", "0.5025", "20", "10050000", "20000000")]
#[case::tick_thousandth("0.001", "0.961", "10", "9610000", "10000000")]
#[case::tick_ten_thousandth("0.0001", "0.9612", "25", "24030000", "25000000")]
#[tokio::test]
async fn test_submit_order_list_serializes_amount_matrix(
    #[case] tick_size: &str,
    #[case] price: &str,
    #[case] quantity: &str,
    #[case] notional_amount: &str,
    #[case] quantity_amount: &str,
    #[values(
        TimeInForce::Gtc,
        TimeInForce::Gtd,
        TimeInForce::Ioc,
        TimeInForce::Fok
    )]
    time_in_force: TimeInForce,
) {
    let state = TestServerState::default();
    *state.batch_order_response.lock().await = Some(json!([
        {"success": true, "orderID": "0xbatch-order-1", "errorMsg": ""},
        {"success": true, "orderID": "0xbatch-order-2", "errorMsg": ""}
    ]));
    let addr = start_mock_server(state.clone()).await;
    let (mut client, _rx, cache) = create_test_execution_client(addr);
    client.start().unwrap();

    let instrument_id = InstrumentId::from("TEST-TOKEN.POLYMARKET");
    add_instrument_to_cache_with_tick(&cache, instrument_id, tick_size, 2);
    let buy = make_limit_order_at_price_and_quantity(
        "O-LIST-AMOUNT-BUY",
        instrument_id,
        OrderSide::Buy,
        false,
        false,
        false,
        time_in_force,
        Price::from(price),
        Quantity::from(quantity),
    );
    let sell = make_limit_order_at_price_and_quantity(
        "O-LIST-AMOUNT-SELL",
        instrument_id,
        OrderSide::Sell,
        false,
        false,
        false,
        time_in_force,
        Price::from(price),
        Quantity::from(quantity),
    );

    for order in [&buy, &sell] {
        cache
            .borrow_mut()
            .add_order(order.clone(), None, None, false)
            .unwrap();
    }

    client
        .submit_order_list(make_submit_order_list_cmd(instrument_id, &[buy, sell]))
        .unwrap();
    wait_until_async(
        || {
            let state = state.clone();
            async move { *state.batch_order_post_count.lock().await == 1 }
        },
        Duration::from_secs(5),
    )
    .await;

    let body = state.last_body.lock().await.clone().unwrap();
    let entries = body.as_array().unwrap();
    assert_eq!(entries.len(), 2);
    for (entry, side) in entries.iter().zip([OrderSide::Buy, OrderSide::Sell]) {
        let signed_order = entry.get("order").unwrap();
        let (expected_maker, expected_taker) = match side {
            OrderSide::Buy => (notional_amount, quantity_amount),
            OrderSide::Sell => (quantity_amount, notional_amount),
            _ => unreachable!(),
        };
        assert_eq!(
            signed_order.get("makerAmount").and_then(Value::as_str),
            Some(expected_maker),
        );
        assert_eq!(
            signed_order.get("takerAmount").and_then(Value::as_str),
            Some(expected_taker),
        );
        assert_eq!(
            entry.get("orderType").and_then(Value::as_str),
            Some(polymarket_order_type(time_in_force)),
        );
    }
}

#[rstest]
#[tokio::test]
async fn test_submit_order_list_denies_unrepresentable_immediate_buys_before_post() {
    let state = TestServerState::default();
    let addr = start_mock_server(state.clone()).await;
    let (mut client, mut rx, cache) = create_test_execution_client(addr);
    client.start().unwrap();

    let instrument_id = InstrumentId::from("TEST-TOKEN.POLYMARKET");
    add_instrument_to_cache_with_tick(&cache, instrument_id, "0.001", 2);
    let orders = [
        make_limit_order_at_price_and_quantity(
            "O-LIST-FAK-FRACTIONAL-CENT",
            instrument_id,
            OrderSide::Buy,
            false,
            false,
            false,
            TimeInForce::Ioc,
            Price::from("0.961"),
            Quantity::from("5.00"),
        ),
        make_limit_order_at_price_and_quantity(
            "O-LIST-FOK-FRACTIONAL-CENT",
            instrument_id,
            OrderSide::Buy,
            false,
            false,
            false,
            TimeInForce::Fok,
            Price::from("0.961"),
            Quantity::from("5.00"),
        ),
    ];

    for order in &orders {
        cache
            .borrow_mut()
            .add_order(order.clone(), None, None, false)
            .unwrap();
    }

    client
        .submit_order_list(make_submit_order_list_cmd(instrument_id, &orders))
        .unwrap();

    for order_type in ["FAK", "FOK"] {
        let denied = assert_order_event(rx.try_recv().unwrap(), "Denied");
        assert_eq!(
            order_event_reason(&denied),
            format!(
                "Polymarket {order_type} BUY maker amount 4.805 pUSD exceeds 2 decimal places for price 0.961 and quantity 5"
            ),
        );
    }
    assert_eq!(*state.order_post_count.lock().await, 0);
    assert_eq!(*state.batch_order_post_count.lock().await, 0);
}

#[rstest]
#[tokio::test]
async fn test_submit_order_list_posts_batch_and_accepts_orders() {
    let state = TestServerState::default();
    *state.batch_order_response.lock().await = Some(json!([
        {"success": true, "orderID": "0xbatch-order-1", "errorMsg": ""},
        {"success": true, "orderID": "0xbatch-order-2", "errorMsg": ""}
    ]));
    let addr = start_mock_server(state.clone()).await;
    let (mut client, mut rx, cache) = create_test_execution_client(addr);
    client.start().unwrap();

    let instrument_id = InstrumentId::from("TEST-TOKEN.POLYMARKET");
    add_instrument_to_cache(&cache, instrument_id);

    let order1 = make_limit_order(
        "O-LIST-1",
        instrument_id,
        OrderSide::Buy,
        false,
        false,
        false,
        TimeInForce::Gtc,
    );
    let order2 = make_limit_order(
        "O-LIST-2",
        instrument_id,
        OrderSide::Sell,
        false,
        false,
        false,
        TimeInForce::Gtc,
    );
    cache
        .borrow_mut()
        .add_order(order1.clone(), None, None, false)
        .unwrap();
    cache
        .borrow_mut()
        .add_order(order2.clone(), None, None, false)
        .unwrap();

    let cmd = make_submit_order_list_cmd(instrument_id, &[order1, order2]);
    client.submit_order_list(cmd).unwrap();

    assert_order_event(recv_execution_event(&mut rx).await, "Submitted");
    assert_order_event(recv_execution_event(&mut rx).await, "Submitted");
    assert_order_event(recv_execution_event(&mut rx).await, "Accepted");
    assert_order_event(recv_execution_event(&mut rx).await, "Accepted");

    assert_eq!(*state.batch_order_post_count.lock().await, 1);
    assert_eq!(state.last_path.lock().await.as_str(), "/orders");
    let body = state.last_body.lock().await.clone().unwrap();
    let entries = body.as_array().unwrap();
    assert_eq!(entries.len(), 2);
    for entry in entries {
        let obj = entry.as_object().unwrap();
        assert!(obj.contains_key("order"), "entry missing `order` field");
        assert!(obj.contains_key("owner"), "entry missing `owner` field");
        assert_eq!(
            obj.get("orderType").and_then(Value::as_str),
            Some("GTC"),
            "entry orderType should be GTC"
        );
        let order = obj.get("order").unwrap().as_object().unwrap();
        assert!(order.contains_key("salt"), "signed order missing `salt`");
        assert!(
            order.contains_key("signature"),
            "signed order missing `signature`"
        );
    }
}

#[rstest]
#[case::unsupported_instruction(false)]
#[case::out_of_range_price(true)]
#[tokio::test]
async fn test_submit_order_list_denies_invalid_orders_before_batch_post(
    #[case] out_of_range_price: bool,
) {
    let state = TestServerState::default();
    *state.batch_order_response.lock().await = Some(json!([
        {"success": true, "orderID": "0xbatch-order-1", "errorMsg": ""},
        {"success": true, "orderID": "0xbatch-order-2", "errorMsg": ""}
    ]));
    let addr = start_mock_server(state.clone()).await;
    let (mut client, mut rx, cache) = create_test_execution_client(addr);
    client.start().unwrap();

    let instrument_id = InstrumentId::from("TEST-TOKEN.POLYMARKET");
    add_instrument_to_cache(&cache, instrument_id);

    let valid1 = make_limit_order(
        "O-LIST-VALID-1",
        instrument_id,
        OrderSide::Buy,
        false,
        false,
        false,
        TimeInForce::Gtc,
    );
    let invalid = if out_of_range_price {
        make_limit_order_at_price(
            "O-LIST-INVALID",
            instrument_id,
            OrderSide::Sell,
            false,
            false,
            false,
            TimeInForce::Gtc,
            Price::from("1.01"),
        )
    } else {
        make_limit_order(
            "O-LIST-INVALID",
            instrument_id,
            OrderSide::Sell,
            false,
            false,
            true,
            TimeInForce::Ioc,
        )
    };
    let valid2 = make_limit_order(
        "O-LIST-VALID-2",
        instrument_id,
        OrderSide::Sell,
        false,
        false,
        false,
        TimeInForce::Gtc,
    );
    cache
        .borrow_mut()
        .add_order(valid1.clone(), None, None, false)
        .unwrap();
    cache
        .borrow_mut()
        .add_order(invalid.clone(), None, None, false)
        .unwrap();
    cache
        .borrow_mut()
        .add_order(valid2.clone(), None, None, false)
        .unwrap();

    let cmd = make_submit_order_list_cmd(instrument_id, &[valid1, invalid, valid2]);
    client.submit_order_list(cmd).unwrap();

    assert_order_event(recv_execution_event(&mut rx).await, "Denied");
    assert_order_event(recv_execution_event(&mut rx).await, "Submitted");
    assert_order_event(recv_execution_event(&mut rx).await, "Submitted");
    assert_order_event(recv_execution_event(&mut rx).await, "Accepted");
    assert_order_event(recv_execution_event(&mut rx).await, "Accepted");

    assert_eq!(*state.batch_order_post_count.lock().await, 1);
    assert_eq!(state.last_path.lock().await.as_str(), "/orders");
    let body = state.last_body.lock().await.clone().unwrap();
    assert_eq!(body.as_array().unwrap().len(), 2);
}

#[rstest]
#[tokio::test]
async fn test_submit_order_list_accepts_prices_at_tick_relative_bounds() {
    let state = TestServerState::default();
    *state.batch_order_response.lock().await = Some(json!([
        {"success": true, "orderID": "0xbatch-order-1", "errorMsg": ""},
        {"success": true, "orderID": "0xbatch-order-2", "errorMsg": ""}
    ]));
    let addr = start_mock_server(state.clone()).await;
    let (mut client, mut rx, cache) = create_test_execution_client(addr);
    client.start().unwrap();

    let instrument_id = InstrumentId::from("TEST-TOKEN.POLYMARKET");
    add_instrument_to_cache(&cache, instrument_id);

    // tick=0.0001 -> tick-relative bounds [0.0001, 0.9999]; orders at both bounds submit through
    // the batch path without a local denial and reach the venue POST.
    let at_min = make_limit_order_at_price(
        "O-LIST-MIN",
        instrument_id,
        OrderSide::Buy,
        false,
        false,
        false,
        TimeInForce::Gtc,
        Price::from("0.0001"),
    );
    let at_max = make_limit_order_at_price(
        "O-LIST-MAX",
        instrument_id,
        OrderSide::Sell,
        false,
        false,
        false,
        TimeInForce::Gtc,
        Price::from("0.9999"),
    );
    cache
        .borrow_mut()
        .add_order(at_min.clone(), None, None, false)
        .unwrap();
    cache
        .borrow_mut()
        .add_order(at_max.clone(), None, None, false)
        .unwrap();

    let cmd = make_submit_order_list_cmd(instrument_id, &[at_min, at_max]);
    client.submit_order_list(cmd).unwrap();

    assert_order_event(recv_execution_event(&mut rx).await, "Submitted");
    assert_order_event(recv_execution_event(&mut rx).await, "Submitted");
    assert_order_event(recv_execution_event(&mut rx).await, "Accepted");
    assert_order_event(recv_execution_event(&mut rx).await, "Accepted");

    assert_eq!(*state.batch_order_post_count.lock().await, 1);
    let body = state.last_body.lock().await.clone().unwrap();
    assert_eq!(body.as_array().unwrap().len(), 2);
}

#[rstest]
#[tokio::test]
async fn test_submit_order_list_singleton_routes_through_single_order_path() {
    let state = TestServerState::default();
    let addr = start_mock_server(state.clone()).await;
    let (mut client, mut rx, cache) = create_test_execution_client(addr);
    client.start().unwrap();

    let instrument_id = InstrumentId::from("TEST-TOKEN.POLYMARKET");
    add_instrument_to_cache(&cache, instrument_id);

    let valid = make_limit_order(
        "O-LIST-SINGLE-VALID",
        instrument_id,
        OrderSide::Buy,
        false,
        false,
        false,
        TimeInForce::Gtc,
    );
    let invalid = make_limit_order(
        "O-LIST-SINGLE-INVALID",
        instrument_id,
        OrderSide::Sell,
        false,
        false,
        true,
        TimeInForce::Ioc,
    );
    cache
        .borrow_mut()
        .add_order(valid.clone(), None, None, false)
        .unwrap();
    cache
        .borrow_mut()
        .add_order(invalid.clone(), None, None, false)
        .unwrap();

    let cmd = make_submit_order_list_cmd(instrument_id, &[valid, invalid]);
    client.submit_order_list(cmd).unwrap();

    assert_order_event(recv_execution_event(&mut rx).await, "Denied");
    assert_order_event(recv_execution_event(&mut rx).await, "Submitted");
    assert_order_event(recv_execution_event(&mut rx).await, "Accepted");

    assert_eq!(*state.batch_order_post_count.lock().await, 0);
    assert_eq!(state.last_path.lock().await.as_str(), "/order");
}

#[rstest]
#[tokio::test]
async fn test_submit_order_list_rejects_failed_batch_response_entry() {
    let state = TestServerState::default();
    *state.batch_order_response.lock().await = Some(json!([
        {"success": false, "orderID": null, "errorMsg": "batch rejection"},
        {"success": true, "orderID": "0xbatch-order-2", "errorMsg": ""}
    ]));
    let addr = start_mock_server(state).await;
    let (mut client, mut rx, cache) = create_test_execution_client(addr);
    client.start().unwrap();

    let instrument_id = InstrumentId::from("TEST-TOKEN.POLYMARKET");
    add_instrument_to_cache(&cache, instrument_id);

    let order1 = make_limit_order(
        "O-LIST-REJECT-1",
        instrument_id,
        OrderSide::Buy,
        false,
        false,
        false,
        TimeInForce::Gtc,
    );
    let order2 = make_limit_order(
        "O-LIST-REJECT-2",
        instrument_id,
        OrderSide::Sell,
        false,
        false,
        false,
        TimeInForce::Gtc,
    );
    cache
        .borrow_mut()
        .add_order(order1.clone(), None, None, false)
        .unwrap();
    cache
        .borrow_mut()
        .add_order(order2.clone(), None, None, false)
        .unwrap();

    let cmd = make_submit_order_list_cmd(instrument_id, &[order1, order2]);
    client.submit_order_list(cmd).unwrap();

    assert_order_event(recv_execution_event(&mut rx).await, "Submitted");
    assert_order_event(recv_execution_event(&mut rx).await, "Submitted");
    assert_order_event(recv_execution_event(&mut rx).await, "Rejected");
    assert_order_event(recv_execution_event(&mut rx).await, "Accepted");
}

#[rstest]
#[tokio::test]
async fn test_submit_order_list_leaves_missing_batch_responses_submitted() {
    let state = TestServerState::default();
    *state.batch_order_response.lock().await = Some(json!([
        {"success": true, "orderID": "0xbatch-order-1", "errorMsg": ""}
    ]));
    let addr = start_mock_server(state.clone()).await;
    let (mut client, mut rx, cache) = create_test_execution_client(addr);
    client.start().unwrap();

    let instrument_id = InstrumentId::from("TEST-TOKEN.POLYMARKET");
    add_instrument_to_cache(&cache, instrument_id);

    let order1 = make_limit_order(
        "O-LIST-MISSING-1",
        instrument_id,
        OrderSide::Buy,
        false,
        false,
        false,
        TimeInForce::Gtc,
    );
    let order2 = make_limit_order(
        "O-LIST-MISSING-2",
        instrument_id,
        OrderSide::Sell,
        false,
        false,
        false,
        TimeInForce::Gtc,
    );
    cache
        .borrow_mut()
        .add_order(order1.clone(), None, None, false)
        .unwrap();
    cache
        .borrow_mut()
        .add_order(order2.clone(), None, None, false)
        .unwrap();

    let cmd = make_submit_order_list_cmd(instrument_id, &[order1, order2]);
    client.submit_order_list(cmd).unwrap();

    let submitted1 = assert_order_event(recv_execution_event(&mut rx).await, "Submitted");
    let submitted2 = assert_order_event(recv_execution_event(&mut rx).await, "Submitted");
    cache.borrow_mut().update_order(&submitted1).unwrap();
    let submitted_order2 = cache.borrow_mut().update_order(&submitted2).unwrap();
    assert_order_event(recv_execution_event(&mut rx).await, "Accepted");
    assert_no_execution_event(&mut rx).await;

    let body = state.last_body.lock().await.clone().unwrap();
    let signed_order: PolymarketOrder = serde_json::from_value(body[1]["order"].clone()).unwrap();
    let expected_venue_order_id =
        VenueOrderId::from(format!("{:#x}", order_hash(&signed_order, false).unwrap()).as_str());
    *state.cancel_response.lock().await = Some(json!({
        "canceled": [expected_venue_order_id.to_string()],
        "not_canceled": {}
    }));

    let pending_cancel = OrderPendingCancel::new(
        submitted_order2.trader_id(),
        submitted_order2.strategy_id(),
        submitted_order2.instrument_id(),
        submitted_order2.client_order_id(),
        Some(AccountId::from("POLYMARKET-001")),
        UUID4::new(),
        UnixNanos::default(),
        UnixNanos::default(),
        false,
        None,
    );
    cache
        .borrow_mut()
        .update_order(&OrderEventAny::PendingCancel(pending_cancel))
        .unwrap();

    client
        .cancel_order(make_cancel_cmd("O-LIST-MISSING-2", instrument_id))
        .unwrap();
    wait_until_async(
        || {
            let state = state.clone();
            async move { *state.cancel_delete_count.lock().await == 1 }
        },
        Duration::from_secs(1),
    )
    .await;

    let cancel_body = state.last_body.lock().await.clone().unwrap();
    assert_eq!(
        cancel_body["orderID"],
        Value::String(expected_venue_order_id.to_string()),
    );
}

#[rstest]
#[tokio::test]
async fn test_submit_order_list_does_not_retry_batch_post_on_http_error() {
    let state = TestServerState::default();
    *state.batch_order_response_status.lock().await = StatusCode::INTERNAL_SERVER_ERROR;
    *state.batch_order_response.lock().await = Some(json!({"error": "batch submit failed"}));
    let addr = start_mock_server(state.clone()).await;
    let (mut client, mut rx, cache) = create_test_execution_client_with_retries(addr, 2);
    client.start().unwrap();

    let instrument_id = InstrumentId::from("TEST-TOKEN.POLYMARKET");
    add_instrument_to_cache(&cache, instrument_id);

    let order1 = make_limit_order(
        "O-LIST-ERR-1",
        instrument_id,
        OrderSide::Buy,
        false,
        false,
        false,
        TimeInForce::Gtc,
    );
    let order2 = make_limit_order(
        "O-LIST-ERR-2",
        instrument_id,
        OrderSide::Sell,
        false,
        false,
        false,
        TimeInForce::Gtc,
    );
    cache
        .borrow_mut()
        .add_order(order1.clone(), None, None, false)
        .unwrap();
    cache
        .borrow_mut()
        .add_order(order2.clone(), None, None, false)
        .unwrap();

    let cmd = make_submit_order_list_cmd(instrument_id, &[order1, order2]);
    client.submit_order_list(cmd).unwrap();

    assert_order_event(recv_execution_event(&mut rx).await, "Submitted");
    assert_order_event(recv_execution_event(&mut rx).await, "Submitted");

    wait_until_async(
        || {
            let state = state.clone();
            async move { *state.batch_order_post_count.lock().await == 1 }
        },
        Duration::from_secs(5),
    )
    .await;
    assert_no_execution_event(&mut rx).await;

    // Confirm no background retry fires after the unknown outcome.
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(*state.batch_order_post_count.lock().await, 1);
    assert!(
        rx.try_recv().is_err(),
        "no further events expected after unknown batch outcome"
    );
}

#[rstest]
#[tokio::test]
async fn test_submit_order_list_routes_market_order_through_single_path() {
    let state = TestServerState::default();
    *state.batch_order_response.lock().await = Some(json!([
        {"success": true, "orderID": "0xmix-limit-1", "errorMsg": ""},
        {"success": true, "orderID": "0xmix-limit-2", "errorMsg": ""}
    ]));
    let addr = start_mock_server(state.clone()).await;
    let (mut client, mut rx, cache) = create_test_execution_client(addr);
    client.start().unwrap();

    let instrument_id = InstrumentId::from("TEST-TOKEN.POLYMARKET");
    add_instrument_to_cache(&cache, instrument_id);

    let market = make_market_order("O-MIX-MKT", instrument_id, OrderSide::Sell, false);
    let limit1 = make_limit_order(
        "O-MIX-LIM-1",
        instrument_id,
        OrderSide::Buy,
        false,
        false,
        false,
        TimeInForce::Gtc,
    );
    let limit2 = make_limit_order(
        "O-MIX-LIM-2",
        instrument_id,
        OrderSide::Sell,
        false,
        false,
        false,
        TimeInForce::Gtc,
    );

    for order in [&market, &limit1, &limit2] {
        cache
            .borrow_mut()
            .add_order(order.clone(), None, None, false)
            .unwrap();
    }

    let cmd = make_submit_order_list_cmd(
        instrument_id,
        &[market.clone(), limit1.clone(), limit2.clone()],
    );
    client.submit_order_list(cmd).unwrap();

    // Market and batch paths spawn independent tasks, so collect events and
    // group them rather than asserting a total order across both tasks.
    let mut submitted = Vec::new();
    let mut accepted = Vec::new();

    for _ in 0..6 {
        let event = recv_execution_event(&mut rx).await;
        match event {
            ExecutionEvent::Order(OrderEventAny::Submitted(e)) => submitted.push(e),
            ExecutionEvent::Order(OrderEventAny::Accepted(e)) => accepted.push(e),
            other => panic!("Unexpected event: {other:?}"),
        }
    }
    assert_eq!(submitted.len(), 3, "one Submitted per order in the list");
    assert_eq!(accepted.len(), 3, "one Accepted per order in the list");

    let submitted_ids: HashSet<String> = submitted
        .iter()
        .map(|e| e.client_order_id.to_string())
        .collect();
    assert!(submitted_ids.contains("O-MIX-MKT"));
    assert!(submitted_ids.contains("O-MIX-LIM-1"));
    assert!(submitted_ids.contains("O-MIX-LIM-2"));

    assert_eq!(
        *state.order_post_count.lock().await,
        1,
        "market order must go through POST /order"
    );
    assert_eq!(
        *state.batch_order_post_count.lock().await,
        1,
        "limit orders must go through POST /orders"
    );
    let body = state.last_body.lock().await.clone().unwrap();
    // last_body races between the two handlers; either handler's body is
    // valid, so assert whichever shape we got is well-formed.
    match body {
        Value::Array(ref entries) => assert_eq!(entries.len(), 2),
        Value::Object(ref obj) => assert!(obj.contains_key("order")),
        other => panic!("unexpected last_body shape: {other:?}"),
    }
}

#[rstest]
#[tokio::test]
async fn test_submit_order_list_preserves_rejected_reason_from_batch_response() {
    let state = TestServerState::default();
    *state.batch_order_response.lock().await = Some(json!([
        {"success": false, "orderID": null, "errorMsg": "insufficient balance"},
        {"success": true, "orderID": "0xreason-2", "errorMsg": ""}
    ]));
    let addr = start_mock_server(state).await;
    let (mut client, mut rx, cache) = create_test_execution_client(addr);
    client.start().unwrap();

    let instrument_id = InstrumentId::from("TEST-TOKEN.POLYMARKET");
    add_instrument_to_cache(&cache, instrument_id);

    let order1 = make_limit_order(
        "O-LIST-REASON-1",
        instrument_id,
        OrderSide::Buy,
        false,
        false,
        false,
        TimeInForce::Gtc,
    );
    let order2 = make_limit_order(
        "O-LIST-REASON-2",
        instrument_id,
        OrderSide::Sell,
        false,
        false,
        false,
        TimeInForce::Gtc,
    );
    cache
        .borrow_mut()
        .add_order(order1.clone(), None, None, false)
        .unwrap();
    cache
        .borrow_mut()
        .add_order(order2.clone(), None, None, false)
        .unwrap();

    let cmd = make_submit_order_list_cmd(instrument_id, &[order1, order2]);
    client.submit_order_list(cmd).unwrap();

    assert_order_event(recv_execution_event(&mut rx).await, "Submitted");
    assert_order_event(recv_execution_event(&mut rx).await, "Submitted");
    let rejected = assert_order_event(recv_execution_event(&mut rx).await, "Rejected");
    assert_order_event(recv_execution_event(&mut rx).await, "Accepted");

    let reason = order_event_reason(&rejected);
    assert!(
        reason.contains("insufficient balance"),
        "Rejected reason should preserve errorMsg, was {reason}"
    );
}

#[rstest]
#[case::unknown_client_id("unknown")]
#[case::closed_order("closed")]
#[case::unsupported_order_type("unsupported")]
#[case::missing_instrument("missing_instrument")]
#[tokio::test]
async fn test_submit_order_list_filters_out_ineligible_entries(#[case] kind: &str) {
    let state = TestServerState::default();
    *state.batch_order_response.lock().await = Some(json!([
        {"success": true, "orderID": "0xfilter-1", "errorMsg": ""},
        {"success": true, "orderID": "0xfilter-2", "errorMsg": ""}
    ]));
    let addr = start_mock_server(state.clone()).await;
    let (mut client, mut rx, cache) = create_test_execution_client(addr);
    client.start().unwrap();

    let instrument_id = InstrumentId::from("TEST-TOKEN.POLYMARKET");
    add_instrument_to_cache(&cache, instrument_id);

    let valid1 = make_limit_order(
        "O-FILTER-VALID-1",
        instrument_id,
        OrderSide::Buy,
        false,
        false,
        false,
        TimeInForce::Gtc,
    );
    let valid2 = make_limit_order(
        "O-FILTER-VALID-2",
        instrument_id,
        OrderSide::Sell,
        false,
        false,
        false,
        TimeInForce::Gtc,
    );
    cache
        .borrow_mut()
        .add_order(valid1.clone(), None, None, false)
        .unwrap();
    cache
        .borrow_mut()
        .add_order(valid2.clone(), None, None, false)
        .unwrap();

    let ineligible = match kind {
        "unknown" => {
            // Build an order without inserting it into the cache.
            make_limit_order(
                "O-FILTER-UNKNOWN",
                instrument_id,
                OrderSide::Buy,
                false,
                false,
                false,
                TimeInForce::Gtc,
            )
        }
        "closed" => {
            let closed = make_closed_limit_order("O-FILTER-CLOSED", instrument_id, OrderSide::Buy);
            cache
                .borrow_mut()
                .add_order(closed.clone(), None, None, false)
                .unwrap();
            closed
        }
        "unsupported" => {
            let stop = make_stop_market_order("O-FILTER-STOP", instrument_id, OrderSide::Buy);
            cache
                .borrow_mut()
                .add_order(stop.clone(), None, None, false)
                .unwrap();
            stop
        }
        "missing_instrument" => {
            let other_instrument = InstrumentId::from("OTHER-TOKEN.POLYMARKET");
            let order = make_limit_order(
                "O-FILTER-MISSING",
                other_instrument,
                OrderSide::Buy,
                false,
                false,
                false,
                TimeInForce::Gtc,
            );
            cache
                .borrow_mut()
                .add_order(order.clone(), None, None, false)
                .unwrap();
            order
        }
        other => panic!("unknown case: {other}"),
    };

    let cmd =
        make_submit_order_list_cmd(instrument_id, &[valid1.clone(), ineligible, valid2.clone()]);
    client.submit_order_list(cmd).unwrap();

    // Entries that require an explicit Denied event before the batch fires.
    let expect_denied_first = matches!(kind, "unsupported" | "missing_instrument");
    if expect_denied_first {
        let denied = assert_order_event(recv_execution_event(&mut rx).await, "Denied");
        let reason = order_event_reason(&denied);

        match kind {
            "unsupported" => assert!(
                reason.contains("Unsupported order type"),
                "reason was {reason}"
            ),
            "missing_instrument" => {
                assert!(reason.contains(INSTRUMENT_NOT_FOUND), "reason was {reason}");
            }
            _ => unreachable!(),
        }
    }

    assert_order_event(recv_execution_event(&mut rx).await, "Submitted");
    assert_order_event(recv_execution_event(&mut rx).await, "Submitted");
    assert_order_event(recv_execution_event(&mut rx).await, "Accepted");
    assert_order_event(recv_execution_event(&mut rx).await, "Accepted");

    assert_eq!(*state.batch_order_post_count.lock().await, 1);
    let body = state.last_body.lock().await.clone().unwrap();
    assert_eq!(
        body.as_array().unwrap().len(),
        2,
        "ineligible entry must not appear in the batch body"
    );
}

#[rstest]
#[tokio::test]
async fn test_submit_order_list_routes_remainder_singleton_through_single_order_path() {
    const TOTAL: usize = 16;

    let state = TestServerState::default();
    let addr = start_mock_server(state.clone()).await;
    let (mut client, mut rx, cache) = create_test_execution_client(addr);
    client.start().unwrap();

    let instrument_id = InstrumentId::from("TEST-TOKEN.POLYMARKET");
    add_instrument_to_cache(&cache, instrument_id);

    let orders: Vec<OrderAny> = (0..TOTAL)
        .map(|i| {
            let order = make_limit_order(
                &format!("O-REM-{i}"),
                instrument_id,
                OrderSide::Buy,
                false,
                false,
                false,
                TimeInForce::Gtc,
            );
            cache
                .borrow_mut()
                .add_order(order.clone(), None, None, false)
                .unwrap();
            order
        })
        .collect();

    let cmd = make_submit_order_list_cmd(instrument_id, &orders);
    client.submit_order_list(cmd).unwrap();

    for _ in 0..TOTAL {
        assert_order_event(recv_execution_event(&mut rx).await, "Submitted");
    }

    for _ in 0..TOTAL {
        assert_order_event(recv_execution_event(&mut rx).await, "Accepted");
    }

    assert_eq!(
        *state.batch_order_post_count.lock().await,
        1,
        "the first 15 orders use POST /orders"
    );
    assert_eq!(
        *state.order_post_count.lock().await,
        1,
        "the remainder singleton must use the retrying POST /order path"
    );
}

#[rstest]
#[tokio::test]
async fn test_submit_order_list_chunks_beyond_batch_order_limit() {
    const TOTAL: usize = 17;

    let state = TestServerState::default();
    let addr = start_mock_server(state.clone()).await;
    let (mut client, mut rx, cache) = create_test_execution_client(addr);
    client.start().unwrap();

    let instrument_id = InstrumentId::from("TEST-TOKEN.POLYMARKET");
    add_instrument_to_cache(&cache, instrument_id);

    let orders: Vec<OrderAny> = (0..TOTAL)
        .map(|i| {
            let order = make_limit_order(
                &format!("O-CHUNK-{i}"),
                instrument_id,
                if i % 2 == 0 {
                    OrderSide::Buy
                } else {
                    OrderSide::Sell
                },
                false,
                false,
                false,
                TimeInForce::Gtc,
            );
            cache
                .borrow_mut()
                .add_order(order.clone(), None, None, false)
                .unwrap();
            order
        })
        .collect();

    let cmd = make_submit_order_list_cmd(instrument_id, &orders);
    client.submit_order_list(cmd).unwrap();

    for _ in 0..TOTAL {
        assert_order_event(recv_execution_event(&mut rx).await, "Submitted");
    }

    for _ in 0..TOTAL {
        assert_order_event(recv_execution_event(&mut rx).await, "Accepted");
    }

    assert_eq!(
        *state.batch_order_post_count.lock().await,
        2,
        "17 orders must split into two POST /orders calls (15 + 2)"
    );
    // last_body reflects the most recent chunk; confirm it's the remainder.
    let body = state.last_body.lock().await.clone().unwrap();
    assert_eq!(body.as_array().unwrap().len(), 2);
}

#[rstest]
#[tokio::test]
async fn test_cancel_order_local_validation_failure_does_not_emit_cancel_rejected() {
    let state = TestServerState::default();
    let addr = start_mock_server(state).await;
    let (mut client, mut rx, cache) = create_test_execution_client(addr);
    client.start().unwrap();

    let instrument_id = InstrumentId::from("TEST-TOKEN.POLYMARKET");
    let order = make_limit_order(
        "O-CANCEL-INIT",
        instrument_id,
        OrderSide::Buy,
        false,
        false,
        false,
        TimeInForce::Gtc,
    );

    // Order is in Initialized state (not open), just add it
    cache
        .borrow_mut()
        .add_order(order, None, None, false)
        .unwrap();

    let cmd = make_cancel_cmd("O-CANCEL-INIT", instrument_id);
    client.cancel_order(cmd).unwrap();

    assert_no_execution_event(&mut rx).await;
}

#[rstest]
#[tokio::test]
async fn test_cancel_order_success_no_rejection_event() {
    let state = TestServerState::default();
    *state.cancel_response.lock().await = Some(json!({
        "canceled": ["0xvenue-cancel-ok"],
        "not_canceled": {}
    }));
    let addr = start_mock_server(state.clone()).await;
    let (mut client, mut rx, cache) = create_test_execution_client(addr);
    client.start().unwrap();

    let instrument_id = InstrumentId::from("TEST-TOKEN.POLYMARKET");
    let mut order = make_limit_order(
        "O-CANCEL-OK",
        instrument_id,
        OrderSide::Buy,
        false,
        false,
        false,
        TimeInForce::Gtc,
    );

    cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .unwrap();
    submit_and_accept_order(&cache, &mut order, "0xvenue-cancel-ok");

    let cmd = make_cancel_cmd("O-CANCEL-OK", instrument_id);
    client.cancel_order(cmd).unwrap();

    wait_until_async(
        || {
            let state = state.clone();
            async move { *state.cancel_delete_count.lock().await == 1 }
        },
        Duration::from_secs(5),
    )
    .await;
    assert_no_execution_event(&mut rx).await;
}

#[rstest]
#[tokio::test]
async fn test_cancel_order_ambiguous_http_failure_does_not_emit_cancel_rejected() {
    let state = TestServerState::default();
    *state.cancel_response_status.lock().await = StatusCode::INTERNAL_SERVER_ERROR;
    *state.cancel_response.lock().await = Some(json!({"error": "cancel failed"}));
    let addr = start_mock_server(state.clone()).await;
    let (mut client, mut rx, cache) = create_test_execution_client_with_retries(addr, 2);
    client.start().unwrap();

    let instrument_id = InstrumentId::from("TEST-TOKEN.POLYMARKET");
    let mut order = make_limit_order(
        "O-CANCEL-AMBIGUOUS",
        instrument_id,
        OrderSide::Buy,
        false,
        false,
        false,
        TimeInForce::Gtc,
    );

    cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .unwrap();
    submit_and_accept_order(&cache, &mut order, "0xvenue-cancel-ambiguous");

    let cmd = make_cancel_cmd("O-CANCEL-AMBIGUOUS", instrument_id);
    client.cancel_order(cmd).unwrap();

    wait_until_async(
        || {
            let state = state.clone();
            async move { *state.cancel_delete_count.lock().await == 3 }
        },
        Duration::from_secs(5),
    )
    .await;
    assert_no_execution_event(&mut rx).await;
}

#[rstest]
#[tokio::test]
async fn test_cancel_order_parse_failure_after_send_does_not_emit_cancel_rejected() {
    let state = TestServerState::default();
    *state.cancel_response.lock().await = Some(json!("not a cancel response"));
    let addr = start_mock_server(state.clone()).await;
    let (mut client, mut rx, cache) = create_test_execution_client(addr);
    client.start().unwrap();

    let instrument_id = InstrumentId::from("TEST-TOKEN.POLYMARKET");
    let mut order = make_limit_order(
        "O-CANCEL-PARSE",
        instrument_id,
        OrderSide::Buy,
        false,
        false,
        false,
        TimeInForce::Gtc,
    );

    cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .unwrap();
    submit_and_accept_order(&cache, &mut order, "0xvenue-cancel-parse");

    let cmd = make_cancel_cmd("O-CANCEL-PARSE", instrument_id);
    client.cancel_order(cmd).unwrap();

    wait_until_async(
        || {
            let state = state.clone();
            async move { *state.cancel_delete_count.lock().await == 1 }
        },
        Duration::from_secs(5),
    )
    .await;
    assert_no_execution_event(&mut rx).await;
}

#[rstest]
#[tokio::test]
async fn test_cancel_order_already_done_suppresses_rejection() {
    let state = TestServerState::default();
    *state.cancel_response.lock().await = Some(load_json("http_cancel_response_failed.json"));
    let addr = start_mock_server(state.clone()).await;
    let (mut client, mut rx, cache) = create_test_execution_client(addr);
    client.start().unwrap();

    let instrument_id = InstrumentId::from("TEST-TOKEN.POLYMARKET");
    let mut order = make_limit_order(
        "O-CANCEL-DONE",
        instrument_id,
        OrderSide::Buy,
        false,
        false,
        false,
        TimeInForce::Gtc,
    );

    cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .unwrap();
    submit_and_accept_order(&cache, &mut order, CANCEL_ALREADY_DONE_ORDER_ID);

    let cmd = make_cancel_cmd("O-CANCEL-DONE", instrument_id);
    client.cancel_order(cmd).unwrap();

    wait_until_async(
        || {
            let state = state.clone();
            async move { *state.cancel_delete_count.lock().await == 1 }
        },
        Duration::from_secs(5),
    )
    .await;
    assert_no_execution_event(&mut rx).await;
}

#[rstest]
#[tokio::test]
async fn test_cancel_order_explicit_structured_rejection_emits_cancel_rejected() {
    let state = TestServerState::default();
    *state.cancel_response.lock().await = Some(json!({
        "canceled": [],
        "not_canceled": {
            "0xvenue-cancel-fail": "order not found"
        }
    }));
    let addr = start_mock_server(state).await;
    let (mut client, mut rx, cache) = create_test_execution_client(addr);
    client.start().unwrap();

    let instrument_id = InstrumentId::from("TEST-TOKEN.POLYMARKET");
    let mut order = make_limit_order(
        "O-CANCEL-FAIL",
        instrument_id,
        OrderSide::Buy,
        false,
        false,
        false,
        TimeInForce::Gtc,
    );

    cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .unwrap();
    submit_and_accept_order(&cache, &mut order, "0xvenue-cancel-fail");

    let cmd = make_cancel_cmd("O-CANCEL-FAIL", instrument_id);
    client.cancel_order(cmd).unwrap();

    let event = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .unwrap()
        .unwrap();
    assert_order_event(event, "CancelRejected");
}

#[rstest]
#[tokio::test]
async fn test_batch_cancel_orders_with_partial_failure() {
    let state = TestServerState::default();
    let addr = start_mock_server(state).await;
    let (mut client, mut rx, cache) = create_test_execution_client(addr);
    client.start().unwrap();

    let instrument_id = InstrumentId::from("TEST-TOKEN.POLYMARKET");

    // Create 3 orders, matching the fixture:
    // - 0x111...111 and 0x222...222 are canceled (success)
    // - 0x333...333 is not_canceled (already canceled or matched)
    let mut order1 = make_limit_order(
        "O-BATCH-1",
        instrument_id,
        OrderSide::Buy,
        false,
        false,
        false,
        TimeInForce::Gtc,
    );
    cache
        .borrow_mut()
        .add_order(order1.clone(), None, None, false)
        .unwrap();
    submit_and_accept_order(
        &cache,
        &mut order1,
        "0x1111111111111111111111111111111111111111111111111111111111111111",
    );

    let mut order2 = make_limit_order(
        "O-BATCH-2",
        instrument_id,
        OrderSide::Sell,
        false,
        false,
        false,
        TimeInForce::Gtc,
    );
    cache
        .borrow_mut()
        .add_order(order2.clone(), None, None, false)
        .unwrap();
    submit_and_accept_order(
        &cache,
        &mut order2,
        "0x2222222222222222222222222222222222222222222222222222222222222222",
    );

    let mut order3 = make_limit_order(
        "O-BATCH-3",
        instrument_id,
        OrderSide::Buy,
        false,
        false,
        false,
        TimeInForce::Gtc,
    );
    cache
        .borrow_mut()
        .add_order(order3.clone(), None, None, false)
        .unwrap();
    submit_and_accept_order(
        &cache,
        &mut order3,
        "0x3333333333333333333333333333333333333333333333333333333333333333",
    );

    let cancels = vec![
        make_cancel_cmd("O-BATCH-1", instrument_id),
        make_cancel_cmd("O-BATCH-2", instrument_id),
        make_cancel_cmd("O-BATCH-3", instrument_id),
    ];

    let cmd = BatchCancelOrders::new(
        TraderId::from("TESTER-001"),
        Some(*POLYMARKET_CLIENT_ID),
        StrategyId::from("S-001"),
        instrument_id,
        cancels,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None, // correlation_id
    );

    client.batch_cancel_orders(cmd).unwrap();

    // Order 3 has CANCEL_ALREADY_DONE, so it should be suppressed.
    // No CancelRejected events expected.
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(rx.try_recv().is_err());
}

#[rstest]
#[case::batch(
    ShutdownCancelMode::Batch,
    1_121,
    TEST_CHUNK_PRIVATE_KEY,
    CancelChunkRetry::Succeeds
)]
#[case::cancel_all(
    ShutdownCancelMode::CancelAll,
    121,
    TEST_CANCEL_ALL_PRIVATE_KEY,
    CancelChunkRetry::None
)]
#[case::late_failure(
    ShutdownCancelMode::Batch,
    121,
    TEST_CHUNK_FAILURE_PRIVATE_KEY,
    CancelChunkRetry::Exhausts
)]
#[case::tier_downgrade(
    ShutdownCancelMode::Batch,
    241,
    TEST_CHUNK_DOWNGRADE_PRIVATE_KEY,
    CancelChunkRetry::Downgrades
)]
#[tokio::test]
async fn test_group_cancel_orders_bounds_retries_and_result_processing(
    #[case] mode: ShutdownCancelMode,
    #[case] order_count: usize,
    #[case] private_key: &str,
    #[case] chunk_retry: CancelChunkRetry,
) {
    let state = TestServerState::default();
    state
        .batch_cancel_echo_rejections
        .store(true, Ordering::Release);

    match chunk_retry {
        CancelChunkRetry::None => {}
        CancelChunkRetry::Succeeds => {
            state.batch_cancel_response_statuses.lock().await.extend([
                StatusCode::OK,
                StatusCode::INTERNAL_SERVER_ERROR,
                StatusCode::OK,
                StatusCode::OK,
            ]);
            let mut headers = HeaderMap::new();
            headers.insert("poly-ratelimit-tier", "Gold".parse().unwrap());
            state
                .batch_cancel_response_headers
                .lock()
                .await
                .push_back(headers);
        }
        CancelChunkRetry::Exhausts => {
            state.batch_cancel_response_statuses.lock().await.extend([
                StatusCode::OK,
                StatusCode::INTERNAL_SERVER_ERROR,
                StatusCode::INTERNAL_SERVER_ERROR,
            ]);
        }
        CancelChunkRetry::Downgrades => {
            state.batch_cancel_response_statuses.lock().await.extend([
                StatusCode::OK,
                StatusCode::INTERNAL_SERVER_ERROR,
                StatusCode::OK,
                StatusCode::OK,
            ]);
            let mut headers = state.batch_cancel_response_headers.lock().await;

            for tier in ["Gold", "Standard"] {
                let mut response_headers = HeaderMap::new();
                response_headers.insert("poly-ratelimit-tier", tier.parse().unwrap());
                headers.push_back(response_headers);
            }
        }
    }
    let addr = start_mock_server(state.clone()).await;
    let max_retries = u32::from(!matches!(chunk_retry, CancelChunkRetry::None));
    let mut config = create_test_exec_config_with_retries(addr, max_retries);
    config.private_key = Some(private_key.to_string());
    let (mut client, mut rx, cache) = create_test_execution_client_from_config(config);
    client.start().unwrap();

    let instrument_id = InstrumentId::from("TEST-TOKEN.POLYMARKET");
    let mut cancels = Vec::with_capacity(order_count);
    let mut expected_client_order_ids = HashSet::with_capacity(order_count);
    let mut venue_order_ids = Vec::with_capacity(order_count);
    for index in 0..order_count {
        let client_order_id = format!("O-CHUNK-{index}");
        let venue_order_id = format!("0x{index:064x}");
        let mut order = make_limit_order(
            &client_order_id,
            instrument_id,
            OrderSide::Buy,
            false,
            false,
            false,
            TimeInForce::Gtc,
        );
        cache
            .borrow_mut()
            .add_order(order.clone(), None, None, false)
            .unwrap();
        submit_and_accept_order(&cache, &mut order, &venue_order_id);
        cancels.push(make_cancel_cmd(&client_order_id, instrument_id));
        expected_client_order_ids.insert(client_order_id);
        venue_order_ids.push(venue_order_id);
    }

    match mode {
        ShutdownCancelMode::Batch => client
            .batch_cancel_orders(BatchCancelOrders::new(
                TraderId::from("TESTER-001"),
                Some(*POLYMARKET_CLIENT_ID),
                StrategyId::from("S-001"),
                instrument_id,
                cancels,
                UUID4::new(),
                UnixNanos::default(),
                None,
                None,
            ))
            .unwrap(),
        ShutdownCancelMode::CancelAll => client
            .cancel_all_orders(CancelAllOrders::new(
                TraderId::from("TESTER-001"),
                Some(*POLYMARKET_CLIENT_ID),
                StrategyId::from("S-001"),
                instrument_id,
                OrderSide::Buy,
                UUID4::new(),
                UnixNanos::default(),
                None,
                None,
            ))
            .unwrap(),
        ShutdownCancelMode::Individual => unreachable!(),
    }

    let mut processed_client_order_ids = HashSet::with_capacity(order_count);

    if matches!(chunk_retry, CancelChunkRetry::Exhausts) {
        wait_until_async(
            || {
                let state = state.clone();
                async move { *state.batch_cancel_delete_count.lock().await == 3 }
            },
            Duration::from_secs(5),
        )
        .await;
    } else {
        for _ in 0..order_count {
            let event = assert_order_event(recv_execution_event(&mut rx).await, "CancelRejected");
            let OrderEventAny::CancelRejected(event) = event else {
                unreachable!();
            };
            assert!(processed_client_order_ids.insert(event.client_order_id.to_string()));
        }
    }
    assert_no_execution_event(&mut rx).await;

    let bodies = state.batch_cancel_bodies.lock().await;
    let body_lengths = bodies
        .iter()
        .map(|body| body.as_array().unwrap().len())
        .collect::<Vec<_>>();

    match chunk_retry {
        CancelChunkRetry::None => assert_eq!(body_lengths, vec![120, 1]),
        CancelChunkRetry::Succeeds => {
            assert_eq!(body_lengths, vec![120, 1_000, 1_000, 1]);
            assert_eq!(bodies[1], bodies[2]);
        }
        CancelChunkRetry::Exhausts => {
            assert_eq!(body_lengths, vec![120, 1, 1]);
            assert_eq!(bodies[1], bodies[2]);
        }
        CancelChunkRetry::Downgrades => {
            assert_eq!(body_lengths, vec![120, 121, 120, 1]);
        }
    }
    let successful_body_indices: &[usize] = match chunk_retry {
        CancelChunkRetry::None => &[0, 1],
        CancelChunkRetry::Succeeds => &[0, 2, 3],
        CancelChunkRetry::Exhausts => &[0],
        CancelChunkRetry::Downgrades => &[0, 2, 3],
    };
    let successful_order_ids = bodies
        .iter()
        .enumerate()
        .filter(|(index, _)| successful_body_indices.contains(index))
        .flat_map(|(_, body)| body.as_array().unwrap())
        .map(|order_id| order_id.as_str().unwrap().to_string())
        .collect::<HashSet<_>>();
    let expected_successful_order_ids = if matches!(chunk_retry, CancelChunkRetry::Exhausts) {
        venue_order_ids[..120].iter().cloned().collect()
    } else {
        venue_order_ids.iter().cloned().collect()
    };

    assert_eq!(successful_order_ids, expected_successful_order_ids);

    if matches!(chunk_retry, CancelChunkRetry::Exhausts) {
        assert!(processed_client_order_ids.is_empty());
    } else {
        assert_eq!(processed_client_order_ids, expected_client_order_ids);
    }
    assert_eq!(*state.batch_cancel_delete_count.lock().await, bodies.len());
}

#[rstest]
#[tokio::test]
async fn test_batch_cancel_orders_whole_http_failure_does_not_emit_cancel_rejected_per_order() {
    let state = TestServerState::default();
    *state.batch_cancel_response_status.lock().await = StatusCode::INTERNAL_SERVER_ERROR;
    *state.batch_cancel_response.lock().await = Some(json!({"error": "batch cancel failed"}));
    let addr = start_mock_server(state.clone()).await;
    let (mut client, mut rx, cache) = create_test_execution_client_with_retries(addr, 2);
    client.start().unwrap();

    let instrument_id = InstrumentId::from("TEST-TOKEN.POLYMARKET");

    let mut order1 = make_limit_order(
        "O-BATCH-FAIL-1",
        instrument_id,
        OrderSide::Buy,
        false,
        false,
        false,
        TimeInForce::Gtc,
    );
    cache
        .borrow_mut()
        .add_order(order1.clone(), None, None, false)
        .unwrap();
    submit_and_accept_order(&cache, &mut order1, "0xvenue-batch-fail-1");

    let mut order2 = make_limit_order(
        "O-BATCH-FAIL-2",
        instrument_id,
        OrderSide::Sell,
        false,
        false,
        false,
        TimeInForce::Gtc,
    );
    cache
        .borrow_mut()
        .add_order(order2.clone(), None, None, false)
        .unwrap();
    submit_and_accept_order(&cache, &mut order2, "0xvenue-batch-fail-2");

    let cancels = vec![
        make_cancel_cmd("O-BATCH-FAIL-1", instrument_id),
        make_cancel_cmd("O-BATCH-FAIL-2", instrument_id),
    ];

    let cmd = BatchCancelOrders::new(
        TraderId::from("TESTER-001"),
        Some(*POLYMARKET_CLIENT_ID),
        StrategyId::from("S-001"),
        instrument_id,
        cancels,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );

    client.batch_cancel_orders(cmd).unwrap();

    wait_until_async(
        || {
            let state = state.clone();
            async move { *state.batch_cancel_delete_count.lock().await == 3 }
        },
        Duration::from_secs(5),
    )
    .await;
    assert_no_execution_event(&mut rx).await;
}

#[rstest]
#[case(ShutdownCancelMode::Individual)]
#[case(ShutdownCancelMode::CancelAll)]
#[case(ShutdownCancelMode::Batch)]
#[tokio::test]
async fn test_stop_does_not_abort_shutdown_cancel_response(#[case] mode: ShutdownCancelMode) {
    let state = TestServerState::default();
    let venue_order_id = "0xvenue-shutdown-cancel";
    let rejection = json!({
        "canceled": [],
        "not_canceled": {(venue_order_id): "order not found"}
    });
    *state.cancel_response.lock().await = Some(rejection.clone());
    *state.batch_cancel_response.lock().await = Some(rejection);
    state.cancel_request_gate.enable();
    state.batch_cancel_request_gate.enable();

    let addr = start_mock_server(state.clone()).await;
    let (mut client, mut rx, cache) = create_test_execution_client(addr);
    client.start().unwrap();

    let instrument_id = InstrumentId::from("TEST-TOKEN.POLYMARKET");
    let mut order = make_limit_order(
        "O-SHUTDOWN-CANCEL",
        instrument_id,
        OrderSide::Buy,
        false,
        false,
        false,
        TimeInForce::Gtc,
    );
    cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .unwrap();
    submit_and_accept_order(&cache, &mut order, venue_order_id);

    let cancel = make_cancel_cmd("O-SHUTDOWN-CANCEL", instrument_id);
    match mode {
        ShutdownCancelMode::Individual => client.cancel_order(cancel).unwrap(),
        ShutdownCancelMode::CancelAll => client
            .cancel_all_orders(CancelAllOrders::new(
                TraderId::from("TESTER-001"),
                Some(*POLYMARKET_CLIENT_ID),
                StrategyId::from("S-001"),
                instrument_id,
                OrderSide::Buy,
                UUID4::new(),
                UnixNanos::default(),
                None,
                None,
            ))
            .unwrap(),
        ShutdownCancelMode::Batch => client
            .batch_cancel_orders(BatchCancelOrders::new(
                TraderId::from("TESTER-001"),
                Some(*POLYMARKET_CLIENT_ID),
                StrategyId::from("S-001"),
                instrument_id,
                vec![cancel],
                UUID4::new(),
                UnixNanos::default(),
                None,
                None,
            ))
            .unwrap(),
    }

    let gate = match mode {
        ShutdownCancelMode::Individual => state.cancel_request_gate.clone(),
        ShutdownCancelMode::CancelAll | ShutdownCancelMode::Batch => {
            state.batch_cancel_request_gate.clone()
        }
    };
    wait_until_async(
        || {
            let gate = gate.clone();
            async move { gate.started() == 1 }
        },
        Duration::from_secs(1),
    )
    .await;

    client.stop().unwrap();
    gate.release();

    let event = tokio::time::timeout(Duration::from_secs(1), rx.recv())
        .await
        .expect("shutdown cancel response should be processed")
        .expect("execution event channel should remain open");
    assert_order_event(event, "CancelRejected");
}

#[rstest]
#[tokio::test]
async fn test_disconnect_waits_for_shutdown_cancel_response() {
    let state = TestServerState::default();
    let venue_order_id = "0xvenue-disconnect-cancel";
    *state.cancel_response.lock().await = Some(json!({
        "canceled": [],
        "not_canceled": {(venue_order_id): "order not found"}
    }));
    state.cancel_request_gate.enable();

    let addr = start_mock_server(state.clone()).await;
    let (mut client, mut rx, cache) = create_test_execution_client(addr);
    add_test_account_to_cache(&cache, AccountId::from("POLYMARKET-001"));
    client.start().unwrap();
    client.connect().await.unwrap();

    while rx.try_recv().is_ok() {}

    let instrument_id = InstrumentId::from("TEST-TOKEN.POLYMARKET");
    let mut order = make_limit_order(
        "O-DISCONNECT-CANCEL",
        instrument_id,
        OrderSide::Buy,
        false,
        false,
        false,
        TimeInForce::Gtc,
    );
    cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .unwrap();
    submit_and_accept_order(&cache, &mut order, venue_order_id);
    client
        .cancel_order(make_cancel_cmd("O-DISCONNECT-CANCEL", instrument_id))
        .unwrap();

    wait_until_async(
        || {
            let gate = state.cancel_request_gate.clone();
            async move { gate.started() == 1 }
        },
        Duration::from_secs(1),
    )
    .await;

    let mut disconnect = Box::pin(client.disconnect());
    assert!(
        tokio::time::timeout(Duration::from_millis(50), disconnect.as_mut())
            .await
            .is_err(),
        "disconnect should wait for the in-flight cancel response"
    );

    state.cancel_request_gate.release();
    disconnect.await.unwrap();

    let event = tokio::time::timeout(Duration::from_secs(1), rx.recv())
        .await
        .expect("disconnect cancel response should be processed")
        .expect("execution event channel should remain open");
    assert_order_event(event, "CancelRejected");
}

fn submit_and_pending_cancel(cache: &Rc<RefCell<Cache>>, order: &mut OrderAny) {
    let account_id = AccountId::from("POLYMARKET-001");
    let submitted = TestOrderEventStubs::submitted(order, account_id);
    *order = cache.borrow_mut().update_order(&submitted).unwrap();

    let pending_cancel = OrderPendingCancel::new(
        order.trader_id(),
        order.strategy_id(),
        order.instrument_id(),
        order.client_order_id(),
        Some(account_id),
        UUID4::new(),
        UnixNanos::default(),
        UnixNanos::default(),
        false,
        None, // No venue_order_id yet
    );
    *order = cache
        .borrow_mut()
        .update_order(&OrderEventAny::PendingCancel(pending_cancel))
        .unwrap();
}

#[rstest]
#[tokio::test]
async fn test_cancel_order_deferred_when_no_venue_order_id() {
    let state = TestServerState::default();
    *state.cancel_response.lock().await = Some(json!({
        "canceled": [DEFAULT_ACCEPTED_ORDER_ID],
        "not_canceled": {}
    }));
    let addr = start_mock_server(state.clone()).await;
    let (mut client, mut rx, cache) = create_test_execution_client(addr);
    client.start().unwrap();

    let instrument_id = InstrumentId::from("TEST-TOKEN.POLYMARKET");
    add_instrument_to_cache(&cache, instrument_id);

    let mut order = make_limit_order(
        "O-DEFERRED-CANCEL",
        instrument_id,
        OrderSide::Buy,
        false,
        false,
        false,
        TimeInForce::Gtc,
    );

    cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .unwrap();

    // Transition order to PENDING_CANCEL without a venue_order_id
    submit_and_pending_cancel(&cache, &mut order);

    // Cancel should be deferred (no venue_order_id available)
    let cmd = make_cancel_cmd("O-DEFERRED-CANCEL", instrument_id);
    client.cancel_order(cmd).unwrap();

    // No events emitted yet
    assert!(rx.try_recv().is_err());

    // Submit the order, triggering the HTTP response with a venue_order_id.
    // handle_order_response detects the pending cancel and issues the deferred cancel.
    let submit_cmd = make_submit_cmd(&order, instrument_id);
    client.submit_order(submit_cmd).unwrap();

    // Submitted event (sync)
    let event = rx.try_recv().unwrap();
    assert_order_event(event, "Submitted");

    // Accepted event (async, from HTTP response)
    let event = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .unwrap()
        .unwrap();
    assert_order_event(event, "Accepted");

    wait_until_async(
        || {
            let state = state.clone();
            async move { *state.cancel_delete_count.lock().await == 1 }
        },
        Duration::from_secs(5),
    )
    .await;
    assert_no_execution_event(&mut rx).await;
}

#[rstest]
#[case(ShutdownCancelMode::CancelAll, true)]
#[case(ShutdownCancelMode::Batch, true)]
#[case(ShutdownCancelMode::CancelAll, false)]
#[case(ShutdownCancelMode::Batch, false)]
#[tokio::test]
async fn test_group_cancel_around_batch_submit_ack_is_not_lost(
    #[case] mode: ShutdownCancelMode,
    #[case] cancel_before_ack: bool,
) {
    let state = TestServerState::default();
    let venue_order_ids = ["0xvenue-group-deferred-1", "0xvenue-group-deferred-2"];
    *state.batch_order_response.lock().await = Some(json!([
        {"success": true, "orderID": venue_order_ids[0], "errorMsg": null},
        {"success": true, "orderID": venue_order_ids[1], "errorMsg": null}
    ]));
    *state.cancel_response.lock().await = Some(json!({
        "canceled": venue_order_ids,
        "not_canceled": {}
    }));
    *state.batch_cancel_response.lock().await = Some(json!({
        "canceled": venue_order_ids,
        "not_canceled": {}
    }));
    state.batch_order_request_gate.enable();

    let addr = start_mock_server(state.clone()).await;
    let (mut client, mut rx, cache) = create_test_execution_client(addr);
    client.start().unwrap();

    let instrument_id = InstrumentId::from("TEST-TOKEN.POLYMARKET");
    add_instrument_to_cache(&cache, instrument_id);
    let mut orders: Vec<OrderAny> = (0..2)
        .map(|index| {
            make_limit_order(
                &format!("O-GROUP-DEFERRED-{index}"),
                instrument_id,
                OrderSide::Buy,
                false,
                false,
                false,
                TimeInForce::Gtc,
            )
        })
        .collect();

    for order in &orders {
        cache
            .borrow_mut()
            .add_order(order.clone(), None, None, false)
            .unwrap();
    }

    client
        .submit_order_list(make_submit_order_list_cmd(instrument_id, &orders))
        .unwrap();
    assert_order_event(recv_execution_event(&mut rx).await, "Submitted");
    assert_order_event(recv_execution_event(&mut rx).await, "Submitted");

    wait_until_async(
        || {
            let gate = state.batch_order_request_gate.clone();
            async move { gate.started() == 1 }
        },
        Duration::from_secs(1),
    )
    .await;

    for order in &mut orders {
        submit_and_pending_cancel(&cache, order);
    }

    if !cancel_before_ack {
        state.batch_order_request_gate.release();
        assert_order_event(recv_execution_event(&mut rx).await, "Accepted");
        assert_order_event(recv_execution_event(&mut rx).await, "Accepted");
    }

    match mode {
        ShutdownCancelMode::CancelAll => client
            .cancel_all_orders(CancelAllOrders::new(
                TraderId::from("TESTER-001"),
                Some(*POLYMARKET_CLIENT_ID),
                StrategyId::from("S-001"),
                instrument_id,
                OrderSide::Buy,
                UUID4::new(),
                UnixNanos::default(),
                None,
                None,
            ))
            .unwrap(),
        ShutdownCancelMode::Batch => client
            .batch_cancel_orders(BatchCancelOrders::new(
                TraderId::from("TESTER-001"),
                Some(*POLYMARKET_CLIENT_ID),
                StrategyId::from("S-001"),
                instrument_id,
                orders
                    .iter()
                    .map(|order| make_cancel_cmd(order.client_order_id().as_str(), instrument_id))
                    .collect(),
                UUID4::new(),
                UnixNanos::default(),
                None,
                None,
            ))
            .unwrap(),
        ShutdownCancelMode::Individual => unreachable!(),
    }

    if cancel_before_ack {
        state.batch_order_request_gate.release();
        assert_order_event(recv_execution_event(&mut rx).await, "Accepted");
        assert_order_event(recv_execution_event(&mut rx).await, "Accepted");
    }

    wait_until_async(
        || {
            let state = state.clone();
            async move {
                if cancel_before_ack {
                    *state.cancel_delete_count.lock().await == 2
                } else {
                    *state.batch_cancel_delete_count.lock().await == 1
                }
            }
        },
        Duration::from_secs(1),
    )
    .await;

    assert!(state.open_order_ids.lock().await.is_empty());
}

#[rstest]
#[tokio::test]
async fn test_submit_cancel_stop_cancels_order_accepted_after_stop() {
    let state = TestServerState::default();
    *state.order_response.lock().await = Some(json!({
        "success": true,
        "orderID": DEFAULT_ACCEPTED_ORDER_ID,
        "errorMsg": null
    }));
    *state.cancel_response.lock().await = Some(json!({
        "canceled": [DEFAULT_ACCEPTED_ORDER_ID],
        "not_canceled": {}
    }));
    state.order_request_gate.enable();

    let addr = start_mock_server(state.clone()).await;
    let (mut client, mut rx, cache) = create_test_execution_client(addr);
    client.start().unwrap();

    let instrument_id = InstrumentId::from("TEST-TOKEN.POLYMARKET");
    add_instrument_to_cache(&cache, instrument_id);
    let mut order = make_limit_order(
        "O-SUBMIT-CANCEL-STOP",
        instrument_id,
        OrderSide::Buy,
        false,
        false,
        true,
        TimeInForce::Gtc,
    );
    cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .unwrap();
    submit_and_pending_cancel(&cache, &mut order);

    client
        .cancel_order(make_cancel_cmd("O-SUBMIT-CANCEL-STOP", instrument_id))
        .unwrap();
    client
        .submit_order(make_submit_cmd(&order, instrument_id))
        .unwrap();
    assert_order_event(recv_execution_event(&mut rx).await, "Submitted");

    wait_until_async(
        || {
            let gate = state.order_request_gate.clone();
            async move { gate.started() == 1 }
        },
        Duration::from_secs(1),
    )
    .await;

    client.stop().unwrap();
    state.order_request_gate.release();

    wait_until_async(
        || {
            let state = state.clone();
            async move { *state.cancel_delete_count.lock().await == 1 }
        },
        Duration::from_secs(1),
    )
    .await;

    assert!(state.open_order_ids.lock().await.is_empty());
}

#[rstest]
#[tokio::test]
async fn test_repeated_submit_cancel_shutdown_interleavings_leave_no_open_orders() {
    const ITERATIONS: usize = 16;

    let state = TestServerState::default();
    state.order_request_gate.enable();
    let addr = start_mock_server(state.clone()).await;
    let (mut client, mut rx, cache) = create_test_execution_client(addr);
    let instrument_id = InstrumentId::from("TEST-TOKEN.POLYMARKET");
    add_instrument_to_cache(&cache, instrument_id);
    add_test_account_to_cache(&cache, AccountId::from("POLYMARKET-001"));

    for index in 0..ITERATIONS {
        let client_order_id = format!("O-SHUTDOWN-STRESS-{index}");
        let venue_order_id = format!("0xvenue-shutdown-stress-{index}");
        *state.order_response.lock().await = Some(json!({
            "success": true,
            "orderID": venue_order_id,
            "errorMsg": null
        }));
        *state.cancel_response.lock().await = Some(json!({
            "canceled": [venue_order_id],
            "not_canceled": {}
        }));

        client.start().unwrap();
        if index % 2 == 1 {
            client.connect().await.unwrap();

            while rx.try_recv().is_ok() {}
        }

        let mut order = make_limit_order(
            &client_order_id,
            instrument_id,
            OrderSide::Buy,
            false,
            false,
            true,
            TimeInForce::Gtc,
        );
        cache
            .borrow_mut()
            .add_order(order.clone(), None, None, false)
            .unwrap();
        submit_and_pending_cancel(&cache, &mut order);

        client
            .cancel_order(make_cancel_cmd(&client_order_id, instrument_id))
            .unwrap();
        client
            .submit_order(make_submit_cmd(&order, instrument_id))
            .unwrap();
        assert_order_event(recv_execution_event(&mut rx).await, "Submitted");

        wait_until_async(
            || {
                let gate = state.order_request_gate.clone();
                async move { gate.started() == index + 1 }
            },
            Duration::from_secs(1),
        )
        .await;

        if index % 2 == 0 {
            client.stop().unwrap();
            state.order_request_gate.release();
        } else {
            let mut disconnect = Box::pin(client.disconnect());
            assert!(
                tokio::time::timeout(Duration::from_millis(20), disconnect.as_mut())
                    .await
                    .is_err(),
                "disconnect should wait for the held submit and its deferred cancel"
            );
            state.order_request_gate.release();
            disconnect.await.unwrap();
            client.stop().unwrap();
        }

        wait_until_async(
            || {
                let state = state.clone();
                async move { *state.cancel_delete_count.lock().await == index + 1 }
            },
            Duration::from_secs(1),
        )
        .await;
        assert_order_event(recv_execution_event(&mut rx).await, "Accepted");
        assert!(state.open_order_ids.lock().await.is_empty());
    }
}

#[rstest]
#[case(ShutdownAction::Stop)]
#[case(ShutdownAction::Disconnect)]
#[tokio::test]
async fn test_batch_submit_cancel_shutdown_cancels_orders_accepted_during_shutdown(
    #[case] action: ShutdownAction,
) {
    let state = TestServerState::default();
    let venue_order_ids = ["0xvenue-batch-stop-1", "0xvenue-batch-stop-2"];
    *state.batch_order_response.lock().await = Some(json!([
        {
            "success": true,
            "orderID": venue_order_ids[0],
            "errorMsg": null
        },
        {
            "success": true,
            "orderID": venue_order_ids[1],
            "errorMsg": null
        }
    ]));
    *state.cancel_response.lock().await = Some(json!({
        "canceled": venue_order_ids,
        "not_canceled": {}
    }));
    state.batch_order_request_gate.enable();

    let addr = start_mock_server(state.clone()).await;
    let (mut client, mut rx, cache) = create_test_execution_client(addr);
    add_test_account_to_cache(&cache, AccountId::from("POLYMARKET-001"));
    client.start().unwrap();
    if matches!(action, ShutdownAction::Disconnect) {
        client.connect().await.unwrap();

        while rx.try_recv().is_ok() {}
    }

    let instrument_id = InstrumentId::from("TEST-TOKEN.POLYMARKET");
    add_instrument_to_cache(&cache, instrument_id);
    let mut orders: Vec<OrderAny> = (0..2)
        .map(|index| {
            make_limit_order(
                &format!("O-BATCH-SUBMIT-CANCEL-STOP-{index}"),
                instrument_id,
                OrderSide::Buy,
                false,
                false,
                true,
                TimeInForce::Gtc,
            )
        })
        .collect();

    for order in &mut orders {
        cache
            .borrow_mut()
            .add_order(order.clone(), None, None, false)
            .unwrap();
        submit_and_pending_cancel(&cache, order);
        client
            .cancel_order(make_cancel_cmd(
                order.client_order_id().as_str(),
                instrument_id,
            ))
            .unwrap();
    }

    client
        .submit_order_list(make_submit_order_list_cmd(instrument_id, &orders))
        .unwrap();
    assert_order_event(recv_execution_event(&mut rx).await, "Submitted");
    assert_order_event(recv_execution_event(&mut rx).await, "Submitted");

    wait_until_async(
        || {
            let gate = state.batch_order_request_gate.clone();
            async move { gate.started() == 1 }
        },
        Duration::from_secs(1),
    )
    .await;

    match action {
        ShutdownAction::Stop => {
            client.stop().unwrap();
            state.batch_order_request_gate.release();
        }
        ShutdownAction::Disconnect => {
            let mut disconnect = Box::pin(client.disconnect());
            assert!(
                tokio::time::timeout(Duration::from_millis(50), disconnect.as_mut())
                    .await
                    .is_err(),
                "disconnect should wait for the held batch submit and deferred cancels"
            );
            state.batch_order_request_gate.release();
            disconnect.await.unwrap();
            assert_eq!(*state.cancel_delete_count.lock().await, 2);
            assert!(state.open_order_ids.lock().await.is_empty());
            client.stop().unwrap();
        }
    }

    wait_until_async(
        || {
            let state = state.clone();
            async move { *state.cancel_delete_count.lock().await == 2 }
        },
        Duration::from_secs(1),
    )
    .await;

    assert!(state.open_order_ids.lock().await.is_empty());
}

#[rstest]
#[tokio::test]
async fn test_cancel_order_deferred_with_already_done_response() {
    let state = TestServerState::default();
    // Mock server returns "already canceled or matched" for the cancel
    *state.order_response.lock().await = Some(json!({
        "success": true,
        "orderID": CANCEL_ALREADY_DONE_ORDER_ID,
        "errorMsg": null
    }));
    *state.cancel_response.lock().await = Some(load_json("http_cancel_response_failed.json"));
    let addr = start_mock_server(state.clone()).await;
    let (mut client, mut rx, cache) = create_test_execution_client(addr);
    client.start().unwrap();

    let instrument_id = InstrumentId::from("TEST-TOKEN.POLYMARKET");
    add_instrument_to_cache(&cache, instrument_id);

    let mut order = make_limit_order(
        "O-DEFERRED-DONE",
        instrument_id,
        OrderSide::Buy,
        false,
        false,
        false,
        TimeInForce::Gtc,
    );

    cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .unwrap();

    submit_and_pending_cancel(&cache, &mut order);

    let cmd = make_cancel_cmd("O-DEFERRED-DONE", instrument_id);
    client.cancel_order(cmd).unwrap();

    let submit_cmd = make_submit_cmd(&order, instrument_id);
    client.submit_order(submit_cmd).unwrap();

    // Submitted
    let event = rx.try_recv().unwrap();
    assert_order_event(event, "Submitted");

    // Accepted
    let event = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .unwrap()
        .unwrap();
    assert_order_event(event, "Accepted");

    wait_until_async(
        || {
            let state = state.clone();
            async move { *state.cancel_delete_count.lock().await == 1 }
        },
        Duration::from_secs(5),
    )
    .await;
    assert_no_execution_event(&mut rx).await;
}

#[rstest]
#[tokio::test]
async fn test_cancel_order_deferred_ambiguous_http_failure_does_not_emit_cancel_rejected() {
    let state = TestServerState::default();
    *state.order_response.lock().await = Some(json!({
        "success": true,
        "orderID": "0xvenue-deferred-ambiguous",
        "errorMsg": null
    }));
    *state.cancel_response_status.lock().await = StatusCode::INTERNAL_SERVER_ERROR;
    *state.cancel_response.lock().await = Some(json!({"error": "deferred cancel failed"}));
    let addr = start_mock_server(state.clone()).await;
    let (mut client, mut rx, cache) = create_test_execution_client_with_retries(addr, 2);
    client.start().unwrap();

    let instrument_id = InstrumentId::from("TEST-TOKEN.POLYMARKET");
    add_instrument_to_cache(&cache, instrument_id);

    let mut order = make_limit_order(
        "O-DEFERRED-AMBIGUOUS",
        instrument_id,
        OrderSide::Buy,
        false,
        false,
        false,
        TimeInForce::Gtc,
    );

    cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .unwrap();

    submit_and_pending_cancel(&cache, &mut order);

    let cmd = make_cancel_cmd("O-DEFERRED-AMBIGUOUS", instrument_id);
    client.cancel_order(cmd).unwrap();

    let submit_cmd = make_submit_cmd(&order, instrument_id);
    client.submit_order(submit_cmd).unwrap();

    let event = rx.try_recv().unwrap();
    assert_order_event(event, "Submitted");

    let event = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .unwrap()
        .unwrap();
    assert_order_event(event, "Accepted");

    wait_until_async(
        || {
            let state = state.clone();
            async move { *state.cancel_delete_count.lock().await == 3 }
        },
        Duration::from_secs(5),
    )
    .await;
    assert_no_execution_event(&mut rx).await;
}

#[rstest]
#[tokio::test]
async fn test_cancel_order_deferred_explicit_structured_rejection_emits_cancel_rejected() {
    let state = TestServerState::default();
    // Mock server returns an unexpected cancel failure
    *state.order_response.lock().await = Some(json!({
        "success": true,
        "orderID": "0xvenue-deferred-reject",
        "errorMsg": null
    }));
    *state.cancel_response.lock().await = Some(json!({
        "canceled": [],
        "not_canceled": {
            "0xvenue-deferred-reject": "order not found"
        }
    }));
    let addr = start_mock_server(state).await;
    let (mut client, mut rx, cache) = create_test_execution_client(addr);
    client.start().unwrap();

    let instrument_id = InstrumentId::from("TEST-TOKEN.POLYMARKET");
    add_instrument_to_cache(&cache, instrument_id);

    let mut order = make_limit_order(
        "O-DEFERRED-REJECT",
        instrument_id,
        OrderSide::Buy,
        false,
        false,
        false,
        TimeInForce::Gtc,
    );

    cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .unwrap();

    submit_and_pending_cancel(&cache, &mut order);

    let cmd = make_cancel_cmd("O-DEFERRED-REJECT", instrument_id);
    client.cancel_order(cmd).unwrap();

    let submit_cmd = make_submit_cmd(&order, instrument_id);
    client.submit_order(submit_cmd).unwrap();

    // Submitted
    let event = rx.try_recv().unwrap();
    assert_order_event(event, "Submitted");

    // Accepted
    let event = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .unwrap()
        .unwrap();
    assert_order_event(event, "Accepted");

    // Deferred cancel gets "order not found" which emits CancelRejected
    let event = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .unwrap()
        .unwrap();
    assert_order_event(event, "CancelRejected");
}

#[rstest]
#[tokio::test]
async fn test_cancel_order_uses_cache_index_fallback() {
    // Simulates the window where _post_signed_order completed (venue_order_id
    // cached in the index) but OrderAccepted has not yet been applied to the
    // order object. cancel_order should find the ID via the cache index and
    // proceed with the cancel directly, bypassing the deferred mechanism.
    let state = TestServerState::default();
    let addr = start_mock_server(state).await;
    let (mut client, mut rx, cache) = create_test_execution_client(addr);
    client.start().unwrap();

    let instrument_id = InstrumentId::from("TEST-TOKEN.POLYMARKET");

    let mut order = make_limit_order(
        "O-CACHE-FALLBACK",
        instrument_id,
        OrderSide::Buy,
        false,
        false,
        false,
        TimeInForce::Gtc,
    );

    cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .unwrap();

    // Transition to PENDING_CANCEL (no venue_order_id on the order object)
    submit_and_pending_cancel(&cache, &mut order);

    // Add venue_order_id to the cache INDEX only, simulating what
    // handle_order_response does via emit_order_accepted -> cache update.
    // The order object itself still has venue_order_id = None.
    let vid = VenueOrderId::from("0xvenue-cache-fallback");
    cache
        .borrow_mut()
        .add_venue_order_id(&ClientOrderId::from("O-CACHE-FALLBACK"), &vid, false)
        .unwrap();

    // cancel_order should find the venue_order_id in the cache index
    // and send the cancel HTTP request directly (no deferred mechanism)
    let cmd = make_cancel_cmd("O-CACHE-FALLBACK", instrument_id);
    client.cancel_order(cmd).unwrap();

    // A successful cancel via the mock server produces no rejection event
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(rx.try_recv().is_err());
}

#[rstest]
#[tokio::test]
async fn test_cancel_order_cache_fallback_with_rejection() {
    // Same cache index fallback path, but the venue returns an error so we
    // can verify a CancelRejected event is emitted.
    let state = TestServerState::default();
    *state.cancel_response.lock().await = Some(json!({
        "canceled": [],
        "not_canceled": {
            "0xvenue-cache-reject": "order not found"
        }
    }));
    let addr = start_mock_server(state).await;
    let (mut client, mut rx, cache) = create_test_execution_client(addr);
    client.start().unwrap();

    let instrument_id = InstrumentId::from("TEST-TOKEN.POLYMARKET");

    let mut order = make_limit_order(
        "O-CACHE-REJECT",
        instrument_id,
        OrderSide::Buy,
        false,
        false,
        false,
        TimeInForce::Gtc,
    );

    cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .unwrap();

    submit_and_pending_cancel(&cache, &mut order);

    let vid = VenueOrderId::from("0xvenue-cache-reject");
    cache
        .borrow_mut()
        .add_venue_order_id(&ClientOrderId::from("O-CACHE-REJECT"), &vid, false)
        .unwrap();

    let cmd = make_cancel_cmd("O-CACHE-REJECT", instrument_id);
    client.cancel_order(cmd).unwrap();

    // The cancel hit the venue, received "order not found", emits CancelRejected
    let event = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .unwrap()
        .unwrap();
    assert_order_event(event, "CancelRejected");
}

#[rstest]
#[tokio::test]
async fn test_query_order_does_not_block_within_runtime() {
    let state = TestServerState::default();
    let addr = start_mock_server(state).await;
    let (mut client, mut rx, cache) = create_test_execution_client(addr);
    client.start().unwrap();

    let instrument_id = InstrumentId::from("TEST-TOKEN.POLYMARKET");
    add_instrument_to_cache(&cache, instrument_id);

    let cmd = QueryOrder::new(
        TraderId::from("TESTER-001"),
        Some(*POLYMARKET_CLIENT_ID),
        StrategyId::from("S-001"),
        instrument_id,
        ClientOrderId::from("O-QUERY-001"),
        Some(VenueOrderId::from(
            "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef12",
        )),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None, // correlation_id
    );

    // This must not panic with "Cannot start a runtime from within a runtime"
    client.query_order(cmd).unwrap();

    let event = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .unwrap()
        .unwrap();
    assert_order_status_report(event, OrderStatus::Accepted);
}

#[rstest]
#[tokio::test]
async fn test_query_order_emits_confirmed_fill_before_status_report() {
    let venue_order_id_str = "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef12";
    let state = TestServerState::default();
    let mut order = load_json("http_open_order.json");
    order["id"] = Value::String(venue_order_id_str.to_string());
    order["status"] = Value::String("MATCHED".to_string());
    order["original_size"] = Value::String("10.0000".to_string());
    order["size_matched"] = Value::String("10.0000".to_string());
    *state.single_order_response.lock().await = Some(order);
    *state.trades_response_override.lock().await = Some(recovery_trades_response(
        venue_order_id_str,
        "10.0000",
        "0.5000",
    ));
    let addr = start_mock_server(state).await;
    let (mut client, mut rx, cache) = create_test_execution_client(addr);
    client.start().unwrap();

    let instrument_id = InstrumentId::from("TEST-TOKEN.POLYMARKET");
    add_instrument_to_cache_with_size_precision(&cache, instrument_id, 4);
    let instrument = cache.borrow().instrument(&instrument_id).unwrap().clone();
    client.on_instrument(instrument);
    let cmd = QueryOrder::new(
        TraderId::from("TESTER-001"),
        Some(*POLYMARKET_CLIENT_ID),
        StrategyId::from("S-001"),
        instrument_id,
        ClientOrderId::from("O-QUERY-CONFIRMED"),
        Some(VenueOrderId::from(venue_order_id_str)),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );

    client.query_order(cmd).unwrap();

    assert_recovery_fill_report(
        recv_execution_event(&mut rx).await,
        venue_order_id_str,
        "10.0000",
        "0.5000",
    );
    assert_order_status_report(recv_execution_event(&mut rx).await, OrderStatus::Filled);
}

#[rstest]
#[tokio::test]
async fn test_query_order_excludes_unconfirmed_matched_quantity() {
    let state = TestServerState::default();
    *state.single_order_response.lock().await = Some(json!({
        "associate_trades": ["pending-trade"],
        "id": "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef12",
        "status": "MATCHED",
        "market": "0xtest",
        "original_size": "10.0000",
        "outcome": "Yes",
        "maker_address": "0xtest",
        "owner": "test-owner",
        "price": "0.5100",
        "side": "BUY",
        "size_matched": "10.0000",
        "asset_id": "TEST-TOKEN",
        "expiration": null,
        "order_type": "GTC",
        "created_at": 1_703_875_200_i64
    }));
    let addr = start_mock_server(state).await;
    let (mut client, mut rx, cache) = create_test_execution_client(addr);
    client.start().unwrap();

    let instrument_id = InstrumentId::from("TEST-TOKEN.POLYMARKET");
    add_instrument_to_cache_with_size_precision(&cache, instrument_id, 4);
    let cmd = QueryOrder::new(
        TraderId::from("TESTER-001"),
        Some(*POLYMARKET_CLIENT_ID),
        StrategyId::from("S-001"),
        instrument_id,
        ClientOrderId::from("O-QUERY-PENDING"),
        Some(VenueOrderId::from(
            "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef12",
        )),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );

    client.query_order(cmd).unwrap();

    let event = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .unwrap()
        .unwrap();

    match event {
        ExecutionEvent::Report(ExecutionReport::Order(report)) => {
            assert_eq!(report.order_status, OrderStatus::Filled);
            assert_eq!(report.filled_qty, Quantity::zero(4));
        }
        other => panic!("Expected Order report, was {other:?}"),
    }
}

#[rstest]
#[tokio::test]
async fn test_query_account_does_not_block_within_runtime() {
    let state = TestServerState::default();
    let addr = start_mock_server(state).await;
    let (mut client, mut rx, _cache) = create_test_execution_client(addr);
    client.start().unwrap();

    let cmd = QueryAccount::new(
        TraderId::from("TESTER-001"),
        Some(*POLYMARKET_CLIENT_ID),
        AccountId::from("POLYMARKET-001"),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None, // correlation_id
    );

    // This must not panic with "Cannot start a runtime from within a runtime"
    client.query_account(cmd).unwrap();

    let event = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .unwrap()
        .unwrap();
    assert!(
        matches!(event, ExecutionEvent::Account(_)),
        "Expected Account event, was {event:?}"
    );
}
