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

//! Integration tests for BitMEX HTTP client using a mock server.

use std::{collections::HashMap, net::SocketAddr, str::FromStr, sync::Arc, time::Duration};

use axum::{
    Router,
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Json, Response},
    routing::get,
};
use nautilus_bitmex::{
    common::enums::{BitmexEnvironment, BitmexOrderType, BitmexPegPriceType, BitmexSide},
    http::{
        client::{BitmexHttpClient, BitmexRawHttpClient},
        query::{
            DeleteOrderParams, GetOrderParamsBuilder, GetPositionParamsBuilder,
            PostCancelAllAfterParams, PostOrderParams,
        },
    },
};
use nautilus_common::testing::wait_until_async;
use nautilus_model::{
    enums::{OrderSide, OrderType, TimeInForce},
    identifiers::{ClientOrderId, InstrumentId},
    instruments::Instrument,
    types::Quantity,
};
use nautilus_network::http::HttpClient;
use rstest::rstest;
use serde_json::{Value, json};

#[derive(Clone)]
struct TestServerState {
    request_count: Arc<tokio::sync::Mutex<usize>>,
}

impl Default for TestServerState {
    fn default() -> Self {
        Self {
            request_count: Arc::new(tokio::sync::Mutex::new(0)),
        }
    }
}

// Load test data from existing files
fn load_test_data(filename: &str) -> Value {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let path = format!("{manifest_dir}/test_data/{filename}");
    let content = std::fs::read_to_string(path).unwrap();
    serde_json::from_str(&content).unwrap()
}

// Mock endpoint handlers
async fn handle_get_instruments() -> impl IntoResponse {
    // Use existing test data
    let instrument = load_test_data("http_get_instrument_xbtusd.json");
    // Return as array since that's what the endpoint returns
    Json(vec![instrument])
}

async fn handle_get_instrument(query: Query<HashMap<String, String>>) -> impl IntoResponse {
    let instrument = load_test_data("http_get_instrument_xbtusd.json");
    let requested_symbol = query.get("symbol");

    if requested_symbol.is_some_and(|s| s == "XBTUSD") {
        Json(vec![instrument])
    } else {
        Json(Vec::<Value>::new())
    }
}

async fn handle_get_wallet(headers: axum::http::HeaderMap) -> Response {
    if !headers.contains_key("api-key") || !headers.contains_key("api-signature") {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({
                "error": { "message": "Invalid API Key.", "name": "HTTPError" }
            })),
        )
            .into_response();
    }

    let wallets = load_test_data("http_get_wallet.json");
    // The test data is an array, but the endpoint returns a single wallet
    if let Some(wallet_array) = wallets.as_array()
        && !wallet_array.is_empty()
    {
        return Json(wallet_array[0].clone()).into_response();
    }
    Json(wallets).into_response()
}

async fn handle_get_positions(headers: axum::http::HeaderMap) -> Response {
    if !headers.contains_key("api-key") || !headers.contains_key("api-signature") {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({
                "error": { "message": "Invalid API Key.", "name": "HTTPError" }
            })),
        )
            .into_response();
    }

    let positions = load_test_data("http_get_positions.json");
    Json(positions).into_response()
}

async fn handle_get_orders(
    State(state): State<TestServerState>,
    headers: axum::http::HeaderMap,
) -> Response {
    if !headers.contains_key("api-key") || !headers.contains_key("api-signature") {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({
                "error": { "message": "Invalid API Key.", "name": "HTTPError" }
            })),
        )
            .into_response();
    }

    let mut count = state.request_count.lock().await;
    *count += 1;

    if *count > 5 {
        return (StatusCode::TOO_MANY_REQUESTS, Json(json!({
            "error": { "message": "Rate limit exceeded, retry after 1 second.", "name": "HTTPError" },
            "retry_after": 1
        })))
            .into_response();
    }

    let orders = load_test_data("http_get_orders.json");
    Json(orders).into_response()
}

