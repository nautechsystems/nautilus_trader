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

//! Integration tests for the Coinbase execution-side HTTP reconciliation path.
//!
//! Uses an axum mock server to feed canned responses and capture the outgoing
//! query strings so pagination, URL encoding, and filter-key regressions stay
//! locked in. The mock server accepts any Bearer JWT (no verification), so
//! tests pass bogus credentials that still sign with a valid EC key pair.

use std::{
    collections::{HashMap, HashSet, VecDeque},
    net::SocketAddr,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};

use aws_lc_rs::{
    encoding::AsDer,
    rand as lc_rand,
    signature::{ECDSA_P256_SHA256_FIXED_SIGNING, EcdsaKeyPair},
};
use axum::{
    Router,
    extract::State,
    http::Uri,
    response::{IntoResponse, Json},
    routing::{get, post},
};
use chrono::{TimeZone, Utc};
use nautilus_coinbase::{
    common::{consts::COINBASE_VENUE, enums::CoinbaseEnvironment},
    config::CoinbaseExecClientConfig,
    execution::CoinbaseExecutionClient,
    http::client::CoinbaseHttpClient,
};
use nautilus_common::{
    cache::Cache,
    clients::ExecutionClient,
    live::runner::replace_exec_event_sender,
    messages::{
        ExecutionEvent,
        execution::{GeneratePositionStatusReports, GeneratePositionStatusReportsBuilder},
    },
};
use nautilus_core::UnixNanos;
use nautilus_live::ExecutionClientCore;
use nautilus_model::{
    enums::{AccountType, OmsType, OrderSide, OrderType, PositionSideSpecified, TimeInForce},
    identifiers::{AccountId, ClientId, ClientOrderId, InstrumentId, TraderId, VenueOrderId},
    instruments::InstrumentAny,
    types::{Price, Quantity},
};
use nautilus_network::retry::RetryConfig;
use rstest::rstest;
use rust_decimal_macros::dec;
use serde_json::{Value, json};

fn data_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test_data")
}

fn load_json(filename: &str) -> Value {
    let content = std::fs::read_to_string(data_path().join(filename))
        .unwrap_or_else(|_| panic!("failed to read {filename}"));
    serde_json::from_str(&content).expect("invalid json")
}

// Generates an ES256 EC private key in SEC1 PEM format. Coinbase's credential
// loader accepts both SEC1 and PKCS#8; SEC1 matches production.
fn test_pem_key() -> String {
    let rng = lc_rand::SystemRandom::new();
    let pkcs8 = EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, &rng).unwrap();
    let key_pair =
        EcdsaKeyPair::from_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, pkcs8.as_ref()).unwrap();
    let sec1_der = key_pair.private_key().as_der().unwrap();
    let pem_obj = pem::Pem::new("EC PRIVATE KEY", sec1_der.as_ref().to_vec());
    pem::encode(&pem_obj)
}

fn test_api_key() -> String {
    "organizations/test-org/apiKeys/test-key".to_string()
}

#[derive(Debug, Clone)]
struct RequestRecord {
    path: String,
    raw_query: String,
    body: Option<Value>,
}

#[derive(Default)]
struct TestStateInner {
    requests: Vec<RequestRecord>,
    queues: HashMap<String, VecDeque<Value>>,
    // Paths listed here respond with HTTP 503 so retry-guard tests can
    // observe whether the client issues a second attempt.
    fail_paths: HashSet<String>,
}

#[derive(Clone, Default)]
struct TestState {
    inner: Arc<Mutex<TestStateInner>>,
}

impl TestState {
    fn enqueue(&self, path: &str, response: Value) {
        self.inner
            .lock()
            .unwrap()
            .queues
            .entry(path.to_string())
            .or_default()
            .push_back(response);
    }

    fn next_response(&self, path: &str, raw_query: String) -> Value {
        let mut state = self.inner.lock().unwrap();
        state.requests.push(RequestRecord {
            path: path.to_string(),
            raw_query,
            body: None,
        });
        state
            .queues
            .get_mut(path)
            .and_then(|q| q.pop_front())
            .unwrap_or_else(|| json!({}))
    }

    fn next_response_with_body(&self, path: &str, body: Value) -> Value {
        let mut state = self.inner.lock().unwrap();
        state.requests.push(RequestRecord {
            path: path.to_string(),
            raw_query: String::new(),
            body: Some(body),
        });
        state
            .queues
            .get_mut(path)
            .and_then(|q| q.pop_front())
            .unwrap_or_else(|| json!({}))
    }

    fn requests(&self) -> Vec<RequestRecord> {
        self.inner.lock().unwrap().requests.clone()
    }

    fn mark_failing(&self, path: &str) {
        self.inner
            .lock()
            .unwrap()
            .fail_paths
            .insert(path.to_string());
    }

    fn is_failing(&self, path: &str) -> bool {
        self.inner.lock().unwrap().fail_paths.contains(path)
    }

    fn record_failure(&self, path: &str, raw_query: String, body: Option<Value>) {
        self.inner.lock().unwrap().requests.push(RequestRecord {
            path: path.to_string(),
            raw_query,
            body,
        });
    }

    fn requests_for(&self, path: &str) -> Vec<RequestRecord> {
        self.inner
            .lock()
            .unwrap()
            .requests
            .iter()
            .filter(|r| r.path == path)
            .cloned()
            .collect()
    }
}

async fn handle_orders_batch(State(state): State<TestState>, uri: Uri) -> impl IntoResponse {
    let raw_query = uri.query().unwrap_or("").to_string();
    let response = state.next_response("/orders/historical/batch", raw_query);
    Json(response)
}

async fn handle_order_by_id(
    State(state): State<TestState>,
    axum::extract::Path(order_id): axum::extract::Path<String>,
    uri: Uri,
) -> impl IntoResponse {
    let raw_query = uri.query().unwrap_or("").to_string();
    let path = format!("/orders/historical/{order_id}");
    let response = state.next_response(&path, raw_query);
    Json(response)
}

async fn handle_fills(State(state): State<TestState>, uri: Uri) -> impl IntoResponse {
    let raw_query = uri.query().unwrap_or("").to_string();
    let response = state.next_response("/orders/historical/fills", raw_query);
    Json(response)
}

async fn handle_accounts(State(state): State<TestState>, uri: Uri) -> impl IntoResponse {
    let raw_query = uri.query().unwrap_or("").to_string();
    let response = state.next_response("/accounts", raw_query);
    Json(response)
}

async fn handle_products(State(state): State<TestState>) -> axum::response::Response {
    if state.is_failing("/market/products") {
        state.record_failure("/market/products", String::new(), None);
        return (
            axum::http::StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": "unavailable"})),
        )
            .into_response();
    }

    // If the test enqueued a custom products response use that; otherwise
    // fall back to the shared spot fixture so instrument resolution works
    // out of the box.
    let inner = state.inner.clone();
    let have_queue = inner
        .lock()
        .unwrap()
        .queues
        .get("/market/products")
        .is_some_and(|q| !q.is_empty());

    if have_queue {
        let response = state.next_response("/market/products", String::new());
        Json(response).into_response()
    } else {
        state.next_response("/market/products", String::new());
        Json(load_json("http_products.json")).into_response()
    }
}

