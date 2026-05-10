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
    collections::{HashMap, HashSet},
    net::SocketAddr,
    path::PathBuf,
    rc::Rc,
    sync::Arc,
    time::Duration,
};

use axum::{
    Router,
    body::Bytes,
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Json, Response},
    routing::{delete, get, post},
};
use nautilus_common::{
    cache::Cache,
    clients::ExecutionClient,
    enums::LogLevel,
    live::runner::set_exec_event_sender,
    messages::{
        ExecutionEvent, ExecutionReport,
        execution::{
            BatchCancelOrders, CancelOrder, GenerateFillReports, GenerateOrderStatusReport,
            GenerateOrderStatusReports, GeneratePositionStatusReports, ModifyOrder, QueryAccount,
            QueryOrder, SubmitOrder, SubmitOrderList,
        },
    },
    testing::wait_until_async,
};
use nautilus_core::{UUID4, UnixNanos};
use nautilus_live::ExecutionClientCore;
use nautilus_model::{
    accounts::{AccountAny, cash::CashAccount},
    enums::{
        AccountType, AssetClass, OmsType, OrderSide, OrderStatus, OrderType, TimeInForce,
        TriggerType,
    },
    events::{AccountState, OrderEventAny, OrderPendingCancel},
    identifiers::{
        AccountId, ClientId, ClientOrderId, InstrumentId, OrderListId, StrategyId, Symbol,
        TraderId, Venue, VenueOrderId,
    },
    instruments::{BinaryOption, InstrumentAny},
    orders::{
        LimitOrder, MarketOrder, Order, OrderAny, OrderList, StopMarketOrder,
        stubs::TestOrderEventStubs,
    },
    types::{AccountBalance, Currency, Money, Price, Quantity},
};
use nautilus_network::http::HttpClient;
use nautilus_polymarket::{
    config::PolymarketExecClientConfig, execution::PolymarketExecutionClient,
};
use rstest::rstest;
use serde_json::{Value, json};

const TEST_PRIVATE_KEY: &str = "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef";
const TEST_API_SECRET_B64: &str = "dGVzdF9zZWNyZXRfa2V5XzMyYnl0ZXNfcGFkMTIzNDU=";
#[derive(Clone)]
struct TestServerState {
    last_body: Arc<tokio::sync::Mutex<Option<Value>>>,
    last_headers: Arc<tokio::sync::Mutex<HashMap<String, String>>>,
    last_path: Arc<tokio::sync::Mutex<String>>,
    gamma_response: Arc<tokio::sync::Mutex<Option<Value>>>,
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
    cancel_response: Arc<tokio::sync::Mutex<Option<Value>>>,
    batch_cancel_response: Arc<tokio::sync::Mutex<Option<Value>>>,
    book_response: Arc<tokio::sync::Mutex<Option<Value>>>,
    single_order_response: Arc<tokio::sync::Mutex<Option<Value>>>,
}

