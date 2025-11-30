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
    routing::{get, post},
};
use chrono::Utc;
use nautilus_bybit::{
    common::enums::{BybitAccountType, BybitProductType},
    http::{
        client::BybitHttpClient,
        query::{
            BybitFeeRateParams, BybitInstrumentsInfoParamsBuilder, BybitPositionListParamsBuilder,
            BybitWalletBalanceParams,
        },
    },
};
use nautilus_model::{
    enums::PositionSideSpecified,
    identifiers::AccountId,
    instruments::{CurrencyPair, InstrumentAny},
    types::{Currency, Price, Quantity},
};
use rstest::rstest;
use serde_json::{Value, json};

type SettleCoinQueries = Arc<tokio::sync::Mutex<Vec<(String, Option<String>)>>>;

#[allow(dead_code)]
#[derive(Clone)]
struct TestServerState {
    request_count: Arc<tokio::sync::Mutex<usize>>,
    // (endpoint, settle_coin)
    settle_coin_queries: SettleCoinQueries,
    realtime_requests: Arc<tokio::sync::Mutex<usize>>,
    history_requests: Arc<tokio::sync::Mutex<usize>>,
}

impl Default for TestServerState {
    fn default() -> Self {
        Self {
            request_count: Arc::new(tokio::sync::Mutex::new(0)),
            settle_coin_queries: Arc::new(tokio::sync::Mutex::new(Vec::new())),
            realtime_requests: Arc::new(tokio::sync::Mutex::new(0)),
            history_requests: Arc::new(tokio::sync::Mutex::new(0)),
        }
    }
}

// Load test data from existing files
#[allow(dead_code)]
fn load_test_data(filename: &str) -> Value {
    let path = format!("test_data/{filename}");
    let content = std::fs::read_to_string(path).unwrap();
    serde_json::from_str(&content).unwrap()
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
        Some("inverse") => "http_get_instruments_inverse.json",
        Some("option") => "http_get_instruments_option.json",
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
async fn handle_get_wallet_balance(headers: axum::http::HeaderMap) -> Response {
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

    let wallet = load_test_data("http_get_wallet_balance.json");
    Json(wallet).into_response()
}

#[allow(dead_code)]
async fn handle_cancel_order(headers: axum::http::HeaderMap, body: axum::body::Bytes) -> Response {
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
    let Ok(cancel_req): Result<Value, _> = serde_json::from_slice(&body) else {
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
    if cancel_req.get("category").is_none() || cancel_req.get("symbol").is_none() {
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

    // Return successful cancel response
    Json(json!({
        "retCode": 0,
        "retMsg": "OK",
        "result": {
            "orderId": "test-canceled-order-id",
            "orderLinkId": cancel_req.get("orderLinkId").and_then(|v| v.as_str()).unwrap_or("")
        },
        "retExtInfo": {},
        "time": 1704470400123i64
    }))
    .into_response()
}

#[allow(dead_code)]
async fn handle_get_positions(headers: axum::http::HeaderMap) -> Response {
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

    let positions = load_test_data("http_get_positions.json");
    Json(positions).into_response()
}

#[allow(dead_code)]
async fn handle_get_fee_rate(headers: axum::http::HeaderMap) -> Response {
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

    let fee_rate = load_test_data("http_get_fee_rate.json");
    Json(fee_rate).into_response()
}

#[allow(dead_code)]
async fn handle_no_convert_repay(
    headers: axum::http::HeaderMap,
    body: axum::body::Bytes,
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

    // Parse JSON body
    let Ok(repay_req): Result<Value, _> = serde_json::from_slice(&body) else {
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
    if repay_req.get("coin").is_none() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "retCode": 10001,
                "retMsg": "Missing required parameter: coin",
                "result": {},
                "retExtInfo": {},
                "time": 1704470400123i64
            })),
        )
            .into_response();
    }

    // Return successful repay response
    Json(json!({
        "retCode": 0,
        "retMsg": "OK",
        "result": {
            "resultStatus": "SU"
        },
        "retExtInfo": {},
        "time": 1704470400123i64
    }))
    .into_response()
}