async fn handle_product(
    State(state): State<TestState>,
    axum::extract::Path(product_id): axum::extract::Path<String>,
) -> impl IntoResponse {
    state.next_response(&format!("/market/products/{product_id}"), String::new());

    if product_id == "BTC-USD" {
        Json(load_json("http_product.json"))
    } else if product_id == "BIP-20DEC30-CDE" {
        // Return the first (perpetual) entry from the futures fixture.
        let payload = load_json("http_products_future.json");
        let product = payload["products"][0].clone();
        Json(product)
    } else {
        Json(json!({"error": "not found"}))
    }
}

async fn handle_cfm_balance_summary(State(state): State<TestState>) -> impl IntoResponse {
    state.next_response("/cfm/balance_summary", String::new());
    Json(load_json("http_cfm_balance_summary.json"))
}

async fn handle_cfm_positions(State(state): State<TestState>) -> axum::response::Response {
    if state.is_failing("/cfm/positions") {
        state.record_failure("/cfm/positions", String::new(), None);
        return (
            axum::http::StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": "unavailable"})),
        )
            .into_response();
    }
    state.next_response("/cfm/positions", String::new());
    Json(load_json("http_cfm_positions.json")).into_response()
}

async fn handle_cfm_position(
    State(state): State<TestState>,
    axum::extract::Path(product_id): axum::extract::Path<String>,
) -> axum::response::Response {
    let path = format!("/cfm/positions/{product_id}");
    if state.is_failing(&path) {
        state.record_failure(&path, String::new(), None);
        return (
            axum::http::StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": "unavailable"})),
        )
            .into_response();
    }
    state.next_response(&path, String::new());
    Json(load_json("http_cfm_position.json")).into_response()
}

async fn handle_create_order(
    State(state): State<TestState>,
    Json(body): Json<Value>,
) -> axum::response::Response {
    if state.is_failing("/orders") {
        state.record_failure("/orders", String::new(), Some(body));
        return (
            axum::http::StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": "unavailable"})),
        )
            .into_response();
    }

    let response = state.next_response_with_body("/orders", body);
    Json(response).into_response()
}

async fn handle_cancel_orders(
    State(state): State<TestState>,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    let response = state.next_response_with_body("/orders/batch_cancel", body);
    Json(response)
}

async fn handle_edit_order(
    State(state): State<TestState>,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    let response = state.next_response_with_body("/orders/edit", body);
    Json(response)
}

const API_PREFIX: &str = "/api/v3/brokerage";

fn create_router(state: TestState) -> Router {
    Router::new()
        .route(
            &format!("{API_PREFIX}/orders/historical/batch"),
            get(handle_orders_batch),
        )
        .route(
            &format!("{API_PREFIX}/orders/historical/fills"),
            get(handle_fills),
        )
        .route(
            &format!("{API_PREFIX}/orders/historical/{{order_id}}"),
            get(handle_order_by_id),
        )
        .route(&format!("{API_PREFIX}/accounts"), get(handle_accounts))
        .route(
            &format!("{API_PREFIX}/market/products"),
            get(handle_products),
        )
        .route(
            &format!("{API_PREFIX}/market/products/{{product_id}}"),
            get(handle_product),
        )
        .route(
            &format!("{API_PREFIX}/cfm/balance_summary"),
            get(handle_cfm_balance_summary),
        )
        .route(
            &format!("{API_PREFIX}/cfm/positions"),
            get(handle_cfm_positions),
        )
        .route(
            &format!("{API_PREFIX}/cfm/positions/{{product_id}}"),
            get(handle_cfm_position),
        )
        .route(&format!("{API_PREFIX}/orders"), post(handle_create_order))
        .route(
            &format!("{API_PREFIX}/orders/batch_cancel"),
            post(handle_cancel_orders),
        )
        .route(
            &format!("{API_PREFIX}/orders/edit"),
            post(handle_edit_order),
        )
        .with_state(state)
}