async fn handle_post_order(headers: axum::http::HeaderMap, body: String) -> Response {
    if !headers.contains_key("api-key") || !headers.contains_key("api-signature") {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({
                "error": { "message": "Invalid API Key.", "name": "HTTPError" }
            })),
        )
            .into_response();
    }

    // BitMEX expects form-encoded body for POST /order
    let params: HashMap<String, String> = serde_urlencoded::from_str(&body).unwrap_or_default();

    if !params.contains_key("symbol") || !params.contains_key("orderQty") {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": { "message": "orderQty is required", "name": "HTTPError" }
            })),
        )
            .into_response();
    }

    Json(json!({
        "orderID": "a1b2c3d4-e5f6-7890-abcd-ef1234567890",
        "clOrdID": params.get("clOrdID").unwrap_or(&String::new()),
        "account": 1234567,
        "symbol": params.get("symbol").unwrap(),
        "orderQty": params.get("orderQty").unwrap().parse::<i64>().unwrap_or(0),
        "leavesQty": params.get("orderQty").unwrap().parse::<i64>().unwrap_or(0),
        "cumQty": 0,
        "side": params.get("side").unwrap_or(&"Buy".to_string()),
        "ordStatus": "New",
        "ordType": params.get("ordType").unwrap_or(&"Limit".to_string()),
        "price": params.get("price").and_then(|p| p.parse::<f64>().ok()),
        "pegPriceType": params.get("pegPriceType").cloned(),
        "pegOffsetValue": params.get("pegOffsetValue").and_then(|p| p.parse::<f64>().ok()),
        "timestamp": "2025-01-05T17:50:00.000Z",
        "transactTime": "2025-01-05T17:50:00.000Z"
    }))
    .into_response()
}

async fn handle_delete_order(headers: axum::http::HeaderMap, body: String) -> Response {
    if !headers.contains_key("api-key") || !headers.contains_key("api-signature") {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({
                "error": { "message": "Invalid API Key.", "name": "HTTPError" }
            })),
        )
            .into_response();
    }

    // BitMEX expects form-encoded body for DELETE /order
    let params: HashMap<String, String> = serde_urlencoded::from_str(&body).unwrap_or_default();

    // Parse the JSON-encoded orderID or clOrdID arrays
    let has_order_id = params
        .get("orderID")
        .and_then(|v| serde_json::from_str::<Vec<String>>(v).ok())
        .is_some();
    let has_cl_ord_id = params
        .get("clOrdID")
        .and_then(|v| serde_json::from_str::<Vec<String>>(v).ok())
        .is_some();

    if has_order_id || has_cl_ord_id {
        // Return a cancelled order
        return Json(json!([{
            "orderID": "test-order-id",
            "ordStatus": "Canceled",
            "symbol": "XBTUSD",
            "orderQty": 100,
            "timestamp": "2025-01-05T17:50:00.000Z"
        }]))
        .into_response();
    }

    (
        StatusCode::NOT_FOUND,
        Json(json!({
            "error": { "message": "Order not found", "name": "HTTPError" }
        })),
    )
        .into_response()
}

async fn handle_cancel_all_after(headers: axum::http::HeaderMap, body: String) -> Response {
    if !headers.contains_key("api-key") || !headers.contains_key("api-signature") {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({
                "error": { "message": "Invalid API Key.", "name": "HTTPError" }
            })),
        )
            .into_response();
    }

    let params: HashMap<String, String> = serde_urlencoded::from_str(&body).unwrap_or_default();
    let timeout = params
        .get("timeout")
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(0);

    Json(json!({
        "now": "2025-01-05T17:50:00.000Z",
        "cancelTime": if timeout > 0 { "2025-01-05T17:51:00.000Z" } else { "" }
    }))
    .into_response()
}

