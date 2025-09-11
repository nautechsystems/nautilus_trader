// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

use std::{collections::HashMap, net::SocketAddr, sync::Arc};

use axum::{
    Router,
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Json, Response},
    routing::get,
};
use nautilus_bitmex::{
    common::enums::{BitmexOrderType, BitmexSide},
    http::{
        client::BitmexHttpInnerClient,
        query::{DeleteOrderParams, GetOrderParamsBuilder, PostOrderParams},
    },
};
use rstest::rstest;
use serde_json::{Value, json};
use tokio::sync::Mutex;

#[derive(Clone)]
struct TestServerState {
    request_count: Arc<Mutex<usize>>,
}

impl Default for TestServerState {
    fn default() -> Self {
        Self {
            request_count: Arc::new(Mutex::new(0)),
        }
    }
}

// Load test data from existing files
fn load_test_data(filename: &str) -> Value {
    let path = format!("test_data/{}", filename);
    let content = std::fs::read_to_string(path).expect("Failed to read test data");
    serde_json::from_str(&content).expect("Failed to parse test data")
}

// Mock endpoint handlers
async fn handle_get_instruments() -> impl IntoResponse {
    // Use existing test data
    let instrument = load_test_data("http_get_instrument_xbtusd.json");
    // Return as array since that's what the endpoint returns
    Json(vec![instrument])
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
    if let Some(wallet_array) = wallets.as_array() {
        if !wallet_array.is_empty() {
            return Json(wallet_array[0].clone()).into_response();
        }
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

async fn handle_post_order(
    headers: axum::http::HeaderMap,
    Query(params): Query<HashMap<String, String>>,
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

    // BitMEX uses query parameters for POST /order
    if !params.contains_key("symbol") || !params.contains_key("orderQty") {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": { "message": "orderQty is required", "name": "HTTPError" }
            })),
        )
            .into_response();
    }

    // Create a mock order response
    Json(json!({
        "orderID": "new-order-id-12345",
        "clOrdID": params.get("clOrdID").unwrap_or(&"".to_string()),
        "symbol": params.get("symbol").unwrap(),
        "orderQty": params.get("orderQty").unwrap().parse::<i64>().unwrap_or(0),
        "side": params.get("side").unwrap_or(&"Buy".to_string()),
        "ordStatus": "New",
        "ordType": params.get("ordType").unwrap_or(&"Limit".to_string()),
        "price": params.get("price").and_then(|p| p.parse::<f64>().ok()),
        "timestamp": "2025-01-05T17:50:00.000Z",
        "transactTime": "2025-01-05T17:50:00.000Z"
    }))
    .into_response()
}