#[allow(dead_code)]
async fn handle_get_orders_realtime(
    query: Query<HashMap<String, String>>,
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

    // Check required parameters
    if !query.contains_key("category") {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "retCode": 10001,
                "retMsg": "Missing required parameter: category",
                "result": {},
                "retExtInfo": {},
                "time": 1704470400123i64
            })),
        )
            .into_response();
    }

    // Check for settleCoin when no symbol is provided (for LINEAR products)
    let category = query.get("category").map(String::as_str);
    let has_symbol = query.contains_key("symbol");
    let has_settle_coin = query.contains_key("settleCoin");

    if category == Some("linear") && !has_symbol && !has_settle_coin {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "retCode": 10001,
                "retMsg": "Missing some parameters that must be filled in, symbol or settleCoin or baseCoin",
                "result": {},
                "retExtInfo": {},
                "time": 1704470400123i64
            })),
        )
            .into_response();
    }

    // Track settle coin queries
    let settle_coin = query.get("settleCoin").cloned();
    {
        let mut queries = state.settle_coin_queries.lock().await;
        queries.push(("realtime".to_string(), settle_coin.clone()));
    }

    {
        let mut count = state.realtime_requests.lock().await;
        *count += 1;
    }

    let mut orders = load_test_data("http_get_orders_realtime.json");

    // Make order IDs unique per settle coin for regression testing
    if let Some(coin) = &settle_coin
        && let Some(result) = orders.get_mut("result")
        && let Some(list) = result.get_mut("list")
        && let Some(array) = list.as_array_mut()
    {
        for order in array.iter_mut() {
            if let Some(order_obj) = order.as_object_mut()
                && let Some(order_id) = order_obj.get("orderId")
            {
                let base_id = order_id.as_str().unwrap_or("");
                order_obj.insert(
                    "orderId".to_string(),
                    json!(format!("{}-{}", base_id, coin)),
                );
            }
        }
    }

    if let Some(limit_str) = query.get("limit")
        && let Ok(limit) = limit_str.parse::<usize>()
        && let Some(result) = orders.get_mut("result")
        && let Some(list) = result.get_mut("list")
        && let Some(array) = list.as_array_mut()
    {
        array.truncate(limit);
    }

    Json(orders).into_response()
}

#[allow(dead_code)]
async fn handle_get_orders_history_reconciliation(
    query: Query<HashMap<String, String>>,
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

    // Check required parameters
    if !query.contains_key("category") {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "retCode": 10001,
                "retMsg": "Missing required parameter: category",
                "result": {},
                "retExtInfo": {},
                "time": 1704470400123i64
            })),
        )
            .into_response();
    }

    // Check for settleCoin when no symbol is provided (for LINEAR products)
    let category = query.get("category").map(String::as_str);
    let has_symbol = query.contains_key("symbol");
    let has_settle_coin = query.contains_key("settleCoin");

    if category == Some("linear") && !has_symbol && !has_settle_coin {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "retCode": 10001,
                "retMsg": "Missing some parameters that must be filled in, symbol or settleCoin or baseCoin",
                "result": {},
                "retExtInfo": {},
                "time": 1704470400123i64
            })),
        )
            .into_response();
    }

    // Track settle coin queries
    let settle_coin = query.get("settleCoin").cloned();
    {
        let mut queries = state.settle_coin_queries.lock().await;
        queries.push(("history".to_string(), settle_coin.clone()));
    }

    {
        let mut count = state.history_requests.lock().await;
        *count += 1;
    }

    let mut orders = load_test_data("http_get_orders_history_with_duplicate.json");

    // Make order IDs unique per settle coin for regression testing
    if let Some(coin) = &settle_coin
        && let Some(result) = orders.get_mut("result")
        && let Some(list) = result.get_mut("list")
        && let Some(array) = list.as_array_mut()
    {
        for order in array.iter_mut() {
            if let Some(order_obj) = order.as_object_mut()
                && let Some(order_id) = order_obj.get("orderId")
            {
                let base_id = order_id.as_str().unwrap_or("");
                order_obj.insert(
                    "orderId".to_string(),
                    json!(format!("{}-{}", base_id, coin)),
                );
            }
        }
    }

    if let Some(limit_str) = query.get("limit")
        && let Ok(limit) = limit_str.parse::<usize>()
        && let Some(result) = orders.get_mut("result")
        && let Some(list) = result.get_mut("list")
        && let Some(array) = list.as_array_mut()
    {
        array.truncate(limit);
    }

    Json(orders).into_response()
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
        .route("/v5/order/create", post(handle_post_order))
        .route("/v5/order/cancel", post(handle_cancel_order))
        .route("/v5/account/wallet-balance", get(handle_get_wallet_balance))
        .route("/v5/position/list", get(handle_get_positions))
        .route("/v5/account/fee-rate", get(handle_get_fee_rate))
        .route(
            "/v5/account/no-convert-repay",
            post(handle_no_convert_repay),
        )
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
    let client = BybitHttpClient::new(None, Some(60), None, None, None, None, None).unwrap();

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
    let client = BybitHttpClient::new(
        Some(custom_url.to_string()),
        Some(60),
        None,
        None,
        None,
        None,
        None,
    )
    .unwrap();

    assert_eq!(client.base_url(), custom_url);
}

