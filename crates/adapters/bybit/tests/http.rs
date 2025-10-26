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

//! Integration tests for Bybit HTTP client using a mock server.

use std::{collections::HashMap, net::SocketAddr, sync::Arc};

use axum::{
    Router,
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Json, Response},
    routing::get,
};
use nautilus_bybit::{
    common::enums::BybitProductType,
    http::{client::BybitHttpClient, query::BybitInstrumentsInfoParamsBuilder},
};
use rstest::rstest;
use serde_json::{Value, json};
use tokio::sync::Mutex;

#[allow(dead_code)]
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
#[allow(dead_code)]
fn load_test_data(filename: &str) -> Value {
    let path = format!("test_data/{}", filename);
    let content = std::fs::read_to_string(path).expect("Failed to read test data");
    serde_json::from_str(&content).expect("Failed to parse test data")
}

// Mock endpoint handlers
#[allow(dead_code)]
async fn handle_get_server_time() -> impl IntoResponse {
    Json(json!({
        "retCode": 0,
        "retMsg": "OK",
        "result": {
            "timeSecond": "1704470400",
            "timeNano": "1704470400123456789"
        },
        "retExtInfo": {},
        "time": 1704470400123i64
    }))
}

#[allow(dead_code)]
async fn handle_get_instruments(query: Query<HashMap<String, String>>) -> impl IntoResponse {
    let category = query.get("category").map(String::as_str);

    let filename = match category {
        Some("linear") => "http_get_instruments_linear.json",
        Some("spot") => "http_get_instruments_spot.json",
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "retCode": 10001,
                    "retMsg": "Invalid category parameter",
                    "result": {},
                    "retExtInfo": {},
                    "time": 1704470400123i64
                })),
            )
                .into_response();
        }
    };

    let instruments = load_test_data(filename);
    Json(instruments).into_response()
}

#[allow(dead_code)]
async fn handle_get_klines(query: Query<HashMap<String, String>>) -> impl IntoResponse {
    if !query.contains_key("category") || !query.contains_key("symbol") {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "retCode": 10001,
                "retMsg": "Missing required parameters",
                "result": {},
                "retExtInfo": {},
                "time": 1704470400123i64
            })),
        )
            .into_response();
    }

    let klines = load_test_data("http_get_klines_linear.json");
    Json(klines).into_response()
}

#[allow(dead_code)]
async fn handle_get_trades(query: Query<HashMap<String, String>>) -> impl IntoResponse {
    if !query.contains_key("category") || !query.contains_key("symbol") {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "retCode": 10001,
                "retMsg": "Missing required parameters",
                "result": {},
                "retExtInfo": {},
                "time": 1704470400123i64
            })),
        )
            .into_response();
    }

    let trades = load_test_data("http_get_trades_recent.json");
    Json(trades).into_response()
}

#[allow(dead_code)]
async fn handle_get_orders(
    State(state): State<TestServerState>,
    headers: axum::http::HeaderMap,
) -> Response {
    // Check for authentication headers
    if !headers.contains_key("X-BAPI-API-KEY")
        || !headers.contains_key("X-BAPI-SIGN")
        || !headers.contains_key("X-BAPI-TIMESTAMP")
    {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({
                "retCode": 10003,
                "retMsg": "Invalid API key",
                "result": {},
                "retExtInfo": {},
                "time": 1704470400123i64
            })),
        )
            .into_response();
    }

    let mut count = state.request_count.lock().await;
    *count += 1;

    // Simulate rate limiting after 5 requests
    if *count > 5 {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            Json(json!({
                "retCode": 10006,
                "retMsg": "Too many requests. Please retry after 1 second.",
                "result": {},
                "retExtInfo": {},
                "time": 1704470400123i64
            })),
        )
            .into_response();
    }

    let orders = load_test_data("http_get_orders_history.json");
    Json(orders).into_response()
}

#[allow(dead_code)]
async fn handle_post_order(headers: axum::http::HeaderMap, body: axum::body::Bytes) -> Response {
    // Check for authentication headers
    if !headers.contains_key("X-BAPI-API-KEY")
        || !headers.contains_key("X-BAPI-SIGN")
        || !headers.contains_key("X-BAPI-TIMESTAMP")
    {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({
                "retCode": 10003,
                "retMsg": "Invalid API key",
                "result": {},
                "retExtInfo": {},
                "time": 1704470400123i64
            })),
        )
            .into_response();
    }

    // Parse JSON body
    let Ok(order_req): Result<Value, _> = serde_json::from_slice(&body) else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "retCode": 10001,
                "retMsg": "Invalid JSON body",
                "result": {},
                "retExtInfo": {},
                "time": 1704470400123i64
            })),
        )
            .into_response();
    };

    // Validate required fields
    if order_req.get("category").is_none()
        || order_req.get("symbol").is_none()
        || order_req.get("side").is_none()
        || order_req.get("orderType").is_none()
        || order_req.get("qty").is_none()
    {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "retCode": 10001,
                "retMsg": "Missing required order parameters",
                "result": {},
                "retExtInfo": {},
                "time": 1704470400123i64
            })),
        )
            .into_response();
    }

    // Return successful order response
    Json(json!({
        "retCode": 0,
        "retMsg": "OK",
        "result": {
            "orderId": "test-order-id-12345",
            "orderLinkId": order_req.get("orderLinkId").and_then(|v| v.as_str()).unwrap_or("")
        },
        "retExtInfo": {},
        "time": 1704470400123i64
    }))
    .into_response()
}