async fn start_mock_server(state: TestState) -> SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let router = create_router(state);

    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });

    let start = std::time::Instant::now();

    loop {
        if tokio::net::TcpStream::connect(addr).await.is_ok() {
            break;
        }

        assert!(
            start.elapsed() <= Duration::from_secs(5),
            "Mock server did not start within timeout"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    addr
}

fn create_http_client(addr: SocketAddr) -> CoinbaseHttpClient {
    create_http_client_with_retry(addr, None)
}

fn create_http_client_with_retry(
    addr: SocketAddr,
    retry_config: Option<RetryConfig>,
) -> CoinbaseHttpClient {
    let client = CoinbaseHttpClient::from_credentials(
        &test_api_key(),
        &test_pem_key(),
        CoinbaseEnvironment::Live,
        10,
        None,
        retry_config,
    )
    .unwrap();
    client.set_base_url(format!("http://{addr}"));
    client
}

// Keeps retry-loop tests fast.
fn fast_retry_config(max_retries: u32) -> RetryConfig {
    RetryConfig {
        max_retries,
        initial_delay_ms: 5,
        max_delay_ms: 5,
        backoff_factor: 1.0,
        jitter_ms: 0,
        operation_timeout_ms: Some(2_000),
        immediate_first: false,
        max_elapsed_ms: None,
    }
}

fn account_id() -> AccountId {
    AccountId::new("COINBASE-001")
}

fn order_json(order_id: &str, product_id: &str, client_order_id: &str, status: &str) -> Value {
    json!({
        "order_id": order_id,
        "product_id": product_id,
        "user_id": "user-1",
        "order_configuration": {
            "limit_limit_gtc": {
                "base_size": "0.001",
                "limit_price": "50000.00",
                "post_only": false
            }
        },
        "side": "BUY",
        "client_order_id": client_order_id,
        "status": status,
        "time_in_force": "GOOD_UNTIL_CANCELLED",
        "created_time": "2024-01-15T10:00:00Z",
        "completion_percentage": "0",
        "filled_size": "0",
        "average_filled_price": "0",
        "fee": "0",
        "number_of_fills": "0",
        "filled_value": "0",
        "pending_cancel": false,
        "size_in_quote": false,
        "total_fees": "0",
        "size_inclusive_of_fees": false,
        "total_value_after_fees": "0",
        "trigger_status": "INVALID_ORDER_TYPE",
        "order_type": "LIMIT",
        "reject_reason": "",
        "settled": false,
        "product_type": "SPOT",
        "reject_message": "",
        "cancel_message": "",
        "order_placement_source": "RETAIL_ADVANCED",
        "outstanding_hold_amount": "0",
        "is_liquidation": false,
        "last_fill_time": null,
        "leverage": "",
        "margin_type": "",
        "retail_portfolio_id": "",
        "originating_order_id": "",
        "attached_order_id": ""
    })
}

fn fill_json(trade_id: &str, order_id: &str, product_id: &str) -> Value {
    json!({
        "entry_id": format!("entry-{trade_id}"),
        "trade_id": trade_id,
        "order_id": order_id,
        "trade_time": "2024-01-15T10:30:00Z",
        "trade_type": "FILL",
        "price": "45000.00",
        "size": "0.001",
        "commission": "0.50",
        "product_id": product_id,
        "sequence_timestamp": "2024-01-15T10:30:00.000Z",
        "liquidity_indicator": "MAKER",
        "size_in_quote": false,
        "user_id": "user-1",
        "side": "BUY",
        "retail_portfolio_id": ""
    })
}

fn account_json(currency: &str, available: &str, hold: &str, uuid: &str) -> Value {
    json!({
        "uuid": uuid,
        "name": format!("{currency} wallet"),
        "currency": currency,
        "available_balance": {"value": available, "currency": currency},
        "default": false,
        "active": true,
        "created_at": "2024-01-15T10:00:00Z",
        "updated_at": "2024-01-15T10:00:00Z",
        "deleted_at": null,
        "type": "FIAT",
        "ready": true,
        "hold": {"value": hold, "currency": currency},
        "retail_portfolio_id": "portfolio-1"
    })
}

fn btc_usd_instrument_id() -> InstrumentId {
    InstrumentId::from("BTC-USD.COINBASE")
}

fn query_pairs(raw_query: &str) -> Vec<(String, String)> {
    url::form_urlencoded::parse(raw_query.as_bytes())
        .into_owned()
        .collect()
}

async fn bootstrap_btc_usd_instrument(client: &CoinbaseHttpClient) -> InstrumentAny {
    client
        .request_instrument("BTC-USD")
        .await
        .expect("instrument bootstrap")
}

#[rstest]
#[tokio::test]
async fn test_exec_client_request_order_status_reports_paginates_cursor() {
    let state = TestState::default();
    // Page 1: two orders, has_next=true, cursor="page2"
    state.enqueue(
        "/orders/historical/batch",
        json!({
            "orders": [
                order_json("venue-1", "BTC-USD", "client-1", "OPEN"),
                order_json("venue-2", "BTC-USD", "client-2", "OPEN"),
            ],
            "sequence": "0",
            "has_next": true,
            "cursor": "page2"
        }),
    );
    // Page 2: one order, has_next=false
    state.enqueue(
        "/orders/historical/batch",
        json!({
            "orders": [order_json("venue-3", "BTC-USD", "client-3", "FILLED")],
            "sequence": "0",
            "has_next": false,
            "cursor": ""
        }),
    );

    let addr = start_mock_server(state.clone()).await;
    let client = create_http_client(addr);
    bootstrap_btc_usd_instrument(&client).await;

    let reports = client
        .request_order_status_reports(account_id(), None, false, None, None, None)
        .await
        .unwrap();

    assert_eq!(reports.len(), 3);
    assert_eq!(reports[0].venue_order_id.as_str(), "venue-1");
    assert_eq!(reports[2].venue_order_id.as_str(), "venue-3");

    let batch_requests = state.requests_for("/orders/historical/batch");
    assert_eq!(batch_requests.len(), 2);
    assert!(
        !batch_requests[0].raw_query.contains("cursor="),
        "first request must not send a cursor, query={}",
        batch_requests[0].raw_query
    );
    let second_pairs = query_pairs(&batch_requests[1].raw_query);
    assert!(
        second_pairs
            .iter()
            .any(|(k, v)| k == "cursor" && v == "page2"),
        "second request must carry cursor=page2, query={}",
        batch_requests[1].raw_query
    );
}

#[rstest]
#[tokio::test]
async fn test_exec_client_request_order_status_reports_sends_plural_product_ids() {
    let state = TestState::default();
    state.enqueue(
        "/orders/historical/batch",
        json!({
            "orders": [order_json("venue-1", "BTC-USD", "client-1", "OPEN")],
            "sequence": "0",
            "has_next": false,
            "cursor": ""
        }),
    );

    let addr = start_mock_server(state.clone()).await;
    let client = create_http_client(addr);
    bootstrap_btc_usd_instrument(&client).await;

    let _ = client
        .request_order_status_reports(
            account_id(),
            Some(btc_usd_instrument_id()),
            false,
            None,
            None,
            None,
        )
        .await
        .unwrap();

    let batch_requests = state.requests_for("/orders/historical/batch");
    assert_eq!(batch_requests.len(), 1);
    let pairs = query_pairs(&batch_requests[0].raw_query);
    assert!(
        pairs
            .iter()
            .any(|(k, v)| k == "product_ids" && v == "BTC-USD"),
        "expected product_ids=BTC-USD, query={}",
        batch_requests[0].raw_query
    );
    assert!(
        !pairs.iter().any(|(k, _)| k == "product_id"),
        "must not send singular product_id, query={}",
        batch_requests[0].raw_query
    );
}

#[rstest]
#[tokio::test]
async fn test_exec_client_request_order_status_reports_honors_hard_limit() {
    let state = TestState::default();

    // Three pages of 10 orders each. With limit=25 the client should stop
    // after page 3 and truncate the collected vector to 25.
    for page in 0..3 {
        let orders: Vec<Value> = (0..10)
            .map(|i| {
                order_json(
                    &format!("venue-{page}-{i}"),
                    "BTC-USD",
                    &format!("client-{page}-{i}"),
                    "OPEN",
                )
            })
            .collect();
        let (cursor, has_next) = if page < 2 {
            (format!("page{}", page + 1), true)
        } else {
            (String::new(), false)
        };
        state.enqueue(
            "/orders/historical/batch",
            json!({
                "orders": orders,
                "sequence": "0",
                "has_next": has_next,
                "cursor": cursor
            }),
        );
    }

    let addr = start_mock_server(state.clone()).await;
    let client = create_http_client(addr);
    bootstrap_btc_usd_instrument(&client).await;

    let reports = client
        .request_order_status_reports(account_id(), None, false, None, None, Some(25))
        .await
        .unwrap();

    assert_eq!(reports.len(), 25);
    // The client fetched three pages before truncating at 30 collected items.
    let batch_requests = state.requests_for("/orders/historical/batch");
    assert_eq!(batch_requests.len(), 3);
}

#[rstest]
#[tokio::test]
async fn test_exec_client_request_order_status_reports_encodes_rfc3339_start_date() {
    let state = TestState::default();
    state.enqueue(
        "/orders/historical/batch",
        json!({
            "orders": [],
            "sequence": "0",
            "has_next": false,
            "cursor": ""
        }),
    );

    let addr = start_mock_server(state.clone()).await;
    let client = create_http_client(addr);
    bootstrap_btc_usd_instrument(&client).await;

    let start = Utc.with_ymd_and_hms(2024, 1, 15, 10, 0, 0).unwrap();
    let _ = client
        .request_order_status_reports(account_id(), None, false, Some(start), None, None)
        .await
        .unwrap();

    let batch_requests = state.requests_for("/orders/historical/batch");
    assert_eq!(batch_requests.len(), 1);
    let raw_query = &batch_requests[0].raw_query;
    // RFC 3339 UTC serializes as `+00:00`, which must arrive percent-encoded.
    assert!(
        raw_query.contains("%2B00%3A00"),
        "timestamp plus sign must be percent-encoded, raw_query={raw_query}"
    );
    assert!(
        !raw_query.contains("+00:00"),
        "raw plus sign must not leak into query, raw_query={raw_query}"
    );
}

#[rstest]
#[tokio::test]
async fn test_exec_client_request_order_status_report_by_client_order_id_fallback() {
    let state = TestState::default();
    // When the caller provides only a client_order_id, the client falls back
    // to /orders/historical/batch and filters in-process.
    state.enqueue(
        "/orders/historical/batch",
        json!({
            "orders": [
                order_json("venue-1", "BTC-USD", "wrong-1", "OPEN"),
                order_json("venue-2", "BTC-USD", "target-client", "OPEN"),
                order_json("venue-3", "BTC-USD", "wrong-2", "OPEN"),
            ],
            "sequence": "0",
            "has_next": false,
            "cursor": ""
        }),
    );

    let addr = start_mock_server(state.clone()).await;
    let client = create_http_client(addr);
    bootstrap_btc_usd_instrument(&client).await;

    let report = client
        .request_order_status_report(
            account_id(),
            Some(ClientOrderId::new("target-client")),
            None,
        )
        .await
        .unwrap();

    assert_eq!(report.venue_order_id.as_str(), "venue-2");
    assert_eq!(report.client_order_id.unwrap().as_str(), "target-client");
    // Must hit the batch endpoint, not the single-order endpoint.
    assert_eq!(state.requests_for("/orders/historical/batch").len(), 1);
    let single_hits = state
        .requests()
        .iter()
        .filter(|r| {
            r.path.starts_with("/orders/historical/")
                && r.path != "/orders/historical/batch"
                && r.path != "/orders/historical/fills"
        })
        .count();
    assert_eq!(single_hits, 0);
}

#[rstest]
#[tokio::test]
async fn test_exec_client_request_fill_reports_sends_plural_keys_and_paginates() {
    let state = TestState::default();
    state.enqueue(
        "/orders/historical/fills",
        json!({
            "fills": [fill_json("trade-1", "venue-1", "BTC-USD")],
            "cursor": "fillpage2"
        }),
    );
    state.enqueue(
        "/orders/historical/fills",
        json!({
            "fills": [fill_json("trade-2", "venue-1", "BTC-USD")],
            "cursor": ""
        }),
    );

    let addr = start_mock_server(state.clone()).await;
    let client = create_http_client(addr);
    bootstrap_btc_usd_instrument(&client).await;

    let reports = client
        .request_fill_reports(
            account_id(),
            Some(btc_usd_instrument_id()),
            Some(VenueOrderId::new("venue-1")),
            None,
            None,
            None,
        )
        .await
        .unwrap();

    assert_eq!(reports.len(), 2);
    assert_eq!(reports[0].trade_id.as_str(), "trade-1");
    assert_eq!(reports[1].trade_id.as_str(), "trade-2");

    let fill_requests = state.requests_for("/orders/historical/fills");
    assert_eq!(fill_requests.len(), 2);

    let first_pairs = query_pairs(&fill_requests[0].raw_query);
    assert!(
        first_pairs
            .iter()
            .any(|(k, v)| k == "product_ids" && v == "BTC-USD"),
        "expected product_ids=BTC-USD, query={}",
        fill_requests[0].raw_query
    );
    assert!(
        first_pairs
            .iter()
            .any(|(k, v)| k == "order_ids" && v == "venue-1"),
        "expected order_ids=venue-1, query={}",
        fill_requests[0].raw_query
    );
    assert!(
        !first_pairs
            .iter()
            .any(|(k, _)| k == "product_id" || k == "order_id"),
        "must not send singular filter keys, query={}",
        fill_requests[0].raw_query
    );

    let second_pairs = query_pairs(&fill_requests[1].raw_query);
    assert!(
        second_pairs
            .iter()
            .any(|(k, v)| k == "cursor" && v == "fillpage2"),
        "second request must carry cursor=fillpage2, query={}",
        fill_requests[1].raw_query
    );
}

#[rstest]
#[tokio::test]
async fn test_exec_client_request_account_state_paginates_and_aggregates() {
    let state = TestState::default();
    // Two pages of accounts; USD appears on both pages and must be summed.
    state.enqueue(
        "/accounts",
        json!({
            "accounts": [
                account_json("USD", "1000.00", "50.00", "uuid-1"),
                account_json("BTC", "0.5", "0.1", "uuid-2"),
            ],
            "has_next": true,
            "cursor": "acct-page2",
            "size": 2
        }),
    );
    state.enqueue(
        "/accounts",
        json!({
            "accounts": [
                account_json("USD", "2500.00", "25.00", "uuid-3"),
            ],
            "has_next": false,
            "cursor": "",
            "size": 1
        }),
    );

    let addr = start_mock_server(state.clone()).await;
    let client = create_http_client(addr);

    let account = client.request_account_state(account_id()).await.unwrap();

    // Two unique currencies even though three wallets came back across pages.
    assert_eq!(account.balances.len(), 2);

    let usd = account
        .balances
        .iter()
        .find(|b| b.currency.code.as_str() == "USD")
        .expect("USD aggregated across pages");
    assert_eq!(usd.free.as_decimal(), dec!(3500.00));
    assert_eq!(usd.locked.as_decimal(), dec!(75.00));
    assert_eq!(usd.total.as_decimal(), dec!(3575.00));

    let btc = account
        .balances
        .iter()
        .find(|b| b.currency.code.as_str() == "BTC")
        .expect("BTC present");
    assert_eq!(btc.free.as_decimal(), dec!(0.5));
    assert_eq!(btc.locked.as_decimal(), dec!(0.1));

    // Cursor followed, so the second request carried cursor=acct-page2.
    let account_requests = state.requests_for("/accounts");
    assert_eq!(account_requests.len(), 2);
    let second_pairs = query_pairs(&account_requests[1].raw_query);
    assert!(
        second_pairs
            .iter()
            .any(|(k, v)| k == "cursor" && v == "acct-page2"),
        "second accounts request must carry cursor, query={}",
        account_requests[1].raw_query
    );
}

#[rstest]
#[tokio::test]
async fn test_exec_client_get_or_fetch_instrument_lazy_fetches_missing_product() {
    let state = TestState::default();
    // Two identical order pages so we can issue two independent calls.
    for _ in 0..2 {
        state.enqueue(
            "/orders/historical/batch",
            json!({
                "orders": [order_json("venue-1", "BTC-USD", "client-1", "OPEN")],
                "sequence": "0",
                "has_next": false,
                "cursor": ""
            }),
        );
    }

    let addr = start_mock_server(state.clone()).await;
    // Fresh client, instrument cache empty. The first call must lazy-fetch
    // the product definition via /products/{id} before parsing the order.
    let client = create_http_client(addr);

    let reports = client
        .request_order_status_reports(account_id(), None, false, None, None, None)
        .await
        .unwrap();

    assert_eq!(reports.len(), 1);
    assert_eq!(state.requests_for("/market/products/BTC-USD").len(), 1);

    // A follow-up call must not re-fetch the product: it now hits cache.
    let reports2 = client
        .request_order_status_reports(account_id(), None, false, None, None, None)
        .await
        .unwrap();
    assert_eq!(reports2.len(), 1);
    assert_eq!(
        state.requests_for("/market/products/BTC-USD").len(),
        1,
        "cached instrument must not trigger a second /products fetch"
    );
}

fn create_order_success_response(order_id: &str, client_order_id: &str) -> Value {
    json!({
        "success": true,
        "failure_reason": "",
        "order_id": order_id,
        "success_response": {
            "order_id": order_id,
            "product_id": "BTC-USD",
            "side": "BUY",
            "client_order_id": client_order_id,
        }
    })
}

fn create_order_failure_response() -> Value {
    json!({
        "success": false,
        "failure_reason": "INSUFFICIENT_FUND",
        "order_id": "",
        "error_response": {
            "error": "INSUFFICIENT_FUND",
            "message": "Insufficient balance",
            "error_details": "available=0",
            "preview_failure_reason": "",
            "new_order_failure_reason": "",
        }
    })
}

#[rstest]
#[tokio::test]
async fn test_http_submit_order_limit_gtc_serializes_typed_body() {
    let state = TestState::default();
    state.enqueue(
        "/orders",
        create_order_success_response("venue-100", "client-100"),
    );

    let addr = start_mock_server(state.clone()).await;
    let client = create_http_client(addr);

    let response = client
        .submit_order(
            ClientOrderId::new("client-100"),
            btc_usd_instrument_id(),
            OrderSide::Buy,
            OrderType::Limit,
            Quantity::from("0.5"),
            TimeInForce::Gtc,
            Some(Price::from("50000.00")),
            None,
            None,
            true, // post_only
            false,
            None,
            None,
            false,
            None,
        )
        .await
        .unwrap();

    assert!(response.success);
    assert_eq!(response.order_id, "venue-100");

    // Verify the typed body was serialized correctly to limit_limit_gtc shape.
    let requests = state.requests_for("/orders");
    assert_eq!(requests.len(), 1);
    let body = requests[0].body.as_ref().expect("POST body captured");
    assert_eq!(body["client_order_id"], "client-100");
    assert_eq!(body["product_id"], "BTC-USD");
    assert_eq!(body["side"], "BUY");
    let cfg = &body["order_configuration"]["limit_limit_gtc"];
    assert_eq!(cfg["base_size"], "0.5");
    assert_eq!(cfg["limit_price"], "50000.00");
    assert_eq!(cfg["post_only"], true);
    // retail_portfolio_id was None; the field must be omitted so the venue
    // routes the order to the key's default portfolio.
    assert!(body.get("retail_portfolio_id").is_none());
}

#[rstest]
#[tokio::test]
async fn test_http_submit_order_threads_retail_portfolio_id_when_set() {
    let state = TestState::default();
    state.enqueue(
        "/orders",
        create_order_success_response("venue-portfolio", "client-portfolio"),
    );
    let addr = start_mock_server(state.clone()).await;
    let client = create_http_client(addr);

    let response = client
        .submit_order(
            ClientOrderId::new("client-portfolio"),
            btc_usd_instrument_id(),
            OrderSide::Buy,
            OrderType::Limit,
            Quantity::from("0.5"),
            TimeInForce::Gtc,
            Some(Price::from("50000.00")),
            None,
            None,
            true, // post_only
            false,
            None,
            None,
            false,
            Some("portfolio-uuid-123".to_string()),
        )
        .await
        .unwrap();

    assert!(response.success);
    let requests = state.requests_for("/orders");
    let body = requests[0].body.as_ref().expect("POST body captured");
    assert_eq!(
        body["retail_portfolio_id"], "portfolio-uuid-123",
        "retail_portfolio_id must reach the wire when configured"
    );
}

#[rstest]
#[tokio::test]
async fn test_http_submit_order_market_uses_base_size_when_not_quote_qty() {
    let state = TestState::default();
    state.enqueue(
        "/orders",
        create_order_success_response("venue-200", "client-200"),
    );

    let addr = start_mock_server(state.clone()).await;
    let client = create_http_client(addr);

    let _ = client
        .submit_order(
            ClientOrderId::new("client-200"),
            btc_usd_instrument_id(),
            OrderSide::Buy,
            OrderType::Market,
            Quantity::from("0.001"),
            TimeInForce::Ioc,
            None,
            None,
            None,
            false,
            false, // is_quote_quantity = false → base_size
            None,
            None,
            false,
            None,
        )
        .await
        .unwrap();

    let requests = state.requests_for("/orders");
    assert_eq!(requests.len(), 1);
    let body = requests[0].body.as_ref().unwrap();
    let cfg = &body["order_configuration"]["market_market_ioc"];
    assert_eq!(cfg["base_size"], "0.001");
    assert!(cfg.get("quote_size").is_none());
}

#[rstest]
#[tokio::test]
async fn test_http_submit_order_returns_failure_response() {
    let state = TestState::default();
    state.enqueue("/orders", create_order_failure_response());

    let addr = start_mock_server(state.clone()).await;
    let client = create_http_client(addr);

    let response = client
        .submit_order(
            ClientOrderId::new("client-300"),
            btc_usd_instrument_id(),
            OrderSide::Buy,
            OrderType::Limit,
            Quantity::from("0.5"),
            TimeInForce::Gtc,
            Some(Price::from("50000.00")),
            None,
            None,
            false,
            false,
            None,
            None,
            false,
            None,
        )
        .await
        .unwrap();

    assert!(!response.success);
    let err = response
        .error_response
        .as_ref()
        .expect("error_response set");
    assert_eq!(err.error, "INSUFFICIENT_FUND");
    assert_eq!(err.message, "Insufficient balance");
}

#[rstest]
#[tokio::test]
async fn test_http_submit_order_rejects_unsupported_market_tif() {
    let state = TestState::default();
    let addr = start_mock_server(state.clone()).await;
    let client = create_http_client(addr);

    // DAY on MARKET is unsupported; should fail before HTTP. (IOC and FOK
    // both map to valid `market_market_*` configurations.)
    let result = client
        .submit_order(
            ClientOrderId::new("client-400"),
            btc_usd_instrument_id(),
            OrderSide::Buy,
            OrderType::Market,
            Quantity::from("0.001"),
            TimeInForce::Day,
            None,
            None,
            None,
            false,
            false,
            None,
            None,
            false,
            None,
        )
        .await;

    assert!(result.is_err());
    assert!(
        state.requests_for("/orders").is_empty(),
        "no HTTP call made"
    );
}

#[rstest]
#[tokio::test]
async fn test_http_cancel_orders_serializes_order_ids_and_returns_results() {
    let state = TestState::default();
    state.enqueue(
        "/orders/batch_cancel",
        json!({
            "results": [
                {"success": true, "failure_reason": "", "order_id": "venue-1"},
                {"success": false, "failure_reason": "UNKNOWN_CANCEL_FAILURE_REASON", "order_id": "venue-2"},
            ]
        }),
    );

    let addr = start_mock_server(state.clone()).await;
    let client = create_http_client(addr);

    let response = client
        .cancel_orders(&[VenueOrderId::new("venue-1"), VenueOrderId::new("venue-2")])
        .await
        .unwrap();

    assert_eq!(response.results.len(), 2);
    assert!(response.results[0].success);
    assert!(!response.results[1].success);
    assert_eq!(
        response.results[1].failure_reason,
        "UNKNOWN_CANCEL_FAILURE_REASON"
    );

    let requests = state.requests_for("/orders/batch_cancel");
    assert_eq!(requests.len(), 1);
    let body = requests[0].body.as_ref().unwrap();
    assert_eq!(body["order_ids"][0], "venue-1");
    assert_eq!(body["order_ids"][1], "venue-2");
}

#[rstest]
#[tokio::test]
async fn test_http_modify_order_forwards_price_size_and_stop_price() {
    let state = TestState::default();
    state.enqueue("/orders/edit", json!({"success": true, "errors": []}));

    let addr = start_mock_server(state.clone()).await;
    let client = create_http_client(addr);

    let response = client
        .modify_order(
            VenueOrderId::new("venue-99"),
            Some(Price::from("55000.00")),
            Some(Quantity::from("0.75")),
            Some(Price::from("54000.00")),
        )
        .await
        .unwrap();

    assert!(response.success);
    let requests = state.requests_for("/orders/edit");
    assert_eq!(requests.len(), 1);
    let body = requests[0].body.as_ref().unwrap();
    assert_eq!(body["order_id"], "venue-99");
    assert_eq!(body["price"], "55000.00");
    assert_eq!(body["size"], "0.75");
    assert_eq!(body["stop_price"], "54000.00");
}

#[rstest]
#[tokio::test]
async fn test_http_modify_order_omits_unset_fields() {
    let state = TestState::default();
    state.enqueue("/orders/edit", json!({"success": true, "errors": []}));

    let addr = start_mock_server(state.clone()).await;
    let client = create_http_client(addr);

    let _ = client
        .modify_order(
            VenueOrderId::new("venue-99"),
            Some(Price::from("55000.00")),
            None,
            None,
        )
        .await
        .unwrap();

    let body = state.requests_for("/orders/edit")[0]
        .body
        .as_ref()
        .unwrap()
        .clone();
    assert_eq!(body["price"], "55000.00");
    assert!(body.get("size").is_none());
    assert!(body.get("stop_price").is_none());
}

#[rstest]
#[tokio::test]
async fn test_http_modify_order_returns_typed_failure_reason() {
    let state = TestState::default();
    state.enqueue(
        "/orders/edit",
        json!({
            "success": false,
            "errors": [{
                "edit_failure_reason": "ORDER_ALREADY_FILLED",
                "preview_failure_reason": "",
            }]
        }),
    );

    let addr = start_mock_server(state.clone()).await;
    let client = create_http_client(addr);

    let response = client
        .modify_order(
            VenueOrderId::new("venue-99"),
            Some(Price::from("55000.00")),
            None,
            None,
        )
        .await
        .unwrap();

    assert!(!response.success);
    assert_eq!(response.errors.len(), 1);
    assert_eq!(
        response.errors[0].edit_failure_reason,
        "ORDER_ALREADY_FILLED"
    );
}

// GET is idempotent so transient 503s retry up to `max_retries` times,
// giving `1 + max_retries` attempts.
#[rstest]
#[tokio::test]
async fn test_http_get_retries_transient_failure_up_to_budget() {
    let state = TestState::default();
    state.mark_failing("/market/products");

    let addr = start_mock_server(state.clone()).await;
    let client = create_http_client_with_retry(addr, Some(fast_retry_config(3)));

    let result = client.get_products().await;
    assert!(
        result.is_err(),
        "expected 503 to surface after retry budget"
    );

    let attempts = state.requests_for("/market/products").len();
    assert_eq!(
        attempts, 4,
        "GET should run once plus 3 retries; saw {attempts}"
    );
}

// POSTs to order endpoints mutate live state; replaying could place, edit,
// or cancel twice, so the retry gate must keep them single-shot.
#[rstest]
#[tokio::test]
async fn test_http_post_does_not_retry_transient_failure() {
    let state = TestState::default();
    state.mark_failing("/orders");

    let addr = start_mock_server(state.clone()).await;
    let client = create_http_client_with_retry(addr, Some(fast_retry_config(3)));

    let result = client
        .submit_order(
            ClientOrderId::new("client-retry-guard"),
            btc_usd_instrument_id(),
            OrderSide::Buy,
            OrderType::Limit,
            Quantity::from("0.1"),
            TimeInForce::Gtc,
            Some(Price::from("50000.00")),
            None,
            None,
            false,
            false,
            None,
            None,
            false,
            None,
        )
        .await;

    assert!(result.is_err(), "expected 503 to propagate from POST");

    let attempts = state.requests_for("/orders").len();
    assert_eq!(
        attempts, 1,
        "POST must run exactly once regardless of retry budget; saw {attempts}"
    );
}

#[rstest]
#[tokio::test]
async fn test_http_request_cfm_balance_summary_returns_parsed_summary() {
    let state = TestState::default();
    let addr = start_mock_server(state.clone()).await;
    let client = create_http_client(addr);

    let summary = client
        .request_cfm_balance_summary()
        .await
        .expect("CFM balance summary should deserialize");

    assert_eq!(
        summary.total_usd_balance.value,
        dec!(10000.00),
        "USD balance mirrors the fixture"
    );
    assert_eq!(summary.available_margin.value, dec!(7500.00));
    assert_eq!(
        summary
            .intraday_margin_window_measure
            .as_ref()
            .unwrap()
            .initial_margin
            .value,
        dec!(500.00)
    );

    let requests = state.requests_for("/cfm/balance_summary");
    assert_eq!(requests.len(), 1);
}

#[rstest]
#[tokio::test]
async fn test_http_request_cfm_margin_balances_picks_stricter_window() {
    let state = TestState::default();
    let addr = start_mock_server(state.clone()).await;
    let client = create_http_client(addr);

    let margins = client
        .request_cfm_margin_balances()
        .await
        .expect("margin balances should build from summary");

    // `MarginAccount::split_event_margins` keys account-level margins by
    // Currency alone, so we collapse to a single MarginBalance. We pick the
    // whole window with the larger `initial_margin` (overnight here, 1000 vs
    // 500) so the emitted pair matches a real venue window verbatim.
    assert_eq!(margins.len(), 1);
    assert_eq!(margins[0].initial.as_decimal(), dec!(1000.00));
    assert_eq!(margins[0].maintenance.as_decimal(), dec!(500.00));
}

#[rstest]
#[tokio::test]
async fn test_http_request_cfm_account_state_produces_margin_account() {
    use nautilus_model::enums::AccountType;

    let state = TestState::default();
    let addr = start_mock_server(state.clone()).await;
    let client = create_http_client(addr);

    let account_state = client
        .request_cfm_account_state(account_id())
        .await
        .expect("account state should build");

    assert_eq!(account_state.account_type, AccountType::Margin);
    assert_eq!(account_state.balances.len(), 1);
    // total = total_usd_balance (venue equity); free = available_margin;
    // locked = total - free captures both working-orders hold and margin
    // consumed by open positions.
    assert_eq!(account_state.balances[0].total.as_decimal(), dec!(10000.00));
    assert_eq!(account_state.balances[0].free.as_decimal(), dec!(7500.00));
    assert_eq!(account_state.balances[0].locked.as_decimal(), dec!(2500.00));
    assert_eq!(
        account_state.margins.len(),
        1,
        "intraday + overnight windows collapse to one account-level entry"
    );
}

#[rstest]
#[tokio::test]
async fn test_http_request_position_status_reports_for_cfm() {
    use nautilus_model::enums::PositionSideSpecified;

    let state = TestState::default();
    let addr = start_mock_server(state.clone()).await;
    let client = create_http_client(addr);

    let reports = client
        .request_position_status_reports(account_id())
        .await
        .expect("position reports should build");

    assert_eq!(reports.len(), 1);
    let report = &reports[0];
    assert_eq!(report.position_side, PositionSideSpecified::Long);
    assert_eq!(report.quantity, Quantity::from("2"));
    assert_eq!(report.avg_px_open, Some(dec!(49000.00)));
    assert_eq!(report.instrument_id.symbol.as_str(), "BIP-20DEC30-CDE");

    // `get_or_fetch_instrument` may trigger a single /market/products/{id}
    // lookup to bootstrap the BIP instrument, which is fine: the CFM
    // endpoint itself must only be hit once.
    let positions_requests = state.requests_for("/cfm/positions");
    assert_eq!(positions_requests.len(), 1);
}

#[rstest]
#[tokio::test]
async fn test_http_request_position_status_report_single_product() {
    use nautilus_model::enums::PositionSideSpecified;

    let state = TestState::default();
    let addr = start_mock_server(state.clone()).await;
    let client = create_http_client(addr);

    let instrument_id = InstrumentId::from("BIP-20DEC30-CDE.COINBASE");
    let report = client
        .request_position_status_report(account_id(), instrument_id)
        .await
        .expect("position report should build")
        .expect("fixture provides a non-flat position");

    assert_eq!(report.position_side, PositionSideSpecified::Short);
    assert_eq!(report.quantity, Quantity::from("3"));
    assert_eq!(report.avg_px_open, Some(dec!(51000.00)));

    let single_requests = state.requests_for("/cfm/positions/BIP-20DEC30-CDE");
    assert_eq!(single_requests.len(), 1);
}

#[rstest]
#[tokio::test]
async fn test_http_submit_order_threads_leverage_margin_type_reduce_only() {
    use nautilus_coinbase::common::enums::CoinbaseMarginType;

    let state = TestState::default();
    state.enqueue(
        "/orders",
        create_order_success_response("venue-500", "client-500"),
    );

    let addr = start_mock_server(state.clone()).await;
    let client = create_http_client(addr);

    let _ = client
        .submit_order(
            ClientOrderId::new("client-500"),
            btc_usd_instrument_id(),
            OrderSide::Sell,
            OrderType::Limit,
            Quantity::from("0.5"),
            TimeInForce::Gtc,
            Some(Price::from("50000.00")),
            None,
            None,
            false,
            false,
            Some(dec!(5)),
            Some(CoinbaseMarginType::Cross),
            true,
            None,
        )
        .await
        .expect("submit should succeed");

    let requests = state.requests_for("/orders");
    assert_eq!(requests.len(), 1);
    let body = requests[0].body.as_ref().expect("POST body captured");
    assert_eq!(body["leverage"], "5");
    assert_eq!(body["margin_type"], "CROSS");
    assert_eq!(body["reduce_only"], true);
}

// reduce_only must not leak into the wire payload when the order is not
// flagged; Coinbase would otherwise reject the field on spot accounts.
#[rstest]
#[tokio::test]
async fn test_http_submit_order_omits_derivatives_fields_for_spot_defaults() {
    let state = TestState::default();
    state.enqueue(
        "/orders",
        create_order_success_response("venue-501", "client-501"),
    );

    let addr = start_mock_server(state.clone()).await;
    let client = create_http_client(addr);

    let _ = client
        .submit_order(
            ClientOrderId::new("client-501"),
            btc_usd_instrument_id(),
            OrderSide::Buy,
            OrderType::Limit,
            Quantity::from("0.5"),
            TimeInForce::Gtc,
            Some(Price::from("50000.00")),
            None,
            None,
            false,
            false,
            None,
            None,
            false,
            None,
        )
        .await
        .expect("submit should succeed");

    let requests = state.requests_for("/orders");
    assert_eq!(requests.len(), 1);
    let body = requests[0].body.as_ref().unwrap();
    assert!(body.get("leverage").is_none());
    assert!(body.get("margin_type").is_none());
    assert!(body.get("reduce_only").is_none());
}

// Builds a `CoinbaseExecutionClient` against the mock server with explicit
// account type so exec-client-level dispatch (Margin vs Cash) is exercised
// without going through `connect()`. The returned client is constructed but
// not connected; HTTP-backed methods still hit the mock because the base URL
// is threaded through the config.
fn make_exec_client(
    addr: std::net::SocketAddr,
    account_type: AccountType,
) -> CoinbaseExecutionClient {
    // The emitter inside the exec client tries to publish on the global
    // runner sender; install a drop-through channel so constructing it is
    // safe in tests that only call report-generation methods.
    let (sender, _rx) = tokio::sync::mpsc::unbounded_channel::<ExecutionEvent>();
    replace_exec_event_sender(sender);

    let cache = std::rc::Rc::new(std::cell::RefCell::new(Cache::default()));
    let core = ExecutionClientCore::new(
        TraderId::from("TRADER-001"),
        ClientId::from("COINBASE-TEST"),
        *COINBASE_VENUE,
        OmsType::Netting,
        AccountId::from("COINBASE-001"),
        account_type,
        None,
        cache,
    );

    let config = CoinbaseExecClientConfig {
        api_key: Some(test_api_key()),
        api_secret: Some(test_pem_key()),
        base_url_rest: Some(format!("http://{addr}")),
        account_type,
        ..CoinbaseExecClientConfig::default()
    };

    CoinbaseExecutionClient::new(core, config).expect("exec client construction")
}

fn position_status_reports_cmd(
    instrument_id: Option<InstrumentId>,
) -> GeneratePositionStatusReports {
    GeneratePositionStatusReportsBuilder::default()
        .ts_init(UnixNanos::default())
        .instrument_id(instrument_id)
        .build()
        .expect("cmd build")
}

#[rstest]
#[tokio::test]
async fn test_exec_client_position_reports_margin_list_hits_cfm_positions() {
    let state = TestState::default();
    let addr = start_mock_server(state.clone()).await;
    let client = make_exec_client(addr, AccountType::Margin);

    let reports = client
        .generate_position_status_reports(&position_status_reports_cmd(None))
        .await
        .expect("position reports");

    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].position_side, PositionSideSpecified::Long);
    assert_eq!(reports[0].instrument_id.symbol.as_str(), "BIP-20DEC30-CDE");

    // Exec client must route to the list endpoint, not the single-product one.
    assert_eq!(state.requests_for("/cfm/positions").len(), 1);
    assert!(
        state
            .requests_for("/cfm/positions/BIP-20DEC30-CDE")
            .is_empty()
    );
}