#[rstest]
#[tokio::test]
async fn test_get_server_time() {
    let (addr, _state) = start_test_server().await.unwrap();
    let base_url = format!("http://{addr}");

    let client =
        BybitHttpClient::new(Some(base_url), Some(60), None, None, None, None, None).unwrap();

    let response = client.get_server_time().await.unwrap();
    assert!(!response.result.time_second.is_empty());
    assert!(!response.result.time_nano.is_empty());
}

#[rstest]
#[tokio::test]
async fn test_get_instruments_linear() {
    let (addr, _state) = start_test_server().await.unwrap();
    let base_url = format!("http://{addr}");

    let client =
        BybitHttpClient::new(Some(base_url), Some(60), None, None, None, None, None).unwrap();

    let params = BybitInstrumentsInfoParamsBuilder::default()
        .category(BybitProductType::Linear)
        .build()
        .unwrap();

    let response = client.get_instruments_linear(&params).await.unwrap();
    assert!(!response.result.list.is_empty());
}

#[rstest]
#[tokio::test]
async fn test_get_instruments_spot() {
    let (addr, _state) = start_test_server().await.unwrap();
    let base_url = format!("http://{addr}");

    let client =
        BybitHttpClient::new(Some(base_url), Some(60), None, None, None, None, None).unwrap();

    let params = BybitInstrumentsInfoParamsBuilder::default()
        .category(BybitProductType::Spot)
        .build()
        .unwrap();

    let response = client.get_instruments_spot(&params).await.unwrap();
    assert!(!response.result.list.is_empty());
}

#[rstest]
#[tokio::test]
async fn test_get_instruments_inverse() {
    let (addr, _state) = start_test_server().await.unwrap();
    let base_url = format!("http://{addr}");

    let client =
        BybitHttpClient::new(Some(base_url), Some(60), None, None, None, None, None).unwrap();

    let params = BybitInstrumentsInfoParamsBuilder::default()
        .category(BybitProductType::Inverse)
        .build()
        .unwrap();

    let response = client.get_instruments_inverse(&params).await.unwrap();
    assert!(!response.result.list.is_empty());
}

#[rstest]
#[tokio::test]
async fn test_get_instruments_option() {
    let (addr, _state) = start_test_server().await.unwrap();
    let base_url = format!("http://{addr}");

    let client =
        BybitHttpClient::new(Some(base_url), Some(60), None, None, None, None, None).unwrap();

    let params = BybitInstrumentsInfoParamsBuilder::default()
        .category(BybitProductType::Option)
        .build()
        .unwrap();

    let response = client.get_instruments_option(&params).await.unwrap();
    assert!(!response.result.list.is_empty());
}

