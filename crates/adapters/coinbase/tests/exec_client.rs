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
use nautilus_coinbase::{common::enums::CoinbaseEnvironment, http::client::CoinbaseHttpClient};
use nautilus_model::{
    enums::{OrderSide, OrderType, TimeInForce},
    identifiers::{AccountId, ClientOrderId, InstrumentId, VenueOrderId},
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
    } else {
        Json(json!({"error": "not found"}))
    }
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
    let mut client = CoinbaseHttpClient::from_credentials(
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

    // FOK on MARKET is unsupported; should fail before HTTP.
    let result = client
        .submit_order(
            ClientOrderId::new("client-400"),
            btc_usd_instrument_id(),
            OrderSide::Buy,
            OrderType::Market,
            Quantity::from("0.001"),
            TimeInForce::Fok,
            None,
            None,
            None,
            false,
            false,
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
        )
        .await;

    assert!(result.is_err(), "expected 503 to propagate from POST");

    let attempts = state.requests_for("/orders").len();
    assert_eq!(
        attempts, 1,
        "POST must run exactly once regardless of retry budget; saw {attempts}"
    );
}
