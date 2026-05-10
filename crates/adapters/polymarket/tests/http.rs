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

//! Integration tests for the Polymarket HTTP client using a mock server.

use std::{
    collections::{HashMap, VecDeque},
    net::SocketAddr,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use ahash::AHashMap;
use axum::{
    Router,
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Json, Response},
    routing::{delete, get, post},
};
use nautilus_common::{providers::InstrumentProvider, testing::wait_until_async};
use nautilus_model::identifiers::InstrumentId;
use nautilus_network::{http::HttpClient, retry::RetryConfig};
use nautilus_polymarket::{
    common::{credential::Credential, enums::PolymarketOrderType},
    filters::{
        EventParamsFilter, EventSlugFilter, GammaQueryFilter, MarketSlugFilter, SearchFilter,
        TagFilter,
    },
    http::{
        clob::PolymarketClobHttpClient,
        data_api::PolymarketDataApiHttpClient,
        gamma::{PolymarketGammaHttpClient, PolymarketGammaRawHttpClient},
        models::PolymarketOrder,
        query::{
            CancelMarketOrdersParams, GetBalanceAllowanceParams, GetGammaEventsParams,
            GetGammaMarketsParams, GetOrdersParams, GetSearchParams, GetTradesParams,
        },
    },
    providers::{PolymarketInstrumentProvider, build_gamma_params_from_hashmap},
};
use rstest::rstest;
use serde_json::{Value, json};

// base64url of b"test_secret_key_32bytes_pad12345"
const TEST_API_SECRET_B64: &str = "dGVzdF9zZWNyZXRfa2V5XzMyYnl0ZXNfcGFkMTIzNDU=";
const TEST_ADDRESS: &str = "0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266";

#[derive(Clone)]
struct TestServerState {
    request_count: Arc<tokio::sync::Mutex<usize>>,
    last_body: Arc<tokio::sync::Mutex<Option<Value>>>,
    last_headers: Arc<tokio::sync::Mutex<AHashMap<String, String>>>,
    rate_limit_after: Arc<AtomicUsize>,
    /// Delay before `handle_get_orders` responds. Used by the timeout test.
    get_orders_delay_secs: Arc<AtomicUsize>,
    orders_pages: Arc<tokio::sync::Mutex<VecDeque<Value>>>,
    gamma_response: Arc<tokio::sync::Mutex<Option<Value>>>,
    gamma_slug_responses: Arc<tokio::sync::Mutex<AHashMap<String, Value>>>,
    gamma_force_error: Arc<std::sync::atomic::AtomicBool>,
    gamma_event_slug_responses: Arc<tokio::sync::Mutex<AHashMap<String, Value>>>,
    gamma_events_response: Arc<tokio::sync::Mutex<Option<Value>>>,
    gamma_tags_response: Arc<tokio::sync::Mutex<Option<Value>>>,
    gamma_search_response: Arc<tokio::sync::Mutex<Option<Value>>>,
    gamma_clob_token_responses: Arc<tokio::sync::Mutex<AHashMap<String, Value>>>,
    single_order_response: Arc<tokio::sync::Mutex<Option<Value>>>,
    data_api_trade_pages: Arc<tokio::sync::Mutex<VecDeque<Value>>>,
}