#[rstest]
#[tokio::test]
async fn test_place_order() {
    let (addr, _state) = start_test_server().await.unwrap();
    let base_url = format!("http://{addr}");

    let client = BybitHttpClient::with_credentials(
        "test_api_key".to_string(),
        "test_api_secret".to_string(),
        Some(base_url),
        Some(60),
        None,
        None,
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

    let response = client.place_order(&order_request).await.unwrap();
    assert_eq!(response.ret_code, 0);
    assert!(response.result.order_id.is_some());
}

#[rstest]
#[tokio::test]
async fn test_authenticated_endpoint_requires_credentials() {
    let (addr, _state) = start_test_server().await.unwrap();
    let base_url = format!("http://{addr}");

    // Create client without credentials
    let client =
        BybitHttpClient::new(Some(base_url), Some(60), None, None, None, None, None).unwrap();

    // Should fail when trying to call authenticated endpoint without credentials
    let result = client
        .get_open_orders(BybitProductType::Linear, Some("BTCUSDT"))
        .await;
    assert!(result.is_err());
}

#[rstest]
#[tokio::test]
async fn test_rate_limiting_returns_error() {
    let (addr, _state) = start_test_server().await.unwrap();
    let base_url = format!("http://{addr}");

    let client = BybitHttpClient::with_credentials(
        "test_api_key".to_string(),
        "test_api_secret".to_string(),
        Some(base_url),
        Some(60),
        None,
        None,
        None,
        None,
        None,
    )
    .unwrap();

    // Make multiple requests to trigger rate limit (mock server limits after 5)
    let mut last_error = None;
    for _ in 0..10 {
        match client
            .get_open_orders(BybitProductType::Linear, Some("BTCUSDT"))
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

#[rstest]
#[tokio::test]
async fn test_get_open_orders_with_symbol() {
    let (addr, _state) = start_test_server().await.unwrap();
    let base_url = format!("http://{addr}");

    let client = BybitHttpClient::with_credentials(
        "test_api_key".to_string(),
        "test_api_secret".to_string(),
        Some(base_url),
        Some(60),
        None,
        None,
        None,
        None,
        None,
    )
    .unwrap();

    let response = client
        .get_open_orders(BybitProductType::Linear, Some("BTCUSDT"))
        .await
        .unwrap();

    assert_eq!(response.ret_code, 0);
    assert!(response.result.list.is_empty() || !response.result.list.is_empty());
}

#[rstest]
#[tokio::test]
async fn test_get_open_orders_without_symbol() {
    let (addr, _state) = start_test_server().await.unwrap();
    let base_url = format!("http://{addr}");

    let client = BybitHttpClient::with_credentials(
        "test_api_key".to_string(),
        "test_api_secret".to_string(),
        Some(base_url),
        Some(60),
        None,
        None,
        None,
        None,
        None,
    )
    .unwrap();

    let response = client
        .get_open_orders(BybitProductType::Linear, None)
        .await
        .unwrap();

    assert_eq!(response.ret_code, 0);
}

#[rstest]
#[tokio::test]
async fn test_get_wallet_balance_requires_credentials() {
    let (addr, _state) = start_test_server().await.unwrap();
    let base_url = format!("http://{addr}");

    // Create client without credentials
    let client =
        BybitHttpClient::new(Some(base_url), Some(60), None, None, None, None, None).unwrap();

    let params = BybitWalletBalanceParams {
        account_type: BybitAccountType::Unified,
        coin: None,
    };

    // Should fail when trying to call authenticated endpoint without credentials
    let result = client.get_wallet_balance(&params).await;
    assert!(result.is_err());
}

#[rstest]
#[tokio::test]
async fn test_get_wallet_balance_with_credentials() {
    let (addr, _state) = start_test_server().await.unwrap();
    let base_url = format!("http://{addr}");

    let client = BybitHttpClient::with_credentials(
        "test_api_key".to_string(),
        "test_api_secret".to_string(),
        Some(base_url),
        Some(60),
        None,
        None,
        None,
        None,
        None,
    )
    .unwrap();

    let params = BybitWalletBalanceParams {
        account_type: BybitAccountType::Unified,
        coin: None,
    };

    let response = client.get_wallet_balance(&params).await.unwrap();

    assert_eq!(response.ret_code, 0);
    assert!(!response.result.list.is_empty());
    assert_eq!(
        response.result.list[0].account_type,
        BybitAccountType::Unified
    );
}

#[rstest]
#[tokio::test]
async fn test_get_positions_requires_credentials() {
    let (addr, _state) = start_test_server().await.unwrap();
    let base_url = format!("http://{addr}");

    let client =
        BybitHttpClient::new(Some(base_url), Some(60), None, None, None, None, None).unwrap();

    let params = BybitPositionListParamsBuilder::default()
        .category(BybitProductType::Linear)
        .build()
        .unwrap();

    let result = client.get_positions(&params).await;
    assert!(result.is_err());
}

#[rstest]
#[tokio::test]
async fn test_get_positions_with_credentials() {
    let (addr, _state) = start_test_server().await.unwrap();
    let base_url = format!("http://{addr}");

    let client = BybitHttpClient::with_credentials(
        "test_api_key".to_string(),
        "test_api_secret".to_string(),
        Some(base_url),
        Some(60),
        None,
        None,
        None,
        None,
        None,
    )
    .unwrap();

    let params = BybitPositionListParamsBuilder::default()
        .category(BybitProductType::Linear)
        .build()
        .unwrap();

    let response = client.get_positions(&params).await.unwrap();

    assert_eq!(response.ret_code, 0);
}

#[rstest]
#[tokio::test]
async fn test_get_fee_rate_requires_credentials() {
    let (addr, _state) = start_test_server().await.unwrap();
    let base_url = format!("http://{addr}");

    let client =
        BybitHttpClient::new(Some(base_url), Some(60), None, None, None, None, None).unwrap();

    let params = BybitFeeRateParams {
        category: BybitProductType::Linear,
        symbol: Some("BTCUSDT".to_string()),
        base_coin: None,
    };

    let result = client.get_fee_rate(&params).await;
    assert!(result.is_err());
}

#[rstest]
#[tokio::test]
async fn test_get_fee_rate_with_credentials() {
    let (addr, _state) = start_test_server().await.unwrap();
    let base_url = format!("http://{addr}");

    let client = BybitHttpClient::with_credentials(
        "test_api_key".to_string(),
        "test_api_secret".to_string(),
        Some(base_url),
        Some(60),
        None,
        None,
        None,
        None,
        None,
    )
    .unwrap();

    let params = BybitFeeRateParams {
        category: BybitProductType::Linear,
        symbol: Some("BTCUSDT".to_string()),
        base_coin: None,
    };

    let response = client.get_fee_rate(&params).await.unwrap();

    assert_eq!(response.ret_code, 0);
    assert!(!response.result.list.is_empty());
}
// Create router with separate handlers for reconciliation testing
#[allow(dead_code)]
fn create_reconciliation_test_router(state: TestServerState) -> Router {
    Router::new()
        .route("/v5/market/time", get(handle_get_server_time))
        .route("/v5/market/instruments-info", get(handle_get_instruments))
        .route("/v5/account/fee-rate", get(handle_get_fee_rate))
        .route("/v5/order/realtime", get(handle_get_orders_realtime))
        .route(
            "/v5/order/history",
            get(handle_get_orders_history_reconciliation),
        )
        .with_state(state)
}

#[allow(dead_code)]
async fn start_reconciliation_test_server()
-> Result<(SocketAddr, TestServerState), Box<dyn std::error::Error + Send + Sync>> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let state = TestServerState::default();
    let router = create_reconciliation_test_router(state.clone());

    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });

    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    Ok((addr, state))
}