#[rstest]
#[tokio::test]
async fn test_exec_client_position_reports_margin_single_hits_scoped_endpoint() {
    let state = TestState::default();
    let addr = start_mock_server(state.clone()).await;
    let client = make_exec_client(addr, AccountType::Margin);

    let instrument_id = InstrumentId::from("BIP-20DEC30-CDE.COINBASE");
    let reports = client
        .generate_position_status_reports(&position_status_reports_cmd(Some(instrument_id)))
        .await
        .expect("position reports");

    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].position_side, PositionSideSpecified::Short);
    assert_eq!(reports[0].instrument_id, instrument_id);

    // Exec client must target the single-product endpoint; the list
    // endpoint should not be touched.
    assert_eq!(
        state.requests_for("/cfm/positions/BIP-20DEC30-CDE").len(),
        1
    );
    assert!(state.requests_for("/cfm/positions").is_empty());
}

// Cash clients have no positions: the exec client's fast-path must return
// empty without hitting the venue so a Cash/Margin factory mix-up does not
// leak CFM traffic onto a spot account.
#[rstest]
#[tokio::test]
async fn test_exec_client_position_reports_cash_returns_empty_without_http() {
    let state = TestState::default();
    let addr = start_mock_server(state.clone()).await;
    let client = make_exec_client(addr, AccountType::Cash);

    let reports = client
        .generate_position_status_reports(&position_status_reports_cmd(None))
        .await
        .expect("position reports");

    assert!(reports.is_empty());
    assert!(state.requests_for("/cfm/positions").is_empty());
    assert!(
        state
            .requests_for("/cfm/positions/BIP-20DEC30-CDE")
            .is_empty()
    );
}