async fn handle_delete_order(
    headers: axum::http::HeaderMap,
    Query(params): Query<HashMap<String, String>>,
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

    // BitMEX uses query parameters for DELETE /order
    if params.contains_key("orderID") || params.contains_key("clOrdID") {
        // Return a cancelled order
        return Json(json!([{
            "orderID": params.get("orderID").unwrap_or(&"test-order-id".to_string()),
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

fn create_test_router(state: TestServerState) -> Router {
    Router::new()
        .route("/instrument/active", get(handle_get_instruments))
        .route("/user/wallet", get(handle_get_wallet))
        .route("/position", get(handle_get_positions))
        .route("/order", get(handle_get_orders))
        .route("/order", axum::routing::post(handle_post_order))
        .route("/order", axum::routing::delete(handle_delete_order))
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

    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    Ok((addr, state))
}

#[rstest]
#[tokio::test]
async fn test_get_instruments() {
    let (addr, _state) = start_test_server().await.unwrap();
    let base_url = format!("http://{}", addr);

    let client = BitmexHttpInnerClient::new(Some(base_url), Some(60), None, None, None).unwrap();
    let instruments = client.http_get_instruments(true).await.unwrap();

    assert_eq!(instruments.len(), 1);
    assert_eq!(instruments[0].symbol, "XBTUSD");
}

#[rstest]
#[tokio::test]
async fn test_get_wallet_requires_auth() {
    let (addr, _state) = start_test_server().await.unwrap();
    let base_url = format!("http://{}", addr);

    // Test without credentials - should fail
    let client =
        BitmexHttpInnerClient::new(Some(base_url.clone()), Some(60), None, None, None).unwrap();
    let result = client.http_get_wallet().await;
    assert!(result.is_err());

    // Test with credentials - should succeed
    let client = BitmexHttpInnerClient::with_credentials(
        "test_api_key".to_string(),
        "test_api_secret".to_string(),
        base_url,
        Some(60),
        None,
        None,
        None,
    )
    .unwrap();
    let wallet = client.http_get_wallet().await.unwrap();
    assert_eq!(wallet.currency, "XBt");
}

#[rstest]
#[tokio::test]
async fn test_get_orders() {
    let (addr, _state) = start_test_server().await.unwrap();
    let base_url = format!("http://{}", addr);

    let client = BitmexHttpInnerClient::with_credentials(
        "test_api_key".to_string(),
        "test_api_secret".to_string(),
        base_url,
        Some(60),
        None,
        None,
        None,
    )
    .unwrap();

    let params = GetOrderParamsBuilder::default().build().unwrap();
    let orders = client.http_get_orders(params).await.unwrap();

    assert!(!orders.is_empty());
    assert!(orders[0].symbol.is_some());
}

#[rstest]
#[tokio::test]
async fn test_place_order() {
    let (addr, _state) = start_test_server().await.unwrap();
    let base_url = format!("http://{}", addr);

    let client = BitmexHttpInnerClient::with_credentials(
        "test_api_key".to_string(),
        "test_api_secret".to_string(),
        base_url,
        Some(60),
        None,
        None,
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

    let order = client.http_place_order(params).await.unwrap();

    assert_eq!(order["clOrdID"], "TEST-ORDER-123");
    assert_eq!(order["symbol"], "XBTUSD");
    assert_eq!(order["orderQty"], 100);
    assert_eq!(order["ordStatus"], "New");
}

#[rstest]
#[tokio::test]
async fn test_cancel_order() {
    let (addr, _state) = start_test_server().await.unwrap();
    let base_url = format!("http://{}", addr);

    let client = BitmexHttpInnerClient::with_credentials(
        "test_api_key".to_string(),
        "test_api_secret".to_string(),
        base_url,
        Some(60),
        None,
        None,
        None,
    )
    .unwrap();

    let params = DeleteOrderParams {
        order_id: Some(vec!["test-order-id".to_string()]),
        cl_ord_id: None,
        text: None,
    };

    let result = client.http_cancel_orders(params).await.unwrap();
    assert!(result.is_array());
    let result_array = result.as_array().unwrap();
    assert_eq!(result_array.len(), 1);
    assert_eq!(result_array[0]["ordStatus"], "Canceled");
}

#[rstest]
#[tokio::test]
#[ignore = "slow test, revisit setup"] // TODO: Improve test setup to avoid slow test run time
async fn test_rate_limiting() {
    let (addr, _state) = start_test_server().await.unwrap();
    let base_url = format!("http://{}", addr);

    let client = BitmexHttpInnerClient::with_credentials(
        "test_api_key".to_string(),
        "test_api_secret".to_string(),
        base_url,
        Some(60),
        None,
        None,
        None,
    )
    .unwrap();

    // Make multiple requests to trigger rate limiting
    let params = GetOrderParamsBuilder::default().build().unwrap();
    for i in 0..7 {
        let result = client.http_get_orders(params.clone()).await;
        if i < 5 {
            assert!(result.is_ok(), "Request {} should succeed", i + 1);
        } else {
            assert!(result.is_err(), "Request {} should be rate limited", i + 1);
        }
    }
}