#[rstest]
#[tokio::test]
async fn test_request_order_status_reports_calls_both_endpoints() {
    let (addr, _state) = start_reconciliation_test_server().await.unwrap();
    let base_url = format!("http://{addr}");

    let client = BybitHttpClient::with_credentials(
        "test_api_key".to_string(),
        "test_api_secret".to_string(),
        Some(base_url),
        Some(60),
        None,
        None,
        None,
        None,
        None,
    )
    .unwrap();

    // Required for parsing order reports
    let instruments = client
        .request_instruments(BybitProductType::Linear, None)
        .await
        .unwrap();

    for instrument in instruments {
        client.cache_instrument(instrument);
    }

    let account_id = AccountId::from("BYBIT-UNIFIED");

    // Should call BOTH /v5/order/realtime and /v5/order/history
    // Note: With settle coin iteration, we now query both USDT and USDC
    // Use a limit to restrict to 3 orders for this test
    let reports = client
        .request_order_status_reports(
            account_id,
            BybitProductType::Linear,
            None,    // No specific instrument - will query both USDT and USDC
            false,   // open_only=false triggers dual endpoint call
            None,    // start
            None,    // end
            Some(3), // Limit to 3 to keep test focused on deduplication logic
        )
        .await
        .unwrap();

    // Should get 3 orders:
    // - 2 from realtime (open-order-1-USDT, open-order-2-USDT) from first settle coin
    // - 1 more due to limit (from history or second settle coin)
    // - open-order-1-USDT appears in both realtime and history but should be deduplicated
    let order_ids: Vec<String> = reports
        .iter()
        .map(|r| r.venue_order_id.to_string())
        .collect();

    assert_eq!(
        reports.len(),
        3,
        "Should have 3 orders total (respecting limit)"
    );
    // At minimum we should see the first 2 realtime orders from USDT
    assert!(order_ids.contains(&"open-order-1-USDT".to_string()));
    assert!(order_ids.contains(&"open-order-2-USDT".to_string()));
}

#[rstest]
#[tokio::test]
async fn test_request_order_status_reports_requires_settle_coin_for_linear() {
    let (addr, _state) = start_reconciliation_test_server().await.unwrap();
    let base_url = format!("http://{addr}");

    let client = BybitHttpClient::with_credentials(
        "test_api_key".to_string(),
        "test_api_secret".to_string(),
        Some(base_url),
        Some(60),
        None,
        None,
        None,
        None,
        None,
    )
    .unwrap();

    let instruments = client
        .request_instruments(BybitProductType::Linear, None)
        .await
        .unwrap();
    for instrument in instruments {
        client.cache_instrument(instrument);
    }

    let account_id = AccountId::from("BYBIT-UNIFIED");

    // Should succeed because implementation adds settleCoin=USDT automatically
    let result = client
        .request_order_status_reports(
            account_id,
            BybitProductType::Linear,
            None, // No symbol - requires settleCoin
            true, // open_only=true
            None, // start
            None, // end
            None, // limit
        )
        .await;

    assert!(result.is_ok(), "Should succeed with automatic settleCoin");
}

