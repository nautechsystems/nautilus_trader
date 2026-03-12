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
use nautilus_network::http::HttpClient;
use nautilus_polymarket::{
    common::{credential::Credential, enums::PolymarketOrderType},
    http::{
        clob::PolymarketClobHttpClient,
        gamma::{PolymarketGammaHttpClient, PolymarketGammaRawHttpClient},
        models::PolymarketOrder,
        query::{
            CancelMarketOrdersParams, GetBalanceAllowanceParams, GetGammaMarketsParams,
            GetOrdersParams, GetTradesParams,
        },
    },
    providers::PolymarketInstrumentProvider,
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
    orders_pages: Arc<tokio::sync::Mutex<VecDeque<Value>>>,
    gamma_response: Arc<tokio::sync::Mutex<Option<Value>>>,
    gamma_slug_responses: Arc<tokio::sync::Mutex<AHashMap<String, Value>>>,
    gamma_force_error: Arc<std::sync::atomic::AtomicBool>,
}

impl Default for TestServerState {
    fn default() -> Self {
        Self {
            request_count: Arc::new(tokio::sync::Mutex::new(0)),
            last_body: Arc::new(tokio::sync::Mutex::new(None)),
            last_headers: Arc::new(tokio::sync::Mutex::new(AHashMap::new())),
            rate_limit_after: Arc::new(AtomicUsize::new(usize::MAX)),
            orders_pages: Arc::new(tokio::sync::Mutex::new(VecDeque::new())),
            gamma_response: Arc::new(tokio::sync::Mutex::new(None)),
            gamma_slug_responses: Arc::new(tokio::sync::Mutex::new(AHashMap::new())),
            gamma_force_error: Arc::new(std::sync::atomic::AtomicBool::new(false)),
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
    PolymarketClobHttpClient::new(
        test_credential(),
        TEST_ADDRESS.to_string(),
        Some(format!("http://{addr}")),
        Some(5),
    )
    .unwrap()
}

fn create_gamma_client(addr: &SocketAddr) -> PolymarketGammaRawHttpClient {
    PolymarketGammaRawHttpClient::new(Some(format!("http://{addr}")), Some(5)).unwrap()
}

fn create_gamma_domain_client(addr: &SocketAddr) -> PolymarketGammaHttpClient {
    PolymarketGammaHttpClient::new(Some(format!("http://{addr}")), Some(5)).unwrap()
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

    assert_eq!(balance.balance, rust_decimal_macros::dec!(1000.000000));
    assert_eq!(
        balance.allowance,
        Some(rust_decimal_macros::dec!(999999999.000000))
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
    assert_eq!(markets[0].condition_id, "0xabc123def456789");
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
    assert_eq!(markets[0].condition_id, "0xabc123def456789");
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