impl Default for TestServerState {
    fn default() -> Self {
        Self {
            last_body: Arc::new(tokio::sync::Mutex::new(None)),
            last_headers: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            last_path: Arc::new(tokio::sync::Mutex::new(String::new())),
            gamma_response: Arc::new(tokio::sync::Mutex::new(None)),
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
            cancel_response: Arc::new(tokio::sync::Mutex::new(None)),
            batch_cancel_response: Arc::new(tokio::sync::Mutex::new(None)),
            single_order_response: Arc::new(tokio::sync::Mutex::new(None)),
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
    let trader_id = TraderId::from("TESTER-001");
    let account_id = AccountId::from("POLYMARKET-001");
    let client_id = ClientId::from("POLYMARKET");

    let cache = Rc::new(RefCell::new(Cache::default()));

    let core = ExecutionClientCore::new(
        trader_id,
        client_id,
        Venue::from("POLYMARKET"),
        OmsType::Netting,
        account_id,
        AccountType::Cash,
        None,
        cache.clone(),
    );

    let config = create_test_exec_config(addr);

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
    let trader_id = TraderId::from("TESTER-001");
    let account_id = AccountId::from("POLYMARKET-001");
    let client_id = ClientId::from("POLYMARKET");

    let cache = Rc::new(RefCell::new(Cache::default()));

    let core = ExecutionClientCore::new(
        trader_id,
        client_id,
        Venue::from("POLYMARKET"),
        OmsType::Netting,
        account_id,
        AccountType::Cash,
        None,
        cache.clone(),
    );

    let config = create_test_exec_config_with_retries(addr, max_retries);

    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    set_exec_event_sender(tx);

    let client = PolymarketExecutionClient::new(core, config).unwrap();

    (client, rx, cache)
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

async fn handle_get_trades(State(state): State<TestServerState>) -> Response {
    *state.last_path.lock().await = "/data/trades".to_string();
    Json(load_json("http_trades_page.json")).into_response()
}

async fn handle_get_balance(State(state): State<TestServerState>) -> Response {
    *state.last_path.lock().await = "/balance-allowance".to_string();
    Json(load_json("http_balance_allowance_collateral.json")).into_response()
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
    *state.batch_order_post_count.lock().await += 1;

    let parsed = serde_json::from_slice::<Value>(&body).ok();
    let request_count = parsed
        .as_ref()
        .and_then(Value::as_array)
        .map_or(0, Vec::len);

    if let Some(v) = parsed {
        *state.last_body.lock().await = Some(v);
    }

    let status = *state.batch_order_response_status.lock().await;
    let resp = state.batch_order_response.lock().await;
    let body = resp.clone().unwrap_or_else(|| {
        let entries: Vec<Value> = (0..request_count.max(1))
            .map(|i| {
                json!({
                    "success": true,
                    "orderID": format!("0xauto-{i}"),
                    "errorMsg": ""
                })
            })
            .collect();
        Value::Array(entries)
    });
    (status, Json(body)).into_response()
}

async fn handle_delete_order(State(state): State<TestServerState>, body: Bytes) -> Response {
    *state.last_path.lock().await = "/order".to_string();

    if let Ok(v) = serde_json::from_slice::<Value>(&body) {
        *state.last_body.lock().await = Some(v);
    }

    let resp = state.cancel_response.lock().await;
    let body = resp
        .clone()
        .unwrap_or_else(|| load_json("http_cancel_response_ok.json"));
    Json(body).into_response()
}

async fn handle_delete_orders(State(state): State<TestServerState>, body: Bytes) -> Response {
    *state.last_path.lock().await = "/orders".to_string();

    if let Ok(v) = serde_json::from_slice::<Value>(&body) {
        *state.last_body.lock().await = Some(v);
    }

    let resp = state.batch_cancel_response.lock().await;
    let body = resp
        .clone()
        .unwrap_or_else(|| load_json("http_batch_cancel_response.json"));
    Json(body).into_response()
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
        .route("/health", get(handle_health))
        .route("/positions", get(handle_get_positions))
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

#[rstest]
#[tokio::test]
async fn test_exec_client_creation() {
    let state = TestServerState::default();
    let addr = start_mock_server(state).await;
    let (client, _rx, _cache) = create_test_execution_client(addr);

    assert_eq!(client.client_id(), ClientId::from("POLYMARKET"));
    assert_eq!(client.account_id(), AccountId::from("POLYMARKET-001"));
    assert_eq!(client.venue(), Venue::from("POLYMARKET"));
    assert_eq!(client.oms_type(), OmsType::Netting);
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
    };

    let reports = client.generate_order_status_reports(&cmd).await.unwrap();

    // Without loaded instruments, orders cannot be resolved to instrument IDs
    assert!(reports.is_empty());
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
    };

    let result = client.generate_order_status_report(&cmd).await.unwrap();

    let report = result.unwrap();
    assert_eq!(report.instrument_id, instrument_id);
    assert_eq!(report.account_id, AccountId::from("POLYMARKET-001"));
    assert_eq!(report.order_side, OrderSide::Buy,);
    assert_eq!(report.order_type, OrderType::Limit,);
    assert!(report.price.is_some());
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
        client_id: Some(ClientId::from("POLYMARKET")),
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
        false, // quote_quantity — BUY requires true
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
        Some(ClientId::from("POLYMARKET")),
        StrategyId::from("S-001"),
        instrument_id,
        client_order_id,
        init_event,
        None, // exec_algorithm_id
        None, // position_id
        None, // params
        UUID4::new(),
        UnixNanos::default(),
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
        true,  // quote_quantity — SELL requires false
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
        Some(ClientId::from("POLYMARKET")),
        StrategyId::from("S-001"),
        instrument_id,
        client_order_id,
        init_event,
        None,
        None,
        None,
        UUID4::new(),
        UnixNanos::default(),
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
    OrderAny::Market(MarketOrder::new(
        TraderId::from("TESTER-001"),
        StrategyId::from("S-001"),
        instrument_id,
        ClientOrderId::from(client_order_id),
        side,
        Quantity::new(10.0, 0),
        TimeInForce::Ioc,
        UUID4::new(),
        UnixNanos::default(),
        false, // reduce_only
        quote_quantity,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
    ))
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

#[rstest]
#[tokio::test]
async fn test_fok_deferred_check_emits_rejected_for_unmatched() {
    let state = TestServerState::default();
    // REST returns UNMATCHED for the FOK order status check
    *state.single_order_response.lock().await = Some(json!({
        "associate_trades": [],
        "id": "test-fok-order-id",
        "status": "UNMATCHED",
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

    let order = make_market_order("O-FOK-UNMATCHED", instrument_id, OrderSide::Buy, true);
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

    // Deferred FOK check: after ~5s, should emit a Rejected status report
    let event = tokio::time::timeout(Duration::from_secs(10), rx.recv())
        .await
        .unwrap()
        .unwrap();
    assert_order_status_report(event, OrderStatus::Rejected);
}

fn make_stop_market_order(
    client_order_id: &str,
    instrument_id: InstrumentId,
    side: OrderSide,
) -> OrderAny {
    OrderAny::StopMarket(StopMarketOrder::new(
        TraderId::from("TESTER-001"),
        StrategyId::from("S-001"),
        instrument_id,
        ClientOrderId::from(client_order_id),
        side,
        Quantity::new(10.0, 0),
        Price::new(0.50, 4),
        TriggerType::LastPrice,
        TimeInForce::Gtc,
        None,  // expire_time
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
    ))
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
    let expire_time = if time_in_force == TimeInForce::Gtd {
        Some(UnixNanos::from(2_000_000_000_000_000_000u64))
    } else {
        None
    };

    OrderAny::Limit(LimitOrder::new(
        TraderId::from("TESTER-001"),
        StrategyId::from("S-001"),
        instrument_id,
        ClientOrderId::from(client_order_id),
        side,
        Quantity::new(10.0, 0),
        Price::new(0.50, 4),
        time_in_force,
        expire_time,
        post_only,
        reduce_only,
        quote_quantity,
        None, // display_qty
        None, // emulation_trigger
        None, // trigger_instrument_id
        None, // contingency_type
        None, // order_list_id
        None, // linked_order_ids
        None, // parent_order_id
        None, // exec_algorithm_id
        None, // exec_algorithm_params
        None, // exec_spawn_id
        None, // tags
        UUID4::new(),
        UnixNanos::default(),
    ))
}

fn make_submit_cmd(order: &OrderAny, instrument_id: InstrumentId) -> SubmitOrder {
    SubmitOrder::new(
        TraderId::from("TESTER-001"),
        Some(ClientId::from("POLYMARKET")),
        StrategyId::from("S-001"),
        instrument_id,
        order.client_order_id(),
        order.init_event().clone(),
        None, // exec_algorithm_id
        None, // position_id
        None, // params
        UUID4::new(),
        UnixNanos::default(),
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
        Some(ClientId::from("POLYMARKET")),
        strategy_id,
        order_list,
        order_inits,
        None,
        None,
        None,
        UUID4::new(),
        UnixNanos::default(),
    )
}

fn make_cancel_cmd(client_order_id: &str, instrument_id: InstrumentId) -> CancelOrder {
    CancelOrder::new(
        TraderId::from("TESTER-001"),
        Some(ClientId::from("POLYMARKET")),
        StrategyId::from("S-001"),
        instrument_id,
        ClientOrderId::from(client_order_id),
        None, // venue_order_id
        UUID4::new(),
        UnixNanos::default(),
        None,
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
    let symbol = "71321045679252212594626385532706912750332728571942532289631379312455583992563";
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
        4,                    // price_precision
        size_precision,
        Price::from("0.0001"),
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
    order.apply(submitted).unwrap();
    cache.borrow_mut().update_order(order).unwrap();
    let accepted = TestOrderEventStubs::accepted(order, account_id, vid);
    order.apply(accepted).unwrap();
    cache.borrow_mut().update_order(order).unwrap();
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
async fn test_submit_order_rejected_on_http_error() {
    let state = TestServerState::default();
    *state.order_response_status.lock().await = StatusCode::INTERNAL_SERVER_ERROR;
    *state.order_response.lock().await = Some(load_json("http_order_response_error_500.json"));
    let addr = start_mock_server(state).await;
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

    // Rejected (async, HTTP error triggers rejection)
    let event = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .unwrap()
        .unwrap();
    assert_order_event(event, "Rejected");
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
async fn test_submit_order_rejects_when_5xx_exhausts_retries() {
    // Server returns 500 three times. With max_retries=2 the submitter
    // exhausts retries on the third attempt and emits Rejected.
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

    let event = tokio::time::timeout(Duration::from_secs(10), rx.recv())
        .await
        .expect("expected reject within timeout")
        .unwrap();
    assert_order_event(event, "Rejected");

    // Initial attempt + 2 retries = 3 POSTs, then give up.
    assert_eq!(*state.order_post_count.lock().await, 3);
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
#[tokio::test]
async fn test_submit_order_list_denies_invalid_orders_before_batch_post() {
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
    let invalid = make_limit_order(
        "O-LIST-INVALID",
        instrument_id,
        OrderSide::Sell,
        false,
        false,
        true,
        TimeInForce::Ioc,
    );
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
async fn test_submit_order_list_rejects_orders_missing_batch_responses() {
    let state = TestServerState::default();
    *state.batch_order_response.lock().await = Some(json!([
        {"success": true, "orderID": "0xbatch-order-1", "errorMsg": ""}
    ]));
    let addr = start_mock_server(state).await;
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

    assert_order_event(recv_execution_event(&mut rx).await, "Submitted");
    assert_order_event(recv_execution_event(&mut rx).await, "Submitted");
    assert_order_event(recv_execution_event(&mut rx).await, "Accepted");
    assert_order_event(recv_execution_event(&mut rx).await, "Rejected");
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
    assert_order_event(recv_execution_event(&mut rx).await, "Rejected");
    assert_order_event(recv_execution_event(&mut rx).await, "Rejected");

    assert_eq!(*state.batch_order_post_count.lock().await, 1);

    // Confirm no background retry fires after the rejections.
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(*state.batch_order_post_count.lock().await, 1);
    assert!(
        rx.try_recv().is_err(),
        "no further events expected after batch rejection"
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
                assert!(
                    reason.contains("Instrument not found"),
                    "reason was {reason}"
                );
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
async fn test_cancel_order_skips_non_open_order() {
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

    // CancelRejected is emitted synchronously for non-open orders
    let event = rx.try_recv().expect("Expected CancelRejected event");
    assert_order_event(event, "CancelRejected");
}

#[rstest]
#[tokio::test]
async fn test_cancel_order_success_no_rejection_event() {
    let state = TestServerState::default();
    let addr = start_mock_server(state).await;
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

    // Wait briefly and verify no rejection event is emitted for a successful cancel
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(rx.try_recv().is_err());
}

#[rstest]
#[tokio::test]
async fn test_cancel_order_already_done_suppresses_rejection() {
    let state = TestServerState::default();
    *state.cancel_response.lock().await = Some(load_json("http_cancel_response_failed.json"));
    let addr = start_mock_server(state).await;
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
    submit_and_accept_order(&cache, &mut order, "0xvenue-cancel-done");

    let cmd = make_cancel_cmd("O-CANCEL-DONE", instrument_id);
    client.cancel_order(cmd).unwrap();

    // CANCEL_ALREADY_DONE should suppress the rejection event
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(rx.try_recv().is_err());
}

#[rstest]
#[tokio::test]
async fn test_cancel_order_other_reason_emits_cancel_rejected() {
    let state = TestServerState::default();
    *state.cancel_response.lock().await = Some(json!({
        "not_canceled": "order not found"
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
        Some(ClientId::from("POLYMARKET")),
        StrategyId::from("S-001"),
        instrument_id,
        cancels,
        UUID4::new(),
        UnixNanos::default(),
        None,
    );

    client.batch_cancel_orders(cmd).unwrap();

    // Order 3 has CANCEL_ALREADY_DONE, so it should be suppressed.
    // No CancelRejected events expected.
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(rx.try_recv().is_err());
}

fn submit_and_pending_cancel(cache: &Rc<RefCell<Cache>>, order: &mut OrderAny) {
    let account_id = AccountId::from("POLYMARKET-001");
    let submitted = TestOrderEventStubs::submitted(order, account_id);
    order.apply(submitted).unwrap();
    cache.borrow_mut().update_order(order).unwrap();

    let pending_cancel = OrderPendingCancel::new(
        order.trader_id(),
        order.strategy_id(),
        order.instrument_id(),
        order.client_order_id(),
        account_id,
        UUID4::new(),
        UnixNanos::default(),
        UnixNanos::default(),
        false,
        None, // No venue_order_id yet
    );
    order
        .apply(OrderEventAny::PendingCancel(pending_cancel))
        .unwrap();
    cache.borrow_mut().update_order(order).unwrap();
}

#[rstest]
#[tokio::test]
async fn test_cancel_order_deferred_when_no_venue_order_id() {
    let state = TestServerState::default();
    let addr = start_mock_server(state).await;
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

    // Deferred cancel fires against the mock server (returns success).
    // A successful cancel produces no rejection event.
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(rx.try_recv().is_err());
}

#[rstest]
#[tokio::test]
async fn test_cancel_order_deferred_with_already_done_response() {
    let state = TestServerState::default();
    // Mock server returns "already canceled or matched" for the cancel
    *state.cancel_response.lock().await = Some(load_json("http_cancel_response_failed.json"));
    let addr = start_mock_server(state).await;
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

    // Deferred cancel gets "already done" response, which is suppressed
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(rx.try_recv().is_err());
}

#[rstest]
#[tokio::test]
async fn test_cancel_order_deferred_with_rejection_response() {
    let state = TestServerState::default();
    // Mock server returns an unexpected cancel failure
    *state.cancel_response.lock().await = Some(json!({
        "not_canceled": "order not found"
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
        "not_canceled": "order not found"
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
        Some(ClientId::from("POLYMARKET")),
        StrategyId::from("S-001"),
        instrument_id,
        ClientOrderId::from("O-QUERY-001"),
        Some(VenueOrderId::from(
            "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef12",
        )),
        UUID4::new(),
        UnixNanos::default(),
        None,
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
async fn test_query_account_does_not_block_within_runtime() {
    let state = TestServerState::default();
    let addr = start_mock_server(state).await;
    let (mut client, mut rx, _cache) = create_test_execution_client(addr);
    client.start().unwrap();

    let cmd = QueryAccount::new(
        TraderId::from("TESTER-001"),
        Some(ClientId::from("POLYMARKET")),
        AccountId::from("POLYMARKET-001"),
        UUID4::new(),
        UnixNanos::default(),
        None,
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