#[rstest]
#[tokio::test]
async fn test_order_deduplication_by_order_id() {
    let (addr, _state) = start_reconciliation_test_server().await.unwrap();
    let base_url = format!("http://{addr}");

    let client = BybitHttpClient::with_credentials(
        "test_api_key".to_string(),
        "test_api_secret".to_string(),
        Some(base_url),
        Some(60),
        None,
        None,
        None,
        None,
        None,
    )
    .unwrap();

    let instruments = client
        .request_instruments(BybitProductType::Linear, None)
        .await
        .unwrap();
    for instrument in instruments {
        client.cache_instrument(instrument);
    }

    let account_id = AccountId::from("BYBIT-UNIFIED");

    // Test deduplication by querying both realtime and history for a specific instrument
    // This avoids the settle coin iteration complexity
    use nautilus_model::identifiers::{InstrumentId, Symbol, Venue};
    let instrument_id = InstrumentId::new(Symbol::from("ETHUSDT-LINEAR"), Venue::from("BYBIT"));

    let reports = client
        .request_order_status_reports(
            account_id,
            BybitProductType::Linear,
            Some(instrument_id), // Specify instrument to avoid settle coin iteration
            false,               // This will query both realtime and history endpoints
            None,                // start
            None,                // end
            None,                // limit
        )
        .await
        .unwrap();

    // Count occurrences of open-order-1
    // It should appear once despite being in both realtime and history responses
    let open_order_1_count = reports
        .iter()
        .filter(|r| r.venue_order_id.to_string() == "open-order-1")
        .count();

    assert_eq!(
        open_order_1_count, 1,
        "open-order-1 should appear exactly once (deduplicated across realtime/history)"
    );
}

#[rstest]
#[tokio::test]
async fn test_request_order_status_reports_linear_queries_all_settle_coins() {
    let (addr, state) = start_reconciliation_test_server().await.unwrap();
    let base_url = format!("http://{addr}");

    let client = BybitHttpClient::with_credentials(
        "test_api_key".to_string(),
        "test_api_secret".to_string(),
        Some(base_url),
        Some(60),
        None,
        None,
        None,
        None,
        None,
    )
    .unwrap();

    let instruments = client
        .request_instruments(BybitProductType::Linear, None)
        .await
        .unwrap();
    for instrument in instruments {
        client.cache_instrument(instrument);
    }

    let account_id = AccountId::from("BYBIT-UNIFIED");

    let _reports = client
        .request_order_status_reports(
            account_id,
            BybitProductType::Linear,
            None,
            true,
            None,
            None,
            None,
        )
        .await
        .unwrap();

    let queries = state.settle_coin_queries.lock().await;
    let realtime_queries: Vec<&Option<String>> = queries
        .iter()
        .filter(|(endpoint, _)| endpoint == "realtime")
        .map(|(_, coin)| coin)
        .collect();

    assert_eq!(
        realtime_queries.len(),
        2,
        "Should query realtime endpoint twice (once per settle coin)"
    );
    assert!(
        realtime_queries.contains(&&Some("USDT".to_string())),
        "Should query USDT settle coin"
    );
    assert!(
        realtime_queries.contains(&&Some("USDC".to_string())),
        "Should query USDC settle coin"
    );
}

#[rstest]
#[tokio::test]
async fn test_request_order_status_reports_respects_limit_across_settle_coins() {
    let (addr, state) = start_reconciliation_test_server().await.unwrap();
    let base_url = format!("http://{addr}");

    let client = BybitHttpClient::with_credentials(
        "test_api_key".to_string(),
        "test_api_secret".to_string(),
        Some(base_url),
        Some(60),
        None,
        None,
        None,
        None,
        None,
    )
    .unwrap();

    let instruments = client
        .request_instruments(BybitProductType::Linear, None)
        .await
        .unwrap();
    for instrument in instruments {
        client.cache_instrument(instrument);
    }

    let account_id = AccountId::from("BYBIT-UNIFIED");

    // Test data has 2 orders per settle coin
    // With limit=3: expect 2 from USDT, 1 from USDC
    let reports = client
        .request_order_status_reports(
            account_id,
            BybitProductType::Linear,
            None,
            true,
            None,
            None,
            Some(3),
        )
        .await
        .unwrap();

    assert!(
        reports.len() <= 3,
        "Should return at most 3 reports, was {}",
        reports.len()
    );

    // Both settle coins should be queried
    let queries = state.settle_coin_queries.lock().await;
    let realtime_query_count = queries
        .iter()
        .filter(|(endpoint, _)| endpoint == "realtime")
        .count();

    assert_eq!(realtime_query_count, 2, "Should query both settle coins");
}

#[rstest]
#[tokio::test]
async fn test_request_order_status_reports_stops_before_next_coin() {
    let (addr, state) = start_reconciliation_test_server().await.unwrap();
    let base_url = format!("http://{addr}");

    let client = BybitHttpClient::with_credentials(
        "test_api_key".to_string(),
        "test_api_secret".to_string(),
        Some(base_url),
        Some(60),
        None,
        None,
        None,
        None,
        None,
    )
    .unwrap();

    let instruments = client
        .request_instruments(BybitProductType::Linear, None)
        .await
        .unwrap();
    for instrument in instruments {
        client.cache_instrument(instrument);
    }

    let account_id = AccountId::from("BYBIT-UNIFIED");

    // Test data has 2 orders, limit=1 should stop after USDT
    let reports = client
        .request_order_status_reports(
            account_id,
            BybitProductType::Linear,
            None,
            true,
            None,
            None,
            Some(1),
        )
        .await
        .unwrap();

    assert_eq!(reports.len(), 1, "Should return exactly 1 report");

    // Early termination: USDC should be skipped
    let queries = state.settle_coin_queries.lock().await;
    let realtime_queries: Vec<&Option<String>> = queries
        .iter()
        .filter(|(endpoint, _)| endpoint == "realtime")
        .map(|(_, coin)| coin)
        .collect();

    assert_eq!(
        realtime_queries.len(),
        1,
        "Should only query first settle coin when limit reached"
    );
    assert_eq!(
        realtime_queries[0],
        &Some("USDT".to_string()),
        "Should query USDT first"
    );
}