// A 5xx from /cfm/positions must surface as an error rather than collapse
// to an empty Ok(vec![]); otherwise `generate_mass_status` would return a
// snapshot with zero positions and the live manager's reconciliation path
// would treat a venue outage as "no positions open". Retry budgets may
// retry the GET internally before the error propagates.
#[rstest]
#[tokio::test]
async fn test_exec_client_position_reports_margin_list_propagates_http_failure() {
    let state = TestState::default();
    state.mark_failing("/cfm/positions");
    let addr = start_mock_server(state.clone()).await;
    let client = make_exec_client(addr, AccountType::Margin);

    let result = client
        .generate_position_status_reports(&position_status_reports_cmd(None))
        .await;
    assert!(
        result.is_err(),
        "503 from /cfm/positions must propagate, was {:?}",
        result.as_ref().map(Vec::len)
    );
    assert!(!state.requests_for("/cfm/positions").is_empty());
}

#[rstest]
#[tokio::test]
async fn test_exec_client_position_reports_margin_single_propagates_http_failure() {
    let state = TestState::default();
    state.mark_failing("/cfm/positions/BIP-20DEC30-CDE");
    let addr = start_mock_server(state.clone()).await;
    let client = make_exec_client(addr, AccountType::Margin);

    let result = client
        .generate_position_status_reports(&position_status_reports_cmd(Some(InstrumentId::from(
            "BIP-20DEC30-CDE.COINBASE",
        ))))
        .await;
    assert!(
        result.is_err(),
        "503 from /cfm/positions/BIP-20DEC30-CDE must propagate, was {:?}",
        result.as_ref().map(Vec::len)
    );
}