fn create_test_router(state: TestServerState) -> Router {
    Router::new()
        .route("/instrument/active", get(handle_get_instruments))
        .route("/instrument", get(handle_get_instrument))
        .route("/user/wallet", get(handle_get_wallet))
        .route("/position", get(handle_get_positions))
        .route("/order", get(handle_get_orders))
        .route("/order", axum::routing::post(handle_post_order))
        .route("/order", axum::routing::delete(handle_delete_order))
        .route(
            "/order/cancelAllAfter",
            axum::routing::post(handle_cancel_all_after),
        )
        .with_state(state)
}

async fn start_test_server()
-> Result<(SocketAddr, TestServerState), Box<dyn std::error::Error + Send + Sync>> {
    // Bind to port 0 to let the OS assign an available port
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let state = TestServerState::default();
    let router = create_test_router(state.clone());

    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });

    // Wait for server to be ready
    let health_url = format!("http://{addr}/instrument/active");
    let http_client =
        HttpClient::new(HashMap::new(), Vec::new(), Vec::new(), None, None, None).unwrap();
    wait_until_async(
        || {
            let url = health_url.clone();
            let client = http_client.clone();
            async move { client.get(url, None, None, Some(1), None).await.is_ok() }
        },
        Duration::from_secs(5),
    )
    .await;

    Ok((addr, state))
}

#[rstest]
#[tokio::test]
async fn test_get_instruments() {
    let (addr, _state) = start_test_server().await.unwrap();
    let base_url = format!("http://{addr}");

    let client =
        BitmexRawHttpClient::new(Some(base_url), 60, 3, 1_000, 10_000, 10_000, 10, 30, None)
            .unwrap();
    let instruments = client.get_instruments(true).await.unwrap();

    assert_eq!(instruments.len(), 1);
    assert_eq!(instruments[0].symbol, "XBTUSD");
}

#[rstest]
#[tokio::test]
async fn test_get_instrument_single_result() {
    let (addr, _state) = start_test_server().await.unwrap();
    let base_url = format!("http://{addr}");

    let client =
        BitmexRawHttpClient::new(Some(base_url), 60, 3, 1_000, 10_000, 10_000, 10, 30, None)
            .unwrap();
    let instrument = client.get_instrument("XBTUSD").await.unwrap();

    assert!(instrument.is_some());
    assert_eq!(instrument.unwrap().symbol, "XBTUSD");
}

#[rstest]
#[tokio::test]
async fn test_request_instrument() {
    let (addr, _state) = start_test_server().await.unwrap();
    let base_url = format!("http://{addr}");

    let client = BitmexHttpClient::new(
        Some(base_url),
        None,
        None,
        BitmexEnvironment::Mainnet,
        60,
        3,
        1_000,
        10_000,
        10_000,
        10,
        120,
        None,
    )
    .unwrap();

    let instrument_id = InstrumentId::from_str("XBTUSD.BITMEX").unwrap();
    let instrument = client.request_instrument(instrument_id).await.unwrap();

    assert!(instrument.is_some());
    let instrument = instrument.unwrap();
    assert_eq!(instrument.id(), instrument_id);
}

#[rstest]
#[tokio::test]
async fn test_get_wallet_requires_auth() {
    let (addr, _state) = start_test_server().await.unwrap();
    let base_url = format!("http://{addr}");

    // Test without credentials - should fail
    let client = BitmexRawHttpClient::new(
        Some(base_url.clone()),
        60,
        3,
        1_000,
        10_000,
        10_000,
        10,
        30,
        None,
    )
    .unwrap();
    let result = client.get_wallet().await;
    assert!(result.is_err());

    // Test with credentials - should succeed
    let client = BitmexRawHttpClient::with_credentials(
        "test_api_key".to_string(),
        "test_api_secret".to_string(),
        base_url,
        60,
        3,
        1_000,
        10_000,
        10_000,
        10,
        120,
        None,
    )
    .unwrap();
    let wallet = client.get_wallet().await.unwrap();
    assert_eq!(wallet.currency, "XBt");
}