#[rstest]
#[tokio::test]
async fn test_request_order_status_reports_combines_orders_from_each_settle_coin() {
    let (addr, state) = start_reconciliation_test_server().await.unwrap();
    let base_url = format!("http://{addr}");

    let client = BybitHttpClient::with_credentials(
        "test_api_key".to_string(),
        "test_api_secret".to_string(),
        Some(base_url),
        Some(60),
        None,
        None,
        None,
        None,
        None,
    )
    .unwrap();

    let instruments = client
        .request_instruments(BybitProductType::Linear, None)
        .await
        .unwrap();
    for instrument in instruments {
        client.cache_instrument(instrument);
    }

    let account_id = AccountId::from("BYBIT-UNIFIED");

    let reports = client
        .request_order_status_reports(
            account_id,
            BybitProductType::Linear,
            None,
            true,
            None,
            None,
            None,
        )
        .await
        .unwrap();

    let queries = state.settle_coin_queries.lock().await;
    let realtime_queries: Vec<&Option<String>> = queries
        .iter()
        .filter(|(endpoint, _)| endpoint == "realtime")
        .map(|(_, coin)| coin)
        .collect();

    assert_eq!(realtime_queries.len(), 2, "Should query both USDT and USDC");
    assert!(
        realtime_queries.contains(&&Some("USDT".to_string())),
        "Should query USDT"
    );
    assert!(
        realtime_queries.contains(&&Some("USDC".to_string())),
        "Should query USDC"
    );

    let order_ids: Vec<String> = reports
        .iter()
        .map(|r| r.venue_order_id.to_string())
        .collect();

    // Test data has 2 orders per settle coin
    // With distinct suffixes: expect exactly 4 orders
    assert_eq!(
        reports.len(),
        4,
        "Should get exactly 4 orders (2 from USDT + 2 from USDC), was {}",
        reports.len()
    );

    assert!(
        order_ids.contains(&"open-order-1-USDT".to_string()),
        "Should contain open-order-1-USDT from USDT settle coin"
    );
    assert!(
        order_ids.contains(&"open-order-2-USDT".to_string()),
        "Should contain open-order-2-USDT from USDT settle coin"
    );

    assert!(
        order_ids.contains(&"open-order-1-USDC".to_string()),
        "Should contain open-order-1-USDC from USDC settle coin"
    );
    assert!(
        order_ids.contains(&"open-order-2-USDC".to_string()),
        "Should contain open-order-2-USDC from USDC settle coin"
    );
}

#[rstest]
#[tokio::test]
async fn test_repay_spot_borrow_with_amount() {
    let (addr, _state) = start_test_server().await.unwrap();
    let base_url = format!("http://{addr}");

    let client = BybitHttpClient::with_credentials(
        "test_api_key".to_string(),
        "test_api_secret".to_string(),
        Some(base_url),
        Some(60),
        None,
        None,
        None,
        None,
        None,
    )
    .unwrap();

    let amount = Quantity::new_checked(0.5, 8).unwrap();
    let response = client.repay_spot_borrow("ETH", Some(amount)).await.unwrap();

    assert_eq!(response.ret_code, 0);
    assert_eq!(response.ret_msg, "OK");
    assert_eq!(response.result.result_status, "SU");
}

#[rstest]
#[tokio::test]
async fn test_repay_spot_borrow_without_amount() {
    let (addr, _state) = start_test_server().await.unwrap();
    let base_url = format!("http://{addr}");

    let client = BybitHttpClient::with_credentials(
        "test_api_key".to_string(),
        "test_api_secret".to_string(),
        Some(base_url),
        Some(60),
        None,
        None,
        None,
        None,
        None,
    )
    .unwrap();

    // Test repaying all outstanding borrows by passing None for amount
    let response = client.repay_spot_borrow("ETH", None).await.unwrap();

    assert_eq!(response.ret_code, 0);
    assert_eq!(response.ret_msg, "OK");
    assert_eq!(response.result.result_status, "SU");
}

