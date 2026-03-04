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
    cell::RefCell, collections::HashMap, net::SocketAddr, path::PathBuf, rc::Rc, sync::Arc,
    time::Duration,
};

use axum::{
    Router,
    body::Bytes,
    extract::State,
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
        ExecutionEvent,
        execution::{
            BatchCancelOrders, CancelOrder, GenerateFillReports, GenerateOrderStatusReport,
            GenerateOrderStatusReports, GeneratePositionStatusReports, ModifyOrder, SubmitOrder,
        },
    },
    testing::wait_until_async,
};
use nautilus_core::{UUID4, UnixNanos};
use nautilus_live::ExecutionClientCore;
use nautilus_model::{
    accounts::{AccountAny, cash::CashAccount},
    enums::{AccountType, AssetClass, CurrencyType, OmsType, OrderSide, OrderType, TimeInForce},
    events::{AccountState, OrderEventAny},
    identifiers::{
        AccountId, ClientId, ClientOrderId, InstrumentId, StrategyId, Symbol, TraderId, Venue,
        VenueOrderId,
    },
    instruments::{BinaryOption, InstrumentAny},
    orders::{LimitOrder, MarketOrder, Order, OrderAny, stubs::TestOrderEventStubs},
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
    cancel_response: Arc<tokio::sync::Mutex<Option<Value>>>,
    batch_cancel_response: Arc<tokio::sync::Mutex<Option<Value>>>,
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
            cancel_response: Arc::new(tokio::sync::Mutex::new(None)),
            batch_cancel_response: Arc::new(tokio::sync::Mutex::new(None)),
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
    PolymarketExecClientConfig {
        private_key: Some(TEST_PRIVATE_KEY.to_string()),
        api_key: Some("test_api_key".to_string()),
        api_secret: Some(TEST_API_SECRET_B64.to_string()),
        passphrase: Some("test_pass".to_string()),
        funder: None,
        base_url_http: Some(format!("http://{addr}")),
        base_url_ws: Some(format!("ws://{addr}/ws")),
        base_url_gamma: Some(format!("http://{addr}")),
        http_timeout_secs: 5,
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
    Json(load_json("http_open_order.json")).into_response()
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

    if let Ok(v) = serde_json::from_slice::<Value>(&body) {
        *state.last_body.lock().await = Some(v);
    }

    let status = *state.order_response_status.lock().await;
    let resp = state.order_response.lock().await;
    let body = resp
        .clone()
        .unwrap_or_else(|| load_json("http_order_response_ok.json"));
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

async fn handle_health() -> impl IntoResponse {
    StatusCode::OK
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
        .route("/orders", delete(handle_delete_orders))
        .route("/cancel-all", delete(handle_cancel_all))
        .route("/markets", get(handle_gamma_markets))
        .route("/health", get(handle_health))
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

    let usdc = Currency::new("USDC", 6, 0, "USDC", CurrencyType::Crypto);
    let balances = vec![AccountBalance::new(
        Money::new(1000.0, usdc),
        Money::new(0.0, usdc),
        Money::new(1000.0, usdc),
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

    client.modify_order(&cmd).unwrap();

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
async fn test_submit_order_denied_for_market_order() {
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
        false, // quote_quantity
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

    client.submit_order(&cmd).unwrap();

    let event = rx.try_recv().unwrap();
    match event {
        ExecutionEvent::Order(order_event) => {
            assert!(
                matches!(order_event, OrderEventAny::Denied(_)),
                "Expected Denied for market order, was {order_event:?}"
            );
        }
        other => panic!("Expected Order event, was {other:?}"),
    }
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
    let raw_symbol = Symbol::from(
        "71321045679252212594626385532706912750332728571942532289631379312455583992563",
    );

    let instrument = BinaryOption::new(
        instrument_id,
        raw_symbol,
        AssetClass::Alternative,
        Currency::new("USDC", 6, 0, "USDC", CurrencyType::Crypto),
        UnixNanos::default(), // activation_ns
        UnixNanos::default(), // expiration_ns
        4,                    // price_precision
        0,                    // size_precision
        Price::from("0.0001"),
        Quantity::from("1"),
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

    client.submit_order(&cmd).unwrap();

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

    client.submit_order(&cmd).unwrap();

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

    client.submit_order(&cmd).unwrap();

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

    client.submit_order(&cmd).unwrap();

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

    client.submit_order(&cmd).unwrap();

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

    client.submit_order(&cmd).unwrap();

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

    client.submit_order(&cmd).unwrap();

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
    client.cancel_order(&cmd).unwrap();

    // No event should be emitted since the order is not open
    assert!(rx.try_recv().is_err());
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
    client.cancel_order(&cmd).unwrap();

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
    client.cancel_order(&cmd).unwrap();

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
    client.cancel_order(&cmd).unwrap();

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

    client.batch_cancel_orders(&cmd).unwrap();

    // Order 3 has CANCEL_ALREADY_DONE, so it should be suppressed.
    // No CancelRejected events expected.
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(rx.try_recv().is_err());
}