impl Default for TestServerState {
    fn default() -> Self {
        Self {
            request_count: Arc::new(tokio::sync::Mutex::new(0)),
            last_body: Arc::new(tokio::sync::Mutex::new(None)),
            last_headers: Arc::new(tokio::sync::Mutex::new(AHashMap::new())),
            rate_limit_after: Arc::new(AtomicUsize::new(usize::MAX)),
            get_orders_delay_secs: Arc::new(AtomicUsize::new(0)),
            orders_pages: Arc::new(tokio::sync::Mutex::new(VecDeque::new())),
            gamma_response: Arc::new(tokio::sync::Mutex::new(None)),
            gamma_slug_responses: Arc::new(tokio::sync::Mutex::new(AHashMap::new())),
            gamma_force_error: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            gamma_event_slug_responses: Arc::new(tokio::sync::Mutex::new(AHashMap::new())),
            gamma_events_response: Arc::new(tokio::sync::Mutex::new(None)),
            gamma_tags_response: Arc::new(tokio::sync::Mutex::new(None)),
            gamma_search_response: Arc::new(tokio::sync::Mutex::new(None)),
            gamma_clob_token_responses: Arc::new(tokio::sync::Mutex::new(AHashMap::new())),
            single_order_response: Arc::new(tokio::sync::Mutex::new(None)),
            data_api_trade_pages: Arc::new(tokio::sync::Mutex::new(VecDeque::new())),
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

fn test_credential() -> Credential {
    Credential::new("test_api_key", TEST_API_SECRET_B64, "test_pass".to_string()).unwrap()
}

fn create_clob_client(addr: &SocketAddr) -> PolymarketClobHttpClient {
    create_clob_client_with_timeout(addr, 5)
}

fn create_clob_client_with_timeout(
    addr: &SocketAddr,
    timeout_secs: u64,
) -> PolymarketClobHttpClient {
    PolymarketClobHttpClient::new(
        test_credential(),
        TEST_ADDRESS.to_string(),
        Some(format!("http://{addr}")),
        timeout_secs,
    )
    .unwrap()
}

fn create_data_api_client(addr: &SocketAddr) -> PolymarketDataApiHttpClient {
    PolymarketDataApiHttpClient::new(Some(format!("http://{addr}")), 5).unwrap()
}

fn create_gamma_client(addr: &SocketAddr) -> PolymarketGammaRawHttpClient {
    PolymarketGammaRawHttpClient::new(Some(format!("http://{addr}")), 5).unwrap()
}

fn create_gamma_domain_client(addr: &SocketAddr) -> PolymarketGammaHttpClient {
    PolymarketGammaHttpClient::new(Some(format!("http://{addr}")), 5, RetryConfig::default())
        .unwrap()
}

fn gamma_market_with_slug(slug: &str, condition_id: &str, token_ids: [&str; 2]) -> Value {
    json!({
        "id": "100001",
        "conditionId": condition_id,
        "questionID": "0xquestion_test",
        "clobTokenIds": format!("[\"{}\", \"{}\"]", token_ids[0], token_ids[1]),
        "outcomes": "[\"Yes\", \"No\"]",
        "outcomePrices": "[\"0.60\", \"0.40\"]",
        "question": format!("Test market for slug {slug}"),
        "description": "Test description",
        "startDate": "2025-01-01T00:00:00Z",
        "endDate": "2025-12-31T23:59:59Z",
        "active": true,
        "closed": false,
        "acceptingOrders": true,
        "enableOrderBook": true,
        "orderPriceMinTickSize": 0.01,
        "orderMinSize": 5.0,
        "makerBaseFee": 0,
        "takerBaseFee": 30,
        "slug": slug,
        "negRisk": false
    })
}

fn extract_headers(headers: &HeaderMap) -> AHashMap<String, String> {
    headers
        .iter()
        .map(|(k, v)| (k.as_str().to_string(), v.to_str().unwrap_or("").to_string()))
        .collect()
}

async fn maybe_rate_limit(state: &TestServerState) -> Option<Response> {
    let mut count = state.request_count.lock().await;
    *count += 1;
    let limit = state.rate_limit_after.load(Ordering::Relaxed);
    if *count > limit {
        Some(
            (
                StatusCode::TOO_MANY_REQUESTS,
                Json(json!({"error": "Rate limit exceeded"})),
            )
                .into_response(),
        )
    } else {
        None
    }
}

async fn handle_get_orders(State(state): State<TestServerState>, headers: HeaderMap) -> Response {
    if let Some(r) = maybe_rate_limit(&state).await {
        return r;
    }
    let delay = state.get_orders_delay_secs.load(Ordering::Relaxed);
    if delay > 0 {
        tokio::time::sleep(Duration::from_secs(delay as u64)).await;
    }
    *state.last_headers.lock().await = extract_headers(&headers);
    let mut pages = state.orders_pages.lock().await;
    if let Some(page) = pages.pop_front() {
        return Json(page).into_response();
    }
    Json(load_json("http_open_orders_page.json")).into_response()
}

async fn handle_get_trades(State(state): State<TestServerState>, headers: HeaderMap) -> Response {
    if let Some(r) = maybe_rate_limit(&state).await {
        return r;
    }
    *state.last_headers.lock().await = extract_headers(&headers);
    Json(load_json("http_trades_page.json")).into_response()
}

async fn handle_get_balance(State(state): State<TestServerState>, headers: HeaderMap) -> Response {
    if let Some(r) = maybe_rate_limit(&state).await {
        return r;
    }
    *state.last_headers.lock().await = extract_headers(&headers);
    Json(load_json("http_balance_allowance_collateral.json")).into_response()
}

async fn handle_post_order(
    State(state): State<TestServerState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if let Some(r) = maybe_rate_limit(&state).await {
        return r;
    }
    *state.last_headers.lock().await = extract_headers(&headers);

    if let Ok(v) = serde_json::from_slice::<Value>(&body) {
        *state.last_body.lock().await = Some(v);
    }
    Json(load_json("http_order_response_ok.json")).into_response()
}

async fn handle_delete_order(
    State(state): State<TestServerState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if let Some(r) = maybe_rate_limit(&state).await {
        return r;
    }
    *state.last_headers.lock().await = extract_headers(&headers);

    if let Ok(v) = serde_json::from_slice::<Value>(&body) {
        *state.last_body.lock().await = Some(v);
    }
    Json(load_json("http_cancel_response_ok.json")).into_response()
}

async fn handle_delete_orders(
    State(state): State<TestServerState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if let Some(r) = maybe_rate_limit(&state).await {
        return r;
    }
    *state.last_headers.lock().await = extract_headers(&headers);

    if let Ok(v) = serde_json::from_slice::<Value>(&body) {
        *state.last_body.lock().await = Some(v);
    }
    Json(load_json("http_batch_cancel_response.json")).into_response()
}

async fn handle_cancel_all(State(state): State<TestServerState>, headers: HeaderMap) -> Response {
    if let Some(r) = maybe_rate_limit(&state).await {
        return r;
    }
    *state.last_headers.lock().await = extract_headers(&headers);
    *state.last_body.lock().await = None;
    Json(load_json("http_batch_cancel_response.json")).into_response()
}

async fn handle_cancel_market(
    State(state): State<TestServerState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if let Some(r) = maybe_rate_limit(&state).await {
        return r;
    }
    *state.last_headers.lock().await = extract_headers(&headers);

    if let Ok(v) = serde_json::from_slice::<Value>(&body) {
        *state.last_body.lock().await = Some(v);
    }
    Json(load_json("http_batch_cancel_response.json")).into_response()
}

async fn handle_gamma_markets(
    State(state): State<TestServerState>,
    axum::extract::Query(params): axum::extract::Query<HashMap<String, String>>,
) -> Response {
    if state
        .gamma_force_error
        .load(std::sync::atomic::Ordering::Relaxed)
    {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }

    if let Some(slug) = params.get("slug") {
        let slug_map = state.gamma_slug_responses.lock().await;
        if let Some(v) = slug_map.get(slug) {
            return Json(v.clone()).into_response();
        }
    }

    // Check for clob_token_ids-based lookup
    if let Some(resp) = handle_gamma_markets_with_clob_tokens(&state, &params).await {
        return resp;
    }

    let resp = state.gamma_response.lock().await;
    match resp.as_ref() {
        Some(v) => Json(v.clone()).into_response(),
        None => Json(json!([])).into_response(),
    }
}

async fn handle_gamma_markets_with_clob_tokens(
    state: &TestServerState,
    params: &HashMap<String, String>,
) -> Option<Response> {
    if let Some(clob_ids) = params.get("clob_token_ids") {
        let map = state.gamma_clob_token_responses.lock().await;
        if let Some(v) = map.get(clob_ids) {
            return Some(Json(v.clone()).into_response());
        }
        // If specific clob_token_ids are requested but not in the map,
        // check all registered slug responses for matching token_ids
        return None;
    }
    None
}

async fn handle_gamma_events(
    State(state): State<TestServerState>,
    axum::extract::Query(params): axum::extract::Query<HashMap<String, String>>,
) -> Response {
    if let Some(slug) = params.get("slug") {
        let slug_map = state.gamma_event_slug_responses.lock().await;
        if let Some(v) = slug_map.get(slug) {
            return Json(v.clone()).into_response();
        }
    }

    // Return generic events response if set
    let resp = state.gamma_events_response.lock().await;
    if let Some(v) = resp.as_ref() {
        return Json(v.clone()).into_response();
    }

    Json(json!([])).into_response()
}

async fn handle_gamma_tags(State(state): State<TestServerState>) -> Response {
    let resp = state.gamma_tags_response.lock().await;
    match resp.as_ref() {
        Some(v) => Json(v.clone()).into_response(),
        None => Json(json!([])).into_response(),
    }
}

async fn handle_public_search(State(state): State<TestServerState>) -> Response {
    let resp = state.gamma_search_response.lock().await;
    match resp.as_ref() {
        Some(v) => Json(v.clone()).into_response(),
        None => Json(json!({"markets": [], "events": []})).into_response(),
    }
}

async fn handle_data_api_trades(
    State(state): State<TestServerState>,
    axum::extract::Query(params): axum::extract::Query<HashMap<String, String>>,
) -> Response {
    let all_trades = state.data_api_trade_pages.lock().await;
    // Flatten all enqueued pages into a single pool for offset/limit slicing
    let pool: Vec<Value> = all_trades
        .iter()
        .filter_map(|v| v.as_array())
        .flatten()
        .cloned()
        .collect();

    let offset: usize = params
        .get("offset")
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let limit: usize = params
        .get("limit")
        .and_then(|s| s.parse().ok())
        .unwrap_or(pool.len());

    let page: Vec<Value> = pool.into_iter().skip(offset).take(limit).collect();
    Json(json!(page)).into_response()
}

async fn handle_get_order(State(state): State<TestServerState>) -> Response {
    let resp = state.single_order_response.lock().await;
    match resp.as_ref() {
        Some(v) => Json(v.clone()).into_response(),
        // Simulate empty 200 OK (no body)
        None => (StatusCode::OK, "").into_response(),
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
        .route("/cancel-market-orders", delete(handle_cancel_market))
        .route("/markets", get(handle_gamma_markets))
        .route("/events", get(handle_gamma_events))
        .route("/tags", get(handle_gamma_tags))
        .route("/public-search", get(handle_public_search))
        .route("/trades", get(handle_data_api_trades))
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
async fn test_get_orders_returns_orders() {
    let state = TestServerState::default();
    let addr = start_mock_server(state.clone()).await;
    let client = create_clob_client(&addr);

    let orders = client.get_orders(GetOrdersParams::default()).await.unwrap();

    assert_eq!(orders.len(), 2);
    assert_eq!(
        orders[0].id,
        "0xaaaa000000000000000000000000000000000000000000000000000000000001"
    );
    assert_eq!(
        orders[1].id,
        "0xbbbb000000000000000000000000000000000000000000000000000000000002"
    );
}

#[rstest]
#[tokio::test]
async fn test_get_trades_returns_trades() {
    let state = TestServerState::default();
    let addr = start_mock_server(state.clone()).await;
    let client = create_clob_client(&addr);

    let trades = client.get_trades(GetTradesParams::default()).await.unwrap();

    assert_eq!(trades.len(), 1);
    assert_eq!(trades[0].id, "trade-0x001");
}

#[rstest]
#[tokio::test]
async fn test_get_balance_allowance_returns_data() {
    let state = TestServerState::default();
    let addr = start_mock_server(state.clone()).await;
    let client = create_clob_client(&addr);

    let balance = client
        .get_balance_allowance(GetBalanceAllowanceParams::default())
        .await
        .unwrap();

    // Fixture is now in integer-micro-pUSD form, matching the live API.
    assert_eq!(balance.balance, rust_decimal_macros::dec!(1_000_000_000));
    assert_eq!(
        balance.allowance,
        Some(rust_decimal_macros::dec!(999_999_999_000_000)),
    );
}

#[rstest]
#[tokio::test]
async fn test_cancel_order_sends_order_id_in_body() {
    let state = TestServerState::default();
    let addr = start_mock_server(state.clone()).await;
    let client = create_clob_client(&addr);
    let order_id = "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef12";

    client.cancel_order(order_id).await.unwrap();

    let body = state.last_body.lock().await;
    let body = body.as_ref().unwrap();
    assert_eq!(body.get("orderID").unwrap().as_str().unwrap(), order_id);
}

#[rstest]
#[tokio::test]
async fn test_cancel_orders_sends_ids_array() {
    let state = TestServerState::default();
    let addr = start_mock_server(state.clone()).await;
    let client = create_clob_client(&addr);
    let id1 = "0x1111111111111111111111111111111111111111111111111111111111111111";
    let id2 = "0x2222222222222222222222222222222222222222222222222222222222222222";

    client.cancel_orders(&[id1, id2]).await.unwrap();

    let body = state.last_body.lock().await;
    let ids = body.as_ref().unwrap().as_array().unwrap();
    assert_eq!(ids.len(), 2);
    assert_eq!(ids[0].as_str().unwrap(), id1);
    assert_eq!(ids[1].as_str().unwrap(), id2);
}

#[rstest]
#[tokio::test]
async fn test_cancel_all_sends_no_body() {
    let state = TestServerState::default();
    let addr = start_mock_server(state.clone()).await;
    let client = create_clob_client(&addr);

    client.cancel_all().await.unwrap();

    // Server clears last_body to None for cancel-all (no body expected)
    let body = state.last_body.lock().await;
    assert!(body.is_none());
}

#[rstest]
#[tokio::test]
async fn test_cancel_market_orders_sends_market_param() {
    let state = TestServerState::default();
    let addr = start_mock_server(state.clone()).await;
    let client = create_clob_client(&addr);
    let market = "0xdd22472e552920b8438158ea7238bfadfa4f736aa4cee91a6b86c39ead110917";

    let params = CancelMarketOrdersParams {
        market: Some(market.to_string()),
        asset_id: None,
    };
    client.cancel_market_orders(params).await.unwrap();

    let body = state.last_body.lock().await;
    assert_eq!(
        body.as_ref()
            .unwrap()
            .get("market")
            .unwrap()
            .as_str()
            .unwrap(),
        market
    );
}

#[rstest]
#[tokio::test]
async fn test_authenticated_requests_include_poly_headers() {
    let state = TestServerState::default();
    let addr = start_mock_server(state.clone()).await;
    let client = create_clob_client(&addr);

    client.get_orders(GetOrdersParams::default()).await.unwrap();

    let headers = state.last_headers.lock().await;
    assert!(
        headers.contains_key("poly_address"),
        "Missing POLY_ADDRESS header"
    );
    assert!(
        headers.contains_key("poly_signature"),
        "Missing POLY_SIGNATURE header"
    );
    assert!(
        headers.contains_key("poly_timestamp"),
        "Missing POLY_TIMESTAMP header"
    );
    assert!(
        headers.contains_key("poly_api_key"),
        "Missing POLY_API_KEY header"
    );
    assert!(
        headers.contains_key("poly_passphrase"),
        "Missing POLY_PASSPHRASE header"
    );
    assert_eq!(headers["poly_address"], TEST_ADDRESS);
    assert_eq!(headers["poly_api_key"], "test_api_key");
    assert_eq!(headers["poly_passphrase"], "test_pass");
}

#[rstest]
#[tokio::test]
async fn test_rate_limit_returns_error() {
    let state = TestServerState::default();
    state.rate_limit_after.store(2, Ordering::Relaxed);
    let addr = start_mock_server(state.clone()).await;
    let client = create_clob_client(&addr);

    assert!(client.get_orders(GetOrdersParams::default()).await.is_ok());
    assert!(client.get_orders(GetOrdersParams::default()).await.is_ok());

    // Third request exceeds the limit
    let result = client.get_orders(GetOrdersParams::default()).await;
    assert!(result.is_err());
}

#[rstest]
#[tokio::test]
async fn test_request_times_out_when_server_is_slow() {
    // Server stalls for 3s; client is configured with a 1s transport timeout.
    // The request must error with a timeout near the 1s mark, not earlier
    // (would mean a different error class) and not later (would mean the
    // timeout did not engage).
    let state = TestServerState::default();
    state.get_orders_delay_secs.store(3, Ordering::Relaxed);
    let addr = start_mock_server(state.clone()).await;
    let client = create_clob_client_with_timeout(&addr, 1);

    let started = std::time::Instant::now();
    let result = client.get_orders(GetOrdersParams::default()).await;
    let elapsed = started.elapsed();

    let err = result.expect_err("request must error when server exceeds timeout");
    let err_text = err.to_string().to_lowercase();
    assert!(
        err_text.contains("timeout") || err_text.contains("timed out"),
        "error must indicate a timeout, not some other failure (got: {err_text})",
    );

    // Lower bound: must not have errored before the configured timeout.
    assert!(
        elapsed >= Duration::from_millis(800),
        "request errored before the timeout could engage (took {elapsed:?})",
    );
    // Upper bound: must not have hung past the configured timeout.
    assert!(
        elapsed < Duration::from_millis(2_500),
        "request did not honour the timeout (took {elapsed:?})",
    );

    // Server must have actually received the request (one increment via
    // `maybe_rate_limit` before the handler stalls).
    assert_eq!(
        *state.request_count.lock().await,
        1,
        "exactly one request should have reached the mock"
    );
}

#[rstest]
#[tokio::test]
async fn test_get_orders_auto_paginates_multiple_pages() {
    let state = TestServerState::default();

    // Page 1: one order, cursor points to page 2
    let page1 = json!({
        "data": [{
            "associate_trades": [],
            "id": "0xpage1order000000000000000000000000000000000000000000000000000001",
            "status": "LIVE",
            "market": "0xdd22472e552920b8438158ea7238bfadfa4f736aa4cee91a6b86c39ead110917",
            "original_size": "100.0000",
            "outcome": "Yes",
            "maker_address": "0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266",
            "owner": "00000000-0000-0000-0000-000000000001",
            "price": "0.5000",
            "side": "BUY",
            "size_matched": "0.0000",
            "asset_id": "71321045679252212594626385532706912750332728571942532289631379312455583992563",
            "expiration": null,
            "order_type": "GTC",
            "created_at": 1703875200001u64
        }],
        "next_cursor": "cGFnZTI="
    });
    // Page 2: one order, terminal cursor
    let page2 = json!({
        "data": [{
            "associate_trades": [],
            "id": "0xpage2order000000000000000000000000000000000000000000000000000002",
            "status": "LIVE",
            "market": "0xdd22472e552920b8438158ea7238bfadfa4f736aa4cee91a6b86c39ead110917",
            "original_size": "50.0000",
            "outcome": "No",
            "maker_address": "0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266",
            "owner": "00000000-0000-0000-0000-000000000001",
            "price": "0.4000",
            "side": "SELL",
            "size_matched": "0.0000",
            "asset_id": "52114319501245915516055106046884209969926127482827954674443846427813813222426",
            "expiration": null,
            "order_type": "GTC",
            "created_at": 1703875200002u64
        }],
        "next_cursor": "LTE="
    });
    state.orders_pages.lock().await.push_back(page1);
    state.orders_pages.lock().await.push_back(page2);

    let addr = start_mock_server(state.clone()).await;
    let client = create_clob_client(&addr);

    let orders = client.get_orders(GetOrdersParams::default()).await.unwrap();

    assert_eq!(orders.len(), 2, "Expected both pages to be combined");
    assert_eq!(
        orders[0].id,
        "0xpage1order000000000000000000000000000000000000000000000000000001"
    );
    assert_eq!(
        orders[1].id,
        "0xpage2order000000000000000000000000000000000000000000000000000002"
    );
}

#[rstest]
#[tokio::test]
async fn test_post_order_sends_order_body() {
    let state = TestServerState::default();
    let addr = start_mock_server(state.clone()).await;
    let client = create_clob_client(&addr);

    let order = load_json("http_signed_order.json");
    let order: PolymarketOrder = serde_json::from_value(order).unwrap();

    client
        .post_order(&order, PolymarketOrderType::GTC, false)
        .await
        .unwrap();

    let body = state.last_body.lock().await;
    let body = body.as_ref().unwrap();
    assert!(body.get("order").is_some(), "Body must contain 'order'");
    assert!(body.get("owner").is_some(), "Body must contain 'owner'");
    assert!(
        body.get("orderType").is_some(),
        "Body must contain 'orderType'"
    );
}

#[rstest]
#[tokio::test]
async fn test_get_orders_with_caller_provided_cursor_not_overwritten() {
    let state = TestServerState::default();

    // The server returns a single page ending with LTE= from the default handler
    let addr = start_mock_server(state.clone()).await;
    let client = create_clob_client(&addr);

    // Pass an explicit cursor; should NOT be overwritten with MA==
    let params = GetOrdersParams {
        next_cursor: Some("custom_cursor".to_string()),
        ..Default::default()
    };
    let result = client.get_orders(params).await;

    // Just verify it succeeds (cursor was passed through, server ignored it)
    assert!(result.is_ok());
}

#[rstest]
#[tokio::test]
async fn test_get_gamma_markets_bare_array_response() {
    let state = TestServerState::default();
    let gamma_market = load_json("gamma_market.json");
    *state.gamma_response.lock().await = Some(json!([gamma_market]));

    let addr = start_mock_server(state.clone()).await;
    let client = create_gamma_client(&addr);

    let markets = client
        .get_gamma_markets(GetGammaMarketsParams::default())
        .await
        .unwrap();

    assert_eq!(markets.len(), 1);
    assert_eq!(
        markets[0].condition_id,
        "0x78443f961b9a65869dcb39359de9960165c7e5cbad0904eac7f29cd77872a63b"
    );
}

#[rstest]
#[tokio::test]
async fn test_get_gamma_markets_wrapped_data_response() {
    let state = TestServerState::default();
    let gamma_market = load_json("gamma_market.json");
    *state.gamma_response.lock().await = Some(json!({"data": [gamma_market]}));

    let addr = start_mock_server(state.clone()).await;
    let client = create_gamma_client(&addr);

    let markets = client
        .get_gamma_markets(GetGammaMarketsParams::default())
        .await
        .unwrap();

    assert_eq!(markets.len(), 1);
    assert_eq!(
        markets[0].condition_id,
        "0x78443f961b9a65869dcb39359de9960165c7e5cbad0904eac7f29cd77872a63b"
    );
}

#[rstest]
#[tokio::test]
async fn test_load_by_slugs_does_not_set_initialized() {
    let state = TestServerState::default();
    let market = gamma_market_with_slug(
        "test-slug",
        "0xcondition_a",
        ["11111111111111111111", "22222222222222222222"],
    );
    state
        .gamma_slug_responses
        .lock()
        .await
        .insert("test-slug".to_string(), json!([market]));

    let addr = start_mock_server(state.clone()).await;
    let http_client = create_gamma_domain_client(&addr);
    let mut provider = PolymarketInstrumentProvider::new(http_client);

    provider
        .load_by_slugs(vec!["test-slug".to_string()])
        .await
        .unwrap();

    assert_eq!(provider.store().count(), 2);
    assert!(
        !provider.store().is_initialized(),
        "load_by_slugs must not mark the store as initialized"
    );
}

#[rstest]
#[tokio::test]
async fn test_load_by_slugs_then_load_triggers_load_all_fallback() {
    let state = TestServerState::default();
    let slug_market = gamma_market_with_slug(
        "slug-a",
        "0xcondition_slug_a",
        ["33333333333333333333", "44444444444444444444"],
    );
    state
        .gamma_slug_responses
        .lock()
        .await
        .insert("slug-a".to_string(), json!([slug_market]));

    let bulk_market = gamma_market_with_slug(
        "slug-bulk",
        "0xcondition_bulk",
        ["55555555555555555555", "66666666666666666666"],
    );
    *state.gamma_response.lock().await = Some(json!([bulk_market]));

    let addr = start_mock_server(state.clone()).await;
    let http_client = create_gamma_domain_client(&addr);
    let mut provider = PolymarketInstrumentProvider::new(http_client);

    provider
        .load_by_slugs(vec!["slug-a".to_string()])
        .await
        .unwrap();
    assert_eq!(provider.store().count(), 2);

    // load() for an unknown ID triggers load_all since store is not initialized
    let unknown_id = InstrumentId::from("UNKNOWN-UNKNOWN.POLYMARKET");
    let result = provider.load(&unknown_id, None).await;

    // load_all was called (store is now initialized), but unknown instrument still not found
    assert!(result.is_err());
    assert!(provider.store().is_initialized());
    // The bulk market instruments were loaded by the fallback
    assert!(provider.store().count() >= 2);
}

#[rstest]
#[tokio::test]
async fn test_request_instruments_by_slugs_all_fail_returns_error() {
    let state = TestServerState::default();
    state
        .gamma_force_error
        .store(true, std::sync::atomic::Ordering::Relaxed);

    let addr = start_mock_server(state.clone()).await;
    let client = create_gamma_domain_client(&addr);

    let result = client
        .request_instruments_by_slugs(vec!["bad-slug-a".to_string(), "bad-slug-b".to_string()])
        .await;

    assert!(result.is_err(), "All-slug failure must propagate as error");
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("slug requests failed"),
    );
}

#[rstest]
#[tokio::test]
async fn test_request_instruments_by_slugs_partial_failure_succeeds() {
    let state = TestServerState::default();
    let good_market = gamma_market_with_slug(
        "good-slug",
        "0xcondition_good",
        ["77777777777777777777", "88888888888888888888"],
    );
    state
        .gamma_slug_responses
        .lock()
        .await
        .insert("good-slug".to_string(), json!([good_market]));
    // "bad-slug" has no slug entry and force_error is off, so it returns [] (no markets)

    let addr = start_mock_server(state.clone()).await;
    let client = create_gamma_domain_client(&addr);

    let instruments = client
        .request_instruments_by_slugs(vec!["good-slug".to_string(), "bad-slug".to_string()])
        .await
        .unwrap();

    assert_eq!(
        instruments.len(),
        2,
        "good-slug produces 2 instruments (Yes/No)"
    );
}

fn gamma_event_with_markets(slug: &str, markets: &[Value]) -> Value {
    json!({
        "id": "evt-test-001",
        "slug": slug,
        "title": format!("Event for {slug}"),
        "active": true,
        "closed": false,
        "markets": markets
    })
}

#[rstest]
#[tokio::test]
async fn test_load_all_with_slug_filter() {
    let state = TestServerState::default();
    let market = gamma_market_with_slug(
        "filter-slug",
        "0xcondition_filter",
        ["10000000000000000001", "10000000000000000002"],
    );
    state
        .gamma_slug_responses
        .lock()
        .await
        .insert("filter-slug".to_string(), json!([market]));

    let addr = start_mock_server(state.clone()).await;
    let http_client = create_gamma_domain_client(&addr);
    let filter = MarketSlugFilter::from_slugs(vec!["filter-slug".to_string()]);
    let mut provider = PolymarketInstrumentProvider::with_filter(http_client, Arc::new(filter));

    provider.load_all(None).await.unwrap();

    assert_eq!(provider.store().count(), 2);
    assert!(provider.store().is_initialized());
}

#[rstest]
#[tokio::test]
async fn test_load_all_with_gamma_query_filter() {
    let state = TestServerState::default();
    let market = gamma_market_with_slug(
        "query-market",
        "0xcondition_query",
        ["20000000000000000001", "20000000000000000002"],
    );
    // The gamma_response is returned for non-slug market queries
    *state.gamma_response.lock().await = Some(json!([market]));

    let addr = start_mock_server(state.clone()).await;
    let http_client = create_gamma_domain_client(&addr);
    let filter = GammaQueryFilter::new(GetGammaMarketsParams {
        active: Some(true),
        volume_num_min: Some(1000.0),
        ..Default::default()
    });
    let mut provider = PolymarketInstrumentProvider::with_filter(http_client, Arc::new(filter));

    provider.load_all(None).await.unwrap();

    assert_eq!(provider.store().count(), 2);
    assert!(provider.store().is_initialized());
}

#[rstest]
#[tokio::test]
async fn test_load_all_without_filter_loads_everything() {
    let state = TestServerState::default();
    let market = gamma_market_with_slug(
        "bulk-market",
        "0xcondition_bulk_all",
        ["30000000000000000001", "30000000000000000002"],
    );
    *state.gamma_response.lock().await = Some(json!([market]));

    let addr = start_mock_server(state.clone()).await;
    let http_client = create_gamma_domain_client(&addr);
    // No filter — should use bulk loading
    let mut provider = PolymarketInstrumentProvider::new(http_client);

    provider.load_all(None).await.unwrap();

    assert_eq!(provider.store().count(), 2);
    assert!(provider.store().is_initialized());
}

#[rstest]
#[tokio::test]
async fn test_slug_filter_re_evaluated_each_cycle() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    let state = TestServerState::default();
    let market_a = gamma_market_with_slug(
        "slug-cycle-a",
        "0xcondition_cycle_a",
        ["40000000000000000001", "40000000000000000002"],
    );
    let market_b = gamma_market_with_slug(
        "slug-cycle-b",
        "0xcondition_cycle_b",
        ["40000000000000000003", "40000000000000000004"],
    );
    state
        .gamma_slug_responses
        .lock()
        .await
        .insert("slug-cycle-a".to_string(), json!([market_a]));
    state
        .gamma_slug_responses
        .lock()
        .await
        .insert("slug-cycle-b".to_string(), json!([market_b]));

    let addr = start_mock_server(state.clone()).await;
    let http_client = create_gamma_domain_client(&addr);

    let counter = Arc::new(AtomicUsize::new(0));
    let counter_clone = counter.clone();
    let filter = MarketSlugFilter::new(move || {
        let n = counter_clone.fetch_add(1, Ordering::Relaxed);
        if n == 0 {
            vec!["slug-cycle-a".to_string()]
        } else {
            vec!["slug-cycle-b".to_string()]
        }
    });

    let mut provider = PolymarketInstrumentProvider::with_filter(http_client, Arc::new(filter));

    // First cycle: loads slug-cycle-a
    provider.load_all(None).await.unwrap();
    assert_eq!(provider.store().count(), 2);

    // Second cycle: re-evaluates closure, loads slug-cycle-b instead
    provider.load_all(None).await.unwrap();
    assert_eq!(provider.store().count(), 2);
    assert_eq!(counter.load(Ordering::Relaxed), 2);
}

#[rstest]
#[tokio::test]
async fn test_set_filter_then_clear_reverts() {
    let state = TestServerState::default();
    let slug_market = gamma_market_with_slug(
        "filtered-slug",
        "0xcondition_filtered",
        ["50000000000000000001", "50000000000000000002"],
    );
    state
        .gamma_slug_responses
        .lock()
        .await
        .insert("filtered-slug".to_string(), json!([slug_market]));

    let bulk_market = gamma_market_with_slug(
        "bulk-after-clear",
        "0xcondition_bulk_clear",
        ["50000000000000000003", "50000000000000000004"],
    );
    *state.gamma_response.lock().await = Some(json!([bulk_market]));

    let addr = start_mock_server(state.clone()).await;
    let http_client = create_gamma_domain_client(&addr);
    let mut provider = PolymarketInstrumentProvider::new(http_client);

    // Set a filter and load
    let filter = MarketSlugFilter::from_slugs(vec!["filtered-slug".to_string()]);
    provider.add_filter(Arc::new(filter));
    provider.load_all(None).await.unwrap();
    assert_eq!(provider.store().count(), 2);

    // Clear filters and load again — should use bulk loading
    provider.clear_filters();
    provider.load_all(None).await.unwrap();
    assert_eq!(provider.store().count(), 2);
    assert!(provider.store().is_initialized());
}

#[rstest]
#[tokio::test]
async fn test_load_all_with_event_slug_filter() {
    let state = TestServerState::default();

    let market1 = gamma_market_with_slug(
        "event-market-1",
        "0xcondition_evtm1",
        ["60000000000000000001", "60000000000000000002"],
    );
    let market2 = gamma_market_with_slug(
        "event-market-2",
        "0xcondition_evtm2",
        ["60000000000000000003", "60000000000000000004"],
    );
    let event = gamma_event_with_markets("test-event", &[market1, market2]);
    state
        .gamma_event_slug_responses
        .lock()
        .await
        .insert("test-event".to_string(), json!([event]));

    let addr = start_mock_server(state.clone()).await;
    let http_client = create_gamma_domain_client(&addr);

    let filter = EventSlugFilter::from_slugs(vec!["test-event".to_string()]);
    let mut provider = PolymarketInstrumentProvider::with_filter(http_client, Arc::new(filter));

    provider.load_all(None).await.unwrap();

    // 2 markets × 2 outcomes = 4 instruments
    assert_eq!(provider.store().count(), 4);
    assert!(provider.store().is_initialized());
}

#[rstest]
#[tokio::test]
async fn test_composite_filter_combines_market_and_event_slugs() {
    let state = TestServerState::default();

    // Market slug response
    let market = gamma_market_with_slug(
        "composite-market",
        "0xcondition_composite_m",
        ["70000000000000000001", "70000000000000000002"],
    );
    state
        .gamma_slug_responses
        .lock()
        .await
        .insert("composite-market".to_string(), json!([market]));

    // Event slug response with a different market
    let event_market = gamma_market_with_slug(
        "composite-event-market",
        "0xcondition_composite_e",
        ["70000000000000000003", "70000000000000000004"],
    );
    let event = gamma_event_with_markets("composite-event", &[event_market]);
    state
        .gamma_event_slug_responses
        .lock()
        .await
        .insert("composite-event".to_string(), json!([event]));

    let addr = start_mock_server(state.clone()).await;
    let http_client = create_gamma_domain_client(&addr);

    let market_filter = MarketSlugFilter::from_slugs(vec!["composite-market".to_string()]);
    let event_filter = EventSlugFilter::from_slugs(vec!["composite-event".to_string()]);
    let mut provider = PolymarketInstrumentProvider::with_filters(
        http_client,
        vec![Arc::new(market_filter), Arc::new(event_filter)],
    );

    provider.load_all(None).await.unwrap();

    // 1 market slug (2 outcomes) + 1 event market (2 outcomes) = 4 instruments
    assert_eq!(provider.store().count(), 4);
    assert!(provider.store().is_initialized());
}

#[rstest]
#[tokio::test]
async fn test_get_gamma_events_with_params() {
    let state = TestServerState::default();

    let market = gamma_market_with_slug(
        "event-param-market",
        "0xcondition_evt_param",
        ["91000000000000000001", "91000000000000000002"],
    );
    let event = gamma_event_with_markets("event-from-params", &[market]);
    *state.gamma_events_response.lock().await = Some(json!([event]));

    let addr = start_mock_server(state.clone()).await;
    let client = create_gamma_client(&addr);

    let events = client
        .get_gamma_events(GetGammaEventsParams {
            active: Some(true),
            featured: Some(true),
            ..Default::default()
        })
        .await
        .unwrap();

    assert_eq!(events.len(), 1);
    assert_eq!(events[0].slug.as_deref(), Some("event-from-params"));
    assert_eq!(events[0].markets.len(), 1);
}

#[rstest]
#[tokio::test]
async fn test_get_gamma_tags() {
    let state = TestServerState::default();
    *state.gamma_tags_response.lock().await = Some(load_json("gamma_tags.json"));

    let addr = start_mock_server(state.clone()).await;
    let client = create_gamma_client(&addr);

    let tags = client.get_gamma_tags().await.unwrap();

    assert_eq!(tags.len(), 5);
    assert_eq!(tags[0].id, "101259");
    assert_eq!(tags[0].label.as_deref(), Some("Health and Human Services"));
    assert_eq!(tags[1].slug.as_deref(), Some("sweeden"));
}

#[rstest]
#[tokio::test]
async fn test_get_public_search() {
    let state = TestServerState::default();
    *state.gamma_search_response.lock().await = Some(load_json("search_response.json"));

    let addr = start_mock_server(state.clone()).await;
    let client = create_gamma_client(&addr);

    let response = client
        .get_public_search(GetSearchParams {
            q: Some("bitcoin".to_string()),
            ..Default::default()
        })
        .await
        .unwrap();

    // Real API returns no top-level "markets" key
    assert!(response.markets.is_none());

    let events = response.events.as_ref().unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].markets.len(), 1);
}

#[rstest]
#[tokio::test]
async fn test_request_instruments_by_event_params() {
    let state = TestServerState::default();

    let market = gamma_market_with_slug(
        "evt-param-mkt",
        "0xcondition_evt_param_inst",
        ["92000000000000000001", "92000000000000000002"],
    );
    let event = gamma_event_with_markets("evt-param-test", &[market]);
    *state.gamma_events_response.lock().await = Some(json!([event]));

    let addr = start_mock_server(state.clone()).await;
    let client = create_gamma_domain_client(&addr);

    let instruments = client
        .request_instruments_by_event_params(GetGammaEventsParams {
            active: Some(true),
            ..Default::default()
        })
        .await
        .unwrap();

    assert_eq!(instruments.len(), 2);
}

#[rstest]
#[tokio::test]
async fn test_request_instruments_by_search() {
    let state = TestServerState::default();
    *state.gamma_search_response.lock().await = Some(load_json("search_response.json"));

    let addr = start_mock_server(state.clone()).await;
    let client = create_gamma_domain_client(&addr);

    let instruments = client
        .request_instruments_by_search(GetSearchParams {
            q: Some("bitcoin".to_string()),
            ..Default::default()
        })
        .await
        .unwrap();

    // search_response.json: no top-level markets, 1 event with 1 market (2 outcomes) = 2
    assert_eq!(instruments.len(), 2);
}

#[rstest]
#[tokio::test]
async fn test_request_tags() {
    let state = TestServerState::default();
    *state.gamma_tags_response.lock().await = Some(load_json("gamma_tags.json"));

    let addr = start_mock_server(state.clone()).await;
    let client = create_gamma_domain_client(&addr);

    let tags = client.request_tags().await.unwrap();
    assert_eq!(tags.len(), 5);
}

#[rstest]
#[tokio::test]
async fn test_load_ids_fetches_missing_instruments() {
    let state = TestServerState::default();

    let market = gamma_market_with_slug(
        "ids-market",
        "0xcondition_ids",
        ["93000000000000000001", "93000000000000000002"],
    );
    // Use generic gamma response — condition_ids query hits /markets
    *state.gamma_response.lock().await = Some(json!([market]));

    let addr = start_mock_server(state.clone()).await;
    let http_client = create_gamma_domain_client(&addr);
    let mut provider = PolymarketInstrumentProvider::new(http_client);

    // InstrumentId format: "{condition_id}-{token_id}.POLYMARKET"
    let instrument_id = InstrumentId::from("0xcondition_ids-93000000000000000001.POLYMARKET");
    provider.load_ids(&[instrument_id], None).await.unwrap();

    assert_eq!(provider.store().count(), 2);
}

#[rstest]
#[tokio::test]
async fn test_load_ids_skips_already_loaded() {
    let state = TestServerState::default();

    let market = gamma_market_with_slug(
        "preloaded-market",
        "0xcondition_preloaded",
        ["94000000000000000001", "94000000000000000002"],
    );
    state
        .gamma_slug_responses
        .lock()
        .await
        .insert("preloaded-market".to_string(), json!([market]));

    let addr = start_mock_server(state.clone()).await;
    let http_client = create_gamma_domain_client(&addr);
    let mut provider = PolymarketInstrumentProvider::new(http_client);

    // Pre-load
    provider
        .load_by_slugs(vec!["preloaded-market".to_string()])
        .await
        .unwrap();
    assert_eq!(provider.store().count(), 2);

    // load_ids with already-loaded ID should be a no-op
    let existing_id = InstrumentId::from("0xcondition_preloaded-94000000000000000001.POLYMARKET");
    provider.load_ids(&[existing_id], None).await.unwrap();

    // Count should still be 2 (no additional fetch)
    assert_eq!(provider.store().count(), 2);
}

#[rstest]
#[tokio::test]
async fn test_load_single_instrument_direct_fetch() {
    let state = TestServerState::default();

    let market = gamma_market_with_slug(
        "direct-load-market",
        "0xcondition_direct",
        ["95000000000000000001", "95000000000000000002"],
    );
    // Use generic gamma response — condition_ids query hits /markets
    *state.gamma_response.lock().await = Some(json!([market]));

    let addr = start_mock_server(state.clone()).await;
    let http_client = create_gamma_domain_client(&addr);
    let mut provider = PolymarketInstrumentProvider::new(http_client);

    // InstrumentId format: "{condition_id}-{token_id}.POLYMARKET"
    let instrument_id = InstrumentId::from("0xcondition_direct-95000000000000000001.POLYMARKET");
    provider.load(&instrument_id, None).await.unwrap();

    assert!(provider.store().contains(&instrument_id));
    // Direct fetch succeeded, so load_all was NOT called — store not initialized
    assert!(!provider.store().is_initialized());
}

#[rstest]
#[tokio::test]
async fn test_load_all_with_event_params_filter() {
    let state = TestServerState::default();

    let market = gamma_market_with_slug(
        "evt-params-filter-market",
        "0xcondition_epf",
        ["96000000000000000001", "96000000000000000002"],
    );
    let event = gamma_event_with_markets("evt-params-filter", &[market]);
    *state.gamma_events_response.lock().await = Some(json!([event]));

    let addr = start_mock_server(state.clone()).await;
    let http_client = create_gamma_domain_client(&addr);

    let filter = EventParamsFilter::new(GetGammaEventsParams {
        active: Some(true),
        featured: Some(true),
        ..Default::default()
    });
    let mut provider = PolymarketInstrumentProvider::with_filter(http_client, Arc::new(filter));

    provider.load_all(None).await.unwrap();

    assert_eq!(provider.store().count(), 2);
    assert!(provider.store().is_initialized());
}

#[rstest]
#[tokio::test]
async fn test_load_all_with_search_filter() {
    let state = TestServerState::default();
    *state.gamma_search_response.lock().await = Some(load_json("search_response.json"));

    let addr = start_mock_server(state.clone()).await;
    let http_client = create_gamma_domain_client(&addr);

    let filter = SearchFilter::from_query("bitcoin");
    let mut provider = PolymarketInstrumentProvider::with_filter(http_client, Arc::new(filter));

    provider.load_all(None).await.unwrap();

    // No top-level markets, 1 event with 1 market (2 outcomes) = 2
    assert_eq!(provider.store().count(), 2);
    assert!(provider.store().is_initialized());
}

#[rstest]
#[tokio::test]
async fn test_load_all_with_tag_filter() {
    let state = TestServerState::default();

    let market = gamma_market_with_slug(
        "tag-filter-market",
        "0xcondition_tag",
        ["97000000000000000001", "97000000000000000002"],
    );
    *state.gamma_response.lock().await = Some(json!([market]));

    let addr = start_mock_server(state.clone()).await;
    let http_client = create_gamma_domain_client(&addr);

    let filter = TagFilter::from_tag_id("tag-001");
    let mut provider = PolymarketInstrumentProvider::with_filter(http_client, Arc::new(filter));

    provider.load_all(None).await.unwrap();

    assert_eq!(provider.store().count(), 2);
    assert!(provider.store().is_initialized());
}

#[rstest]
#[tokio::test]
async fn test_load_filtered_deduplicates_overlapping_results() {
    let state = TestServerState::default();

    // Same market appears in both slug and query responses
    let market = gamma_market_with_slug(
        "dedup-market",
        "0xcondition_dedup",
        ["98000000000000000001", "98000000000000000002"],
    );
    state
        .gamma_slug_responses
        .lock()
        .await
        .insert("dedup-market".to_string(), json!([market.clone()]));
    *state.gamma_response.lock().await = Some(json!([market]));

    let addr = start_mock_server(state.clone()).await;
    let http_client = create_gamma_domain_client(&addr);

    // Composite filter: market slug + query params both return the same market
    let slug_filter = MarketSlugFilter::from_slugs(vec!["dedup-market".to_string()]);
    let query_filter = GammaQueryFilter::new(GetGammaMarketsParams {
        active: Some(true),
        ..Default::default()
    });
    let mut provider = PolymarketInstrumentProvider::with_filters(
        http_client,
        vec![Arc::new(slug_filter), Arc::new(query_filter)],
    );

    provider.load_all(None).await.unwrap();

    // Should be 2 (Yes/No), not 4 (deduplication should remove duplicates)
    assert_eq!(provider.store().count(), 2);
}

#[rstest]
#[tokio::test]
async fn test_load_all_with_hashmap_filters() {
    let state = TestServerState::default();

    let market = gamma_market_with_slug(
        "hashmap-market",
        "0xcondition_hashmap",
        ["99000000000000000001", "99000000000000000002"],
    );
    *state.gamma_response.lock().await = Some(json!([market]));

    let addr = start_mock_server(state.clone()).await;
    let http_client = create_gamma_domain_client(&addr);
    let mut provider = PolymarketInstrumentProvider::new(http_client);

    let mut filters = HashMap::new();
    filters.insert("active".to_string(), "true".to_string());
    filters.insert("volume_num_min".to_string(), "1000".to_string());

    provider.load_all(Some(&filters)).await.unwrap();

    assert_eq!(provider.store().count(), 2);
    assert!(provider.store().is_initialized());
}

#[rstest]
fn test_build_gamma_params_from_hashmap() {
    let mut map = HashMap::new();
    map.insert("active".to_string(), "true".to_string());
    map.insert("closed".to_string(), "false".to_string());
    map.insert("volume_num_min".to_string(), "1000.5".to_string());
    map.insert("tag_id".to_string(), "politics".to_string());
    map.insert("order".to_string(), "volume".to_string());
    map.insert("max_markets".to_string(), "50".to_string());

    let params = build_gamma_params_from_hashmap(&map);

    assert_eq!(params.active, Some(true));
    assert_eq!(params.closed, Some(false));
    assert_eq!(params.volume_num_min, Some(1000.5));
    assert_eq!(params.tag_id.as_deref(), Some("politics"));
    assert_eq!(params.order.as_deref(), Some("volume"));
    assert_eq!(params.max_markets, Some(50));
}

#[rstest]
fn test_build_gamma_params_from_empty_hashmap() {
    let map = HashMap::new();
    let params = build_gamma_params_from_hashmap(&map);

    assert!(params.active.is_none());
    assert!(params.closed.is_none());
    assert!(params.volume_num_min.is_none());
}

#[rstest]
#[tokio::test]
async fn test_get_order_optional_empty_body_returns_none() {
    let state = TestServerState::default();
    // single_order_response is None → handler returns empty 200
    let addr = start_mock_server(state.clone()).await;
    let client = create_clob_client(&addr);

    let result = client.get_order_optional("test-order-id").await.unwrap();
    assert!(result.is_none());
}

#[rstest]
#[tokio::test]
async fn test_get_order_optional_null_body_returns_none() {
    let state = TestServerState::default();
    // Store literal JSON null
    *state.single_order_response.lock().await = Some(json!(null));
    let addr = start_mock_server(state.clone()).await;
    let client = create_clob_client(&addr);

    let result = client.get_order_optional("test-order-id").await.unwrap();
    assert!(result.is_none());
}

#[rstest]
#[tokio::test]
async fn test_get_order_optional_valid_json_returns_some() {
    let state = TestServerState::default();
    *state.single_order_response.lock().await = Some(load_json("http_open_order.json"));
    let addr = start_mock_server(state.clone()).await;
    let client = create_clob_client(&addr);

    let result = client.get_order_optional("test-order-id").await.unwrap();
    assert!(result.is_some());
}

#[rstest]
#[tokio::test]
async fn test_get_order_empty_body_returns_error() {
    let state = TestServerState::default();
    // single_order_response is None → handler returns empty 200
    let addr = start_mock_server(state.clone()).await;
    let client = create_clob_client(&addr);

    let result = client.get_order("test-order-id").await;
    assert!(result.is_err());
}

fn make_data_api_trade(asset: &str, price: f64, timestamp: i64, tx_suffix: &str) -> Value {
    json!({
        "asset": asset,
        "conditionId": "0xcondition_test",
        "side": "BUY",
        "price": price,
        "size": 10.0,
        "timestamp": timestamp,
        "transactionHash": format!("0x{tx_suffix:0>66}")
    })
}

#[rstest]
#[tokio::test]
async fn test_request_trade_ticks_paginates_multiple_pages() {
    let state = TestServerState::default();
    let token = "token_aaa";
    let condition_id = "0xcondition_test";

    // Enqueue 8 trades total. With limit=Some(5), page_size=5.
    // Request 1: offset=0, limit=5 → 5 trades (full page → continues)
    // Request 2: offset=5, limit=5 → 3 trades (partial → stops)
    // Total raw: 8, after token_id filter: 8 (all match)
    let mut trades = Vec::new();
    for i in 0..8u32 {
        trades.push(make_data_api_trade(
            token,
            0.50 + (i as f64) * 0.01,
            1710000008 - i as i64,
            &format!("aaa{i}"),
        ));
    }
    state
        .data_api_trade_pages
        .lock()
        .await
        .push_back(Value::Array(trades));

    let addr = start_mock_server(state.clone()).await;
    let client = create_data_api_client(&addr);

    let ticks = client
        .request_trade_ticks(
            InstrumentId::from("0xcondition_test-token_aaa.POLYMARKET"),
            condition_id,
            token,
            2,
            2,
            Some(5), // page_size = min(5, 500) = 5
        )
        .await
        .unwrap();

    // 8 trades across 2 pages, truncated to limit 5, reversed to chronological
    assert_eq!(ticks.len(), 5);
    for i in 1..ticks.len() {
        assert!(ticks[i - 1].ts_event <= ticks[i].ts_event);
    }
}

#[rstest]
#[tokio::test]
async fn test_request_trade_ticks_limit_truncation() {
    let state = TestServerState::default();
    let token = "token_bbb";
    let condition_id = "0xcondition_test";

    // Page 1: 3 trades (full page when limit=3), limit target=3 → truncate after page 1
    state.data_api_trade_pages.lock().await.extend([json!([
        make_data_api_trade(token, 0.60, 1710000003, "bbb3"),
        make_data_api_trade(token, 0.59, 1710000002, "bbb2"),
        make_data_api_trade(token, 0.58, 1710000001, "bbb1"),
    ])]);

    let addr = start_mock_server(state.clone()).await;
    let client = create_data_api_client(&addr);

    let ticks = client
        .request_trade_ticks(
            InstrumentId::from("0xcondition_test-token_bbb.POLYMARKET"),
            condition_id,
            token,
            2,
            2,
            Some(3),
        )
        .await
        .unwrap();

    assert_eq!(ticks.len(), 3);
}

#[rstest]
#[tokio::test]
async fn test_request_trade_ticks_single_page_partial() {
    let state = TestServerState::default();
    let token = "token_ccc";
    let condition_id = "0xcondition_test";

    // Single partial page → no second request
    state
        .data_api_trade_pages
        .lock()
        .await
        .extend([json!([make_data_api_trade(
            token, 0.70, 1710000001, "ccc1"
        ),])]);

    let addr = start_mock_server(state.clone()).await;
    let client = create_data_api_client(&addr);

    let ticks = client
        .request_trade_ticks(
            InstrumentId::from("0xcondition_test-token_ccc.POLYMARKET"),
            condition_id,
            token,
            2,
            2,
            None,
        )
        .await
        .unwrap();

    assert_eq!(ticks.len(), 1);
}

#[rstest]
#[tokio::test]
async fn test_request_trade_ticks_empty_response() {
    let state = TestServerState::default();
    // No pages enqueued → handler returns empty array
    let addr = start_mock_server(state.clone()).await;
    let client = create_data_api_client(&addr);

    let ticks = client
        .request_trade_ticks(
            InstrumentId::from("0xcondition_test-token_ddd.POLYMARKET"),
            "0xcondition_test",
            "token_ddd",
            2,
            2,
            None,
        )
        .await
        .unwrap();

    assert!(ticks.is_empty());
}