// Mass-status on a Margin client must route position reports through the
// CFM endpoint, not treat the Margin account as spot.
#[rstest]
#[tokio::test]
async fn test_exec_client_mass_status_margin_includes_cfm_positions() {
    let state = TestState::default();
    // Mass status also fetches orders/fills; enqueue empty pages.
    state.enqueue(
        "/orders/historical/batch",
        json!({"orders": [], "sequence": "0", "has_next": false, "cursor": ""}),
    );
    state.enqueue(
        "/orders/historical/fills",
        json!({"fills": [], "cursor": ""}),
    );
    let addr = start_mock_server(state.clone()).await;
    let client = make_exec_client(addr, AccountType::Margin);

    let mass_status = client
        .generate_mass_status(Some(60))
        .await
        .expect("mass status")
        .expect("mass status populated");

    // One position was expected through /cfm/positions; mass_status's
    // position_reports map is keyed by instrument_id with a Vec of reports.
    let position_reports = mass_status.position_reports();
    let instrument_id = InstrumentId::from("BIP-20DEC30-CDE.COINBASE");
    assert_eq!(
        position_reports.get(&instrument_id).map(Vec::len),
        Some(1),
        "Margin mass status must carry the CFM position"
    );
}

// HTTP error-path tests. Each spins up an ad-hoc router with a single failure
// behaviour so the assertion is unambiguous: the test name names the failure
// mode, and a regression in retry/parse handling fails exactly the right test.