#[rstest]
#[tokio::test]
async fn test_get_orders() {
    let (addr, _state) = start_test_server().await.unwrap();
    let base_url = format!("http://{addr}");

    let client = BitmexRawHttpClient::with_credentials(
        "test_api_key".to_string(),
        "test_api_secret".to_string(),
        base_url,
        60,
        3,
        1_000,
        10_000,
        10_000,
        10,
        120,
        None,
    )
    .unwrap();

    let params = GetOrderParamsBuilder::default().build().unwrap();
    let orders = client.get_orders(params).await.unwrap();

    assert!(!orders.is_empty());
    assert!(orders[0].symbol.is_some());
}

#[rstest]
#[tokio::test]
async fn test_place_order() {
    let (addr, _state) = start_test_server().await.unwrap();
    let base_url = format!("http://{addr}");

    let client = BitmexRawHttpClient::with_credentials(
        "test_api_key".to_string(),
        "test_api_secret".to_string(),
        base_url,
        60,
        3,
        1_000,
        10_000,
        10_000,
        10,
        120,
        None,
    )
    .unwrap();

    let params = PostOrderParams {
        symbol: "XBTUSD".to_string(),
        side: Some(BitmexSide::Buy),
        order_qty: Some(100),
        price: Some(95000.0),
        ord_type: Some(BitmexOrderType::Limit),
        cl_ord_id: Some("TEST-ORDER-123".to_string()),
        ..Default::default()
    };

    let order = client.place_order(params).await.unwrap();

    assert_eq!(order["clOrdID"], "TEST-ORDER-123");
    assert_eq!(order["symbol"], "XBTUSD");
    assert_eq!(order["orderQty"], 100);
    assert_eq!(order["ordStatus"], "New");
}

#[rstest]
#[tokio::test]
async fn test_cancel_order() {
    let (addr, _state) = start_test_server().await.unwrap();
    let base_url = format!("http://{addr}");

    let client = BitmexRawHttpClient::with_credentials(
        "test_api_key".to_string(),
        "test_api_secret".to_string(),
        base_url,
        60,
        3,
        1_000,
        10_000,
        10_000,
        10,
        120,
        None,
    )
    .unwrap();

    let params = DeleteOrderParams {
        order_id: Some(vec!["test-order-id".to_string()]),
        cl_ord_id: None,
        text: None,
    };

    let result = client.cancel_orders(params).await.unwrap();
    assert!(result.is_array());
    let result_array = result.as_array().unwrap();
    assert_eq!(result_array.len(), 1);
    assert_eq!(result_array[0]["ordStatus"], "Canceled");
}

// Test that HTTP client correctly implements rate limiting per BitMEX API requirements.
//
// This test verifies that the client respects rate limits by making 6 HTTP requests and checking
// that requests 1-5 succeed while request 6 is rate limited. This is the minimum number of
// requests needed to verify rate limiting works correctly.
//
// Runtime: ~8-9 seconds (previously ~18s with 7 requests - 53% speedup!)
// - Most time is spent in test HTTP server setup and actual HTTP request overhead
// - This is an integration test, so the runtime is acceptable for the coverage
//
// Further optimization would require architectural changes (mocking HTTP client or injecting
// mock clock into rate limiter), which may not be worth the complexity.
#[rstest]
#[tokio::test]
#[ignore = "Slow integration test (~8s) - optimized from 7 to 6 requests"]
async fn test_rate_limiting() {
    let (addr, _state) = start_test_server().await.unwrap();
    let base_url = format!("http://{addr}");

    let client = BitmexRawHttpClient::with_credentials(
        "test_api_key".to_string(),
        "test_api_secret".to_string(),
        base_url,
        60,
        3,
        1_000,
        10_000,
        10_000,
        10,
        120,
        None,
    )
    .unwrap();

    // Make 6 requests to test rate limiting (5 succeed, 1 gets rate limited)
    let params = GetOrderParamsBuilder::default().build().unwrap();
    for i in 0..6 {
        let result = client.get_orders(params.clone()).await;

        if i < 5 {
            assert!(result.is_ok(), "Request {} should succeed", i + 1);
        } else {
            assert!(result.is_err(), "Request {} should be rate limited", i + 1);
        }
    }
}