#[allow(dead_code)]
fn create_test_router(state: TestServerState) -> Router {
    Router::new()
        .route("/v5/market/time", get(handle_get_server_time))
        .route("/v5/market/instruments-info", get(handle_get_instruments))
        .route("/v5/market/kline", get(handle_get_klines))
        .route("/v5/market/recent-trade", get(handle_get_trades))
        .route("/v5/order/history", get(handle_get_orders))
        .route("/v5/order/realtime", get(handle_get_orders))
        .route("/v5/order/create", axum::routing::post(handle_post_order))
        .with_state(state)
}

#[allow(dead_code)]
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

    // Give server time to start
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    Ok((addr, state))
}

#[rstest]
#[tokio::test]
async fn test_client_creation() {
    let client = BybitHttpClient::new(None, Some(60), None, None, None).unwrap();

    assert!(client.base_url().contains("bybit.com"));
    assert!(client.credential().is_none());
}

#[rstest]
#[tokio::test]
async fn test_client_with_credentials() {
    let client = BybitHttpClient::with_credentials(
        "test_api_key".to_string(),
        "test_api_secret".to_string(),
        Some("https://api.bybit.com".to_string()),
        Some(60),
        None,
        None,
        None,
    )
    .unwrap();

    assert!(client.credential().is_some());
}

#[rstest]
#[tokio::test]
async fn test_testnet_urls() {
    let client = BybitHttpClient::new(
        Some("https://api-testnet.bybit.com".to_string()),
        Some(60),
        None,
        None,
        None,
    )
    .unwrap();

    assert!(client.base_url().contains("testnet"));
}

#[rstest]
#[tokio::test]
async fn test_custom_base_url() {
    let custom_url = "https://custom.bybit.com";
    let client =
        BybitHttpClient::new(Some(custom_url.to_string()), Some(60), None, None, None).unwrap();

    assert_eq!(client.base_url(), custom_url);
}

#[rstest]
#[tokio::test]
async fn test_get_server_time() {
    let (addr, _state) = start_test_server().await.unwrap();
    let base_url = format!("http://{}", addr);

    let client = BybitHttpClient::new(Some(base_url), Some(60), None, None, None).unwrap();

    let response = client.http_get_server_time().await.unwrap();
    assert!(!response.result.time_second.is_empty());
    assert!(!response.result.time_nano.is_empty());
}

#[rstest]
#[tokio::test]
async fn test_get_instruments_linear() {
    let (addr, _state) = start_test_server().await.unwrap();
    let base_url = format!("http://{}", addr);

    let client = BybitHttpClient::new(Some(base_url), Some(60), None, None, None).unwrap();

    let params = BybitInstrumentsInfoParamsBuilder::default()
        .category(BybitProductType::Linear)
        .build()
        .unwrap();

    let response = client.http_get_instruments_linear(&params).await.unwrap();
    assert!(!response.result.list.is_empty());
}

#[rstest]
#[tokio::test]
async fn test_place_order() {
    let (addr, _state) = start_test_server().await.unwrap();
    let base_url = format!("http://{}", addr);

    let client = BybitHttpClient::with_credentials(
        "test_api_key".to_string(),
        "test_api_secret".to_string(),
        Some(base_url),
        Some(60),
        None,
        None,
        None,
    )
    .unwrap();

    let order_request = serde_json::json!({
        "category": "linear",
        "symbol": "BTCUSDT",
        "side": "Buy",
        "orderType": "Limit",
        "qty": "0.001",
        "price": "50000",
        "orderLinkId": "test-order-123"
    });

    let response = client.http_place_order(&order_request).await.unwrap();
    assert_eq!(response.ret_code, 0);
    assert!(response.result.order_id.is_some());
}

#[rstest]
#[tokio::test]
async fn test_authenticated_endpoint_requires_credentials() {
    let (addr, _state) = start_test_server().await.unwrap();
    let base_url = format!("http://{}", addr);

    // Create client without credentials
    let client = BybitHttpClient::new(Some(base_url), Some(60), None, None, None).unwrap();

    // Should fail when trying to call authenticated endpoint without credentials
    let result = client
        .http_get_open_orders(BybitProductType::Linear, Some("BTCUSDT"))
        .await;
    assert!(result.is_err());
}

#[rstest]
#[tokio::test]
async fn test_rate_limiting_returns_error() {
    let (addr, _state) = start_test_server().await.unwrap();
    let base_url = format!("http://{}", addr);

    let client = BybitHttpClient::with_credentials(
        "test_api_key".to_string(),
        "test_api_secret".to_string(),
        Some(base_url),
        Some(60),
        None,
        None,
        None,
    )
    .unwrap();

    // Make multiple requests to trigger rate limit (mock server limits after 5)
    let mut last_error = None;
    for _ in 0..10 {
        match client
            .http_get_open_orders(BybitProductType::Linear, Some("BTCUSDT"))
            .await
        {
            Ok(_) => continue,
            Err(e) => {
                last_error = Some(e);
                break;
            }
        }
    }

    // Verify rate limit was triggered
    assert!(last_error.is_some());
    let error = last_error.unwrap();
    assert!(error.to_string().contains("10006") || error.to_string().contains("Too many"));
}