async fn start_failure_server(router: Router) -> SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });

    let start = std::time::Instant::now();

    loop {
        if tokio::net::TcpStream::connect(addr).await.is_ok() {
            break;
        }
        assert!(
            start.elapsed() <= std::time::Duration::from_secs(5),
            "failure server did not start within timeout"
        );
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    addr
}

#[rstest]
#[tokio::test]
async fn test_http_submit_surfaces_error_on_500_status() {
    let attempts = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let attempts_clone = Arc::clone(&attempts);
    let router = Router::new().route(
        "/api/v3/brokerage/orders",
        post(move || {
            let attempts = Arc::clone(&attempts_clone);
            async move {
                attempts.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                (
                    axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({"error": "internal error"})),
                )
            }
        }),
    );
    let addr = start_failure_server(router).await;
    let client = create_http_client_with_retry(addr, Some(fast_retry_config(3)));

    let result = client
        .submit_order(
            ClientOrderId::new("client-500"),
            btc_usd_instrument_id(),
            OrderSide::Buy,
            OrderType::Limit,
            Quantity::from("0.1"),
            TimeInForce::Gtc,
            Some(Price::from("50000.00")),
            None,
            None,
            false,
            false,
            None,
            None,
            false,
            None,
        )
        .await;
    assert!(result.is_err(), "expected 500 to surface as error");
    // POSTs are non-idempotent; a 500 should never trigger the retry loop.
    assert_eq!(attempts.load(std::sync::atomic::Ordering::SeqCst), 1);
}