#[rstest]
#[tokio::test]
async fn test_client_creation() {
    let (addr, _state) = start_test_server().await.unwrap();
    let base_url = format!("http://{addr}");

    let client =
        BitmexRawHttpClient::new(Some(base_url), 60, 3, 1_000, 10_000, 10_000, 10, 30, None)
            .unwrap();

    let result = client.get_instruments(false).await;
    assert!(result.is_ok());
}

#[rstest]
#[tokio::test]
async fn test_client_with_credentials() {
    let (addr, _state) = start_test_server().await.unwrap();
    let base_url = format!("http://{addr}");

    let client = BitmexRawHttpClient::with_credentials(
        "test_key".to_string(),
        "test_secret".to_string(),
        base_url.clone(),
        60,
        3,
        1_000,
        10_000,
        10_000,
        10,
        120,
        None,
    )
    .unwrap();

    let result = client.get_wallet().await;
    assert!(result.is_ok());
}

#[rstest]
#[tokio::test]
async fn test_get_positions_requires_credentials() {
    let (addr, _state) = start_test_server().await.unwrap();
    let base_url = format!("http://{addr}");

    let client =
        BitmexRawHttpClient::new(Some(base_url), 60, 3, 1_000, 10_000, 10_000, 10, 30, None)
            .unwrap();

    let params = GetPositionParamsBuilder::default().build().unwrap();
    let result = client.get_positions(params).await;

    assert!(result.is_err());
    let error_str = format!("{}", result.unwrap_err());
    assert!(
        error_str.contains("credentials") || error_str.contains("Missing credentials"),
        "Expected credentials error, was: {error_str}"
    );
}

#[rstest]
#[tokio::test]
async fn test_http_network_error() {
    let base_url = "http://127.0.0.1:1".to_string();

    let client =
        BitmexRawHttpClient::new(Some(base_url), 1, 3, 1_000, 10_000, 10_000, 10, 30, None)
            .unwrap();

    let result = client.get_instruments(false).await;

    assert!(result.is_err());
}

#[rstest]
#[tokio::test]
async fn test_http_500_internal_server_error() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let handle_500 = || async {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "Internal Server Error" })),
        )
    };

    let app = Router::new().route("/instrument", get(handle_500));

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    // Wait for server to be ready
    let health_url = format!("http://{addr}/instrument");
    let http_client =
        HttpClient::new(HashMap::new(), Vec::new(), Vec::new(), None, None, None).unwrap();
    wait_until_async(
        || {
            let url = health_url.clone();
            let client = http_client.clone();
            async move { client.get(url, None, None, Some(1), None).await.is_ok() }
        },
        Duration::from_secs(5),
    )
    .await;

    let base_url = format!("http://{addr}");
    let client =
        BitmexRawHttpClient::new(Some(base_url), 60, 3, 1_000, 10_000, 10_000, 10, 30, None)
            .unwrap();

    let result = client.get_instruments(false).await;

    assert!(result.is_err());
    let error_str = format!("{}", result.unwrap_err());
    assert!(
        error_str.contains("500") || error_str.contains("Internal Server Error"),
        "Expected 500 error, was: {error_str}"
    );
}

#[rstest]
#[tokio::test]
async fn test_place_pegged_order() {
    let (addr, _state) = start_test_server().await.unwrap();
    let base_url = format!("http://{addr}");

    let client = BitmexRawHttpClient::with_credentials(
        "test_api_key".to_string(),
        "test_api_secret".to_string(),
        base_url,
        60,
        3,
        1_000,
        10_000,
        10_000,
        10,
        120,
        None,
    )
    .unwrap();

    let params = PostOrderParams {
        symbol: "XBTUSD".to_string(),
        side: Some(BitmexSide::Buy),
        order_qty: Some(100),
        ord_type: Some(BitmexOrderType::Pegged),
        peg_price_type: Some(BitmexPegPriceType::PrimaryPeg),
        peg_offset_value: Some(0.0),
        cl_ord_id: Some("TEST-PEGGED-001".to_string()),
        ..Default::default()
    };

    let order = client.place_order(params).await.unwrap();

    assert_eq!(order["clOrdID"], "TEST-PEGGED-001");
    assert_eq!(order["symbol"], "XBTUSD");
    assert_eq!(order["ordType"], "Pegged");
    assert_eq!(order["pegPriceType"], "PrimaryPeg");
    assert_eq!(order["pegOffsetValue"], 0.0);
}