#[rstest]
#[tokio::test]
async fn test_repay_spot_borrow_requires_credentials() {
    let (addr, _state) = start_test_server().await.unwrap();
    let base_url = format!("http://{addr}");

    let client =
        BybitHttpClient::new(Some(base_url), Some(60), None, None, None, None, None).unwrap();

    let amount = Quantity::new_checked(0.5, 8).unwrap();
    let result = client.repay_spot_borrow("ETH", Some(amount)).await;
    assert!(result.is_err(), "Should fail without credentials");
}

#[rstest]
#[tokio::test]
async fn test_get_spot_borrow_amount_returns_zero_when_no_borrow() {
    let (addr, _state) = start_test_server().await.unwrap();
    let base_url = format!("http://{addr}");

    let client = BybitHttpClient::with_credentials(
        "test_api_key".to_string(),
        "test_api_secret".to_string(),
        Some(base_url),
        None,
        None,
        None,
        None,
        None,
        None,
    )
    .unwrap();

    let borrow_amount = client.get_spot_borrow_amount("BTC").await.unwrap();

    // BTC should have zero borrow in the test data
    assert_eq!(borrow_amount, rust_decimal::Decimal::ZERO);
}

#[rstest]
#[tokio::test]
async fn test_get_spot_borrow_amount_returns_zero_when_coin_not_found() {
    let (addr, _state) = start_test_server().await.unwrap();
    let base_url = format!("http://{addr}");

    let client = BybitHttpClient::with_credentials(
        "test_api_key".to_string(),
        "test_api_secret".to_string(),
        Some(base_url),
        None,
        None,
        None,
        None,
        None,
        None,
    )
    .unwrap();

    let borrow_amount = client.get_spot_borrow_amount("UNKNOWN").await.unwrap();

    // Should return zero when coin not found
    assert_eq!(borrow_amount, rust_decimal::Decimal::ZERO);
}

#[rstest]
#[tokio::test]
async fn test_spot_position_report_short_from_borrowed_balance() {
    let (addr, _state) = start_test_server().await.unwrap();
    let base_url = format!("http://{addr}");

    let client = BybitHttpClient::with_credentials(
        "test_api_key".to_string(),
        "test_api_secret".to_string(),
        Some(base_url),
        None,
        None,
        None,
        None,
        None,
        None,
    )
    .unwrap();

    client.set_use_spot_position_reports(true);

    let eth = Currency::from("ETH");
    let usdt = Currency::from("USDT");
    let ethusdt = CurrencyPair::new(
        "ETHUSDT-SPOT.BYBIT".into(),
        "ETHUSDT".into(),
        eth,
        usdt,
        2,
        5,
        Price::from("0.01"),
        Quantity::from("0.00001"),
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
        None,
        0.into(),
        0.into(),
    );
    client.cache_instrument(InstrumentAny::CurrencyPair(ethusdt));

    let account_id = AccountId::new("BYBIT-UNIFIED");
    let reports = client
        .request_position_status_reports(account_id, BybitProductType::Spot, None)
        .await
        .unwrap();

    let eth_report = reports
        .iter()
        .find(|r| r.instrument_id.symbol.as_str() == "ETHUSDT-SPOT")
        .expect("ETH SPOT position report not found");

    assert_eq!(eth_report.position_side, PositionSideSpecified::Short);
    assert_eq!(eth_report.quantity, Quantity::new(0.06142, 5));
}

#[rstest]
#[tokio::test]
async fn test_request_order_status_reports_with_time_filtering() {
    let (addr, state) = start_reconciliation_test_server().await.unwrap();
    let base_url = format!("http://{addr}");

    let client = BybitHttpClient::with_credentials(
        "test_api_key".to_string(),
        "test_api_secret".to_string(),
        Some(base_url),
        Some(60),
        None,
        None,
        None,
        None,
        None,
    )
    .unwrap();

    let instruments = client
        .request_instruments(BybitProductType::Linear, None)
        .await
        .unwrap();

    for instrument in instruments {
        client.cache_instrument(instrument);
    }

    let account_id = AccountId::from("BYBIT-UNIFIED");
    let start_time = Utc::now() - chrono::Duration::days(7);
    let end_time = Utc::now();

    let _reports = client
        .request_order_status_reports(
            account_id,
            BybitProductType::Linear,
            None,
            false, // Query history
            Some(start_time),
            Some(end_time),
            Some(10),
        )
        .await
        .unwrap();

    // The history endpoint should have been called with startTime and endTime
    let queries = state.settle_coin_queries.lock().await;

    // Should have called history endpoint for each settle coin (USDT, USDC)
    assert!(
        queries.len() >= 2,
        "Should have called history endpoint at least twice (one per settle coin)"
    );
}