#[rstest]
#[tokio::test]
async fn test_http_submit_surfaces_error_on_429_status_without_retry() {
    let attempts = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let attempts_clone = Arc::clone(&attempts);
    let router = Router::new().route(
        "/api/v3/brokerage/orders",
        post(move || {
            let attempts = Arc::clone(&attempts_clone);
            async move {
                attempts.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                (
                    axum::http::StatusCode::TOO_MANY_REQUESTS,
                    Json(json!({"error": "rate limited"})),
                )
            }
        }),
    );
    let addr = start_failure_server(router).await;
    let client = create_http_client_with_retry(addr, Some(fast_retry_config(3)));

    let result = client
        .submit_order(
            ClientOrderId::new("client-429"),
            btc_usd_instrument_id(),
            OrderSide::Buy,
            OrderType::Limit,
            Quantity::from("0.1"),
            TimeInForce::Gtc,
            Some(Price::from("50000.00")),
            None,
            None,
            false,
            false,
            None,
            None,
            false,
            None,
        )
        .await;
    assert!(result.is_err(), "expected 429 to surface as error");
    assert_eq!(attempts.load(std::sync::atomic::Ordering::SeqCst), 1);
}

#[rstest]
#[tokio::test]
async fn test_http_get_products_surfaces_error_on_malformed_body() {
    // Server returns 200 with a non-JSON body. The deserializer must surface
    // the parse failure rather than swallow it silently. Track route hits so
    // the assertion would not pass on an off-route 404.
    let hits = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let hits_clone = Arc::clone(&hits);
    let router = Router::new().route(
        "/api/v3/brokerage/market/products",
        get(move || {
            let hits = Arc::clone(&hits_clone);
            async move {
                hits.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                "not json{{{"
            }
        }),
    );
    let addr = start_failure_server(router).await;
    let client = create_http_client(addr);

    let result = client.get_products().await;
    assert_eq!(
        hits.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "expected the malformed-body route to be hit exactly once"
    );
    let err = result.expect_err("malformed body must surface as a parse error");
    let msg = err.to_string().to_lowercase();
    assert!(
        msg.contains("parse")
            || msg.contains("decode")
            || msg.contains("expected")
            || msg.contains("json")
            || msg.contains("deserialize"),
        "expected a parse/decode-style error, was: {err}"
    );
}

#[rstest]
#[tokio::test]
async fn test_http_get_products_surfaces_error_on_404() {
    let hits = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let hits_clone = Arc::clone(&hits);
    let router = Router::new().route(
        "/api/v3/brokerage/market/products",
        get(move || {
            let hits = Arc::clone(&hits_clone);
            async move {
                hits.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                (
                    axum::http::StatusCode::NOT_FOUND,
                    Json(json!({"error": "not found"})),
                )
            }
        }),
    );
    let addr = start_failure_server(router).await;
    let client = create_http_client(addr);

    let result = client.get_products().await;
    assert_eq!(
        hits.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "expected the 404 route to be hit exactly once"
    );
    let err = result.expect_err("404 must surface as error");
    let msg = err.to_string();
    assert!(
        msg.contains("404") || msg.contains("not found") || msg.contains("Not Found"),
        "expected the error to reference the 404 status, was: {err}"
    );
}