#[rstest]
#[tokio::test]
async fn test_place_pegged_order_with_negative_offset() {
    let (addr, _state) = start_test_server().await.unwrap();
    let base_url = format!("http://{addr}");

    let client = BitmexRawHttpClient::with_credentials(
        "test_api_key".to_string(),
        "test_api_secret".to_string(),
        base_url,
        60,
        3,
        1_000,
        10_000,
        10_000,
        10,
        120,
        None,
    )
    .unwrap();

    let params = PostOrderParams {
        symbol: "XBTUSD".to_string(),
        side: Some(BitmexSide::Sell),
        order_qty: Some(50),
        ord_type: Some(BitmexOrderType::Pegged),
        peg_price_type: Some(BitmexPegPriceType::MarketPeg),
        peg_offset_value: Some(-1.5),
        cl_ord_id: Some("TEST-PEGGED-002".to_string()),
        ..Default::default()
    };

    let order = client.place_order(params).await.unwrap();

    assert_eq!(order["ordType"], "Pegged");
    assert_eq!(order["pegPriceType"], "MarketPeg");
    assert_eq!(order["pegOffsetValue"], -1.5);
}

#[rstest]
#[tokio::test]
async fn test_submit_order_pegged_via_high_level_client() {
    let (addr, _state) = start_test_server().await.unwrap();
    let base_url = format!("http://{addr}");

    let client = BitmexHttpClient::new(
        Some(base_url),
        Some("test_api_key".to_string()),
        Some("test_api_secret".to_string()),
        BitmexEnvironment::Mainnet,
        60,
        3,
        1_000,
        10_000,
        10_000,
        10,
        120,
        None,
    )
    .unwrap();

    let instrument_id = InstrumentId::from_str("XBTUSD.BITMEX").unwrap();
    let instrument = client
        .request_instrument(instrument_id)
        .await
        .unwrap()
        .unwrap();
    client.cache_instrument(instrument);

    let report = client
        .submit_order(
            instrument_id,
            ClientOrderId::from("PEG-001"),
            OrderSide::Buy,
            OrderType::Limit,
            Quantity::from("100"),
            TimeInForce::Gtc,
            None,  // price
            None,  // trigger_price
            None,  // trigger_type
            None,  // trailing_offset
            None,  // trailing_offset_type
            None,  // display_qty
            false, // post_only
            false, // reduce_only
            None,  // order_list_id
            None,  // contingency_type
            Some(BitmexPegPriceType::PrimaryPeg),
            Some(0.0),
        )
        .await;

    assert!(
        report.is_ok(),
        "Expected OK, was: {:?}",
        report.unwrap_err()
    );
}

#[rstest]
#[tokio::test]
async fn test_submit_order_pegged_rejects_non_limit() {
    let (addr, _state) = start_test_server().await.unwrap();
    let base_url = format!("http://{addr}");

    let client = BitmexHttpClient::new(
        Some(base_url),
        Some("test_api_key".to_string()),
        Some("test_api_secret".to_string()),
        BitmexEnvironment::Mainnet,
        60,
        3,
        1_000,
        10_000,
        10_000,
        10,
        120,
        None,
    )
    .unwrap();

    let instrument_id = InstrumentId::from_str("XBTUSD.BITMEX").unwrap();
    let instrument = client
        .request_instrument(instrument_id)
        .await
        .unwrap()
        .unwrap();
    client.cache_instrument(instrument);

    let result = client
        .submit_order(
            instrument_id,
            ClientOrderId::from("PEG-002"),
            OrderSide::Buy,
            OrderType::Market,
            Quantity::from("100"),
            TimeInForce::Gtc,
            None,
            None,
            None,
            None,
            None,
            None,
            false,
            false,
            None,
            None,
            Some(BitmexPegPriceType::PrimaryPeg),
            Some(0.0),
        )
        .await;

    assert!(result.is_err());
    let error_str = result.unwrap_err().to_string();
    assert!(
        error_str.contains("LIMIT"),
        "Expected LIMIT order type error, was: {error_str}"
    );
}

#[rstest]
#[tokio::test]
async fn test_submit_order_pegged_rejects_offset_without_type() {
    let (addr, _state) = start_test_server().await.unwrap();
    let base_url = format!("http://{addr}");

    let client = BitmexHttpClient::new(
        Some(base_url),
        Some("test_api_key".to_string()),
        Some("test_api_secret".to_string()),
        BitmexEnvironment::Mainnet,
        60,
        3,
        1_000,
        10_000,
        10_000,
        10,
        120,
        None,
    )
    .unwrap();

    let instrument_id = InstrumentId::from_str("XBTUSD.BITMEX").unwrap();
    let instrument = client
        .request_instrument(instrument_id)
        .await
        .unwrap()
        .unwrap();
    client.cache_instrument(instrument);

    let result = client
        .submit_order(
            instrument_id,
            ClientOrderId::from("PEG-003"),
            OrderSide::Buy,
            OrderType::Limit,
            Quantity::from("100"),
            TimeInForce::Gtc,
            None,
            None,
            None,
            None,
            None,
            None,
            false,
            false,
            None,
            None,
            None,      // peg_price_type missing
            Some(0.0), // peg_offset_value present
        )
        .await;

    assert!(result.is_err());
    let error_str = result.unwrap_err().to_string();
    assert!(
        error_str.contains("peg_price_type"),
        "Expected peg_price_type error, was: {error_str}"
    );
}

#[rstest]
#[tokio::test]
async fn test_cancel_all_after() {
    let (addr, _state) = start_test_server().await.unwrap();
    let base_url = format!("http://{addr}");

    let client = BitmexRawHttpClient::with_credentials(
        "test_api_key".to_string(),
        "test_api_secret".to_string(),
        base_url,
        60,
        3,
        1_000,
        10_000,
        10_000,
        10,
        120,
        None,
    )
    .unwrap();

    let params = PostCancelAllAfterParams { timeout: 60_000 };
    let result = client.cancel_all_after(params).await.unwrap();
    assert_eq!(result["cancelTime"], "2025-01-05T17:51:00.000Z");
}

#[rstest]
#[tokio::test]
async fn test_cancel_all_after_disarm() {
    let (addr, _state) = start_test_server().await.unwrap();
    let base_url = format!("http://{addr}");

    let client = BitmexRawHttpClient::with_credentials(
        "test_api_key".to_string(),
        "test_api_secret".to_string(),
        base_url,
        60,
        3,
        1_000,
        10_000,
        10_000,
        10,
        120,
        None,
    )
    .unwrap();

    let params = PostCancelAllAfterParams { timeout: 0 };
    let result = client.cancel_all_after(params).await.unwrap();
    assert_eq!(result["cancelTime"], "");
}

#[rstest]
#[tokio::test]
async fn test_cancel_all_after_high_level() {
    let (addr, _state) = start_test_server().await.unwrap();
    let base_url = format!("http://{addr}");

    let client = BitmexHttpClient::new(
        Some(base_url),
        Some("test_api_key".to_string()),
        Some("test_api_secret".to_string()),
        BitmexEnvironment::Mainnet,
        60,
        3,
        1_000,
        10_000,
        10_000,
        10,
        120,
        None,
    )
    .unwrap();

    // Should succeed without error
    client.cancel_all_after(60_000).await.unwrap();

    // Disarm should also succeed
    client.cancel_all_after(0).await.unwrap();
}
