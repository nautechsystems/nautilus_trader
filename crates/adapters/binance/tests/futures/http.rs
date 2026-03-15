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

//! Integration tests for the Binance Futures HTTP client using a mock server.

use std::{
    net::SocketAddr,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use axum::{
    Router,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{delete, get, post},
};
use nautilus_binance::{
    common::enums::{
        BinanceEnvironment, BinanceFuturesOrderType, BinanceProductType, BinanceSide,
        BinanceTimeInForce,
    },
    futures::http::{client::BinanceRawFuturesHttpClient, query::BinanceNewOrderParamsBuilder},
};
use rstest::rstest;
use serde_json::json;

#[derive(Clone)]
struct TestServerState {
    request_count: Arc<AtomicUsize>,
    rate_limit_threshold: usize,
}

impl Default for TestServerState {
    fn default() -> Self {
        Self {
            request_count: Arc::new(AtomicUsize::new(0)),
            rate_limit_threshold: usize::MAX,
        }
    }
}

impl TestServerState {
    fn with_rate_limit(mut self, limit: usize) -> Self {
        self.rate_limit_threshold = limit;
        self
    }

    fn increment_and_check(&self) -> bool {
        self.request_count.fetch_add(1, Ordering::Relaxed) >= self.rate_limit_threshold
    }
}

fn has_auth_headers(headers: &HeaderMap) -> bool {
    headers.contains_key("x-mbx-apikey")
}

fn json_response(body: &serde_json::Value) -> Response {
    (
        StatusCode::OK,
        [("content-type", "application/json")],
        body.to_string(),
    )
        .into_response()
}

fn unauthorized_response() -> Response {
    (
        StatusCode::UNAUTHORIZED,
        [("content-type", "application/json")],
        json!({"code": -2015, "msg": "Invalid API-key, IP, or permissions for action"}).to_string(),
    )
        .into_response()
}

fn rate_limit_response() -> Response {
    (
        StatusCode::TOO_MANY_REQUESTS,
        [("content-type", "application/json")],
        json!({"code": -1015, "msg": "Too many requests"}).to_string(),
    )
        .into_response()
}

fn load_fixture(name: &str) -> serde_json::Value {
    let path = format!(
        "{}/test_data/futures/http_json/{name}",
        env!("CARGO_MANIFEST_DIR")
    );
    let content = std::fs::read_to_string(&path).expect("Failed to read fixture");
    serde_json::from_str(&content).expect("Failed to parse fixture JSON")
}

async fn handle_ping() -> Response {
    json_response(&json!({}))
}

async fn handle_time() -> Response {
    json_response(&json!({"serverTime": 1700000000000_i64}))
}

async fn handle_exchange_info() -> Response {
    json_response(&json!({
        "timezone": "UTC",
        "serverTime": 1700000000000_i64,
        "rateLimits": [],
        "exchangeFilters": [],
        "symbols": [{
            "symbol": "BTCUSDT",
            "pair": "BTCUSDT",
            "contractType": "PERPETUAL",
            "deliveryDate": 4133404800000_i64,
            "onboardDate": 1569398400000_i64,
            "status": "TRADING",
            "baseAsset": "BTC",
            "quoteAsset": "USDT",
            "marginAsset": "USDT",
            "pricePrecision": 2,
            "quantityPrecision": 3,
            "baseAssetPrecision": 8,
            "quotePrecision": 8,
            "underlyingType": "COIN",
            "settlePlan": 0,
            "triggerProtect": "0.0500",
            "filters": [
                {"filterType": "PRICE_FILTER", "minPrice": "0.10", "maxPrice": "1000000", "tickSize": "0.10"},
                {"filterType": "LOT_SIZE", "minQty": "0.001", "maxQty": "1000", "stepSize": "0.001"},
                {"filterType": "MIN_NOTIONAL", "notional": "5"}
            ],
            "orderTypes": ["LIMIT", "MARKET", "STOP", "STOP_MARKET", "TAKE_PROFIT", "TAKE_PROFIT_MARKET", "TRAILING_STOP_MARKET"],
            "timeInForce": ["GTC", "IOC", "FOK", "GTD"]
        }]
    }))
}

async fn handle_depth() -> Response {
    json_response(&json!({
        "lastUpdateId": 1027024,
        "E": 1700000000000_i64,
        "T": 1700000000000_i64,
        "bids": [["50000.00", "1.000"], ["49999.00", "2.000"]],
        "asks": [["50001.00", "0.500"], ["50002.00", "1.500"]]
    }))
}

async fn handle_account(headers: HeaderMap, State(state): State<TestServerState>) -> Response {
    if !has_auth_headers(&headers) {
        return unauthorized_response();
    }

    if state.increment_and_check() {
        return rate_limit_response();
    }
    json_response(&load_fixture("account_info_v2.json"))
}

async fn handle_balance(headers: HeaderMap, State(state): State<TestServerState>) -> Response {
    if !has_auth_headers(&headers) {
        return unauthorized_response();
    }

    if state.increment_and_check() {
        return rate_limit_response();
    }
    json_response(&load_fixture("balance.json"))
}

async fn handle_position_risk(
    headers: HeaderMap,
    State(state): State<TestServerState>,
) -> Response {
    if !has_auth_headers(&headers) {
        return unauthorized_response();
    }

    if state.increment_and_check() {
        return rate_limit_response();
    }
    json_response(&load_fixture("position_risk.json"))
}

async fn handle_order_post(headers: HeaderMap, State(state): State<TestServerState>) -> Response {
    if !has_auth_headers(&headers) {
        return unauthorized_response();
    }

    if state.increment_and_check() {
        return rate_limit_response();
    }
    json_response(&load_fixture("order_response.json"))
}

async fn handle_order_get(headers: HeaderMap, State(state): State<TestServerState>) -> Response {
    if !has_auth_headers(&headers) {
        return unauthorized_response();
    }

    if state.increment_and_check() {
        return rate_limit_response();
    }
    json_response(&load_fixture("order_response.json"))
}

async fn handle_order_delete(headers: HeaderMap, State(state): State<TestServerState>) -> Response {
    if !has_auth_headers(&headers) {
        return unauthorized_response();
    }

    if state.increment_and_check() {
        return rate_limit_response();
    }
    json_response(&load_fixture("order_response.json"))
}

async fn handle_open_orders(headers: HeaderMap, State(state): State<TestServerState>) -> Response {
    if !has_auth_headers(&headers) {
        return unauthorized_response();
    }

    if state.increment_and_check() {
        return rate_limit_response();
    }
    json_response(&json!([]))
}

async fn handle_cancel_all(headers: HeaderMap, State(state): State<TestServerState>) -> Response {
    if !has_auth_headers(&headers) {
        return unauthorized_response();
    }

    if state.increment_and_check() {
        return rate_limit_response();
    }
    json_response(&json!({"code": 200, "msg": "The operation of cancel all open order is done."}))
}

async fn handle_listen_key_post(
    headers: HeaderMap,
    State(state): State<TestServerState>,
) -> Response {
    if !has_auth_headers(&headers) {
        return unauthorized_response();
    }

    if state.increment_and_check() {
        return rate_limit_response();
    }
    json_response(&json!({"listenKey": "test-listen-key-12345"}))
}

async fn handle_listen_key_put(headers: HeaderMap) -> Response {
    if !has_auth_headers(&headers) {
        return unauthorized_response();
    }
    json_response(&json!({}))
}

async fn handle_hedge_mode(headers: HeaderMap) -> Response {
    if !has_auth_headers(&headers) {
        return unauthorized_response();
    }
    json_response(&json!({"dualSidePosition": false}))
}

async fn handle_user_trades(headers: HeaderMap, State(state): State<TestServerState>) -> Response {
    if !has_auth_headers(&headers) {
        return unauthorized_response();
    }

    if state.increment_and_check() {
        return rate_limit_response();
    }
    json_response(&json!([]))
}

async fn handle_all_orders(headers: HeaderMap, State(state): State<TestServerState>) -> Response {
    if !has_auth_headers(&headers) {
        return unauthorized_response();
    }

    if state.increment_and_check() {
        return rate_limit_response();
    }
    json_response(&json!([]))
}

fn create_router(state: TestServerState) -> Router {
    Router::new()
        .route("/fapi/v1/ping", get(handle_ping))
        .route("/fapi/v1/time", get(handle_time))
        .route("/fapi/v1/exchangeInfo", get(handle_exchange_info))
        .route("/fapi/v1/depth", get(handle_depth))
        .route("/fapi/v2/account", get(handle_account))
        .route("/fapi/v2/balance", get(handle_balance))
        .route("/fapi/v2/positionRisk", get(handle_position_risk))
        .route(
            "/fapi/v1/order",
            post(handle_order_post)
                .get(handle_order_get)
                .delete(handle_order_delete)
                .put(handle_order_post),
        )
        .route("/fapi/v1/openOrders", get(handle_open_orders))
        .route("/fapi/v1/allOrders", get(handle_all_orders))
        .route("/fapi/v1/allOpenOrders", delete(handle_cancel_all))
        .route(
            "/fapi/v1/listenKey",
            post(handle_listen_key_post).put(handle_listen_key_put),
        )
        .route("/fapi/v1/positionSide/dual", get(handle_hedge_mode))
        .route("/fapi/v1/userTrades", get(handle_user_trades))
        .with_state(state)
}

async fn start_test_server(
    state: TestServerState,
) -> Result<SocketAddr, Box<dyn std::error::Error + Send + Sync>> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let router = create_router(state);

    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });

    tokio::time::sleep(Duration::from_millis(100)).await;
    Ok(addr)
}

fn create_raw_client(
    addr: &SocketAddr,
    api_key: Option<String>,
    api_secret: Option<String>,
) -> BinanceRawFuturesHttpClient {
    let base_url = format!("http://{addr}");
    BinanceRawFuturesHttpClient::new(
        BinanceProductType::UsdM,
        BinanceEnvironment::Mainnet,
        api_key,
        api_secret,
        Some(base_url),
        None,
        Some(60),
        None,
    )
    .unwrap()
}

#[rstest]
#[tokio::test]
async fn test_ping() {
    let addr = start_test_server(TestServerState::default()).await.unwrap();
    let client = create_raw_client(&addr, None, None);

    let result: serde_json::Value = client.get("ping", None::<&()>, false, false).await.unwrap();
    assert_eq!(result, json!({}));
}

#[rstest]
#[tokio::test]
async fn test_server_time() {
    let addr = start_test_server(TestServerState::default()).await.unwrap();
    let client = create_raw_client(&addr, None, None);

    let result: serde_json::Value = client.get("time", None::<&()>, false, false).await.unwrap();
    assert_eq!(result["serverTime"], 1700000000000_i64);
}

#[rstest]
#[tokio::test]
async fn test_exchange_info() {
    let addr = start_test_server(TestServerState::default()).await.unwrap();
    let client = create_raw_client(&addr, None, None);

    let result: serde_json::Value = client
        .get("exchangeInfo", None::<&()>, false, false)
        .await
        .unwrap();
    let symbols = result["symbols"].as_array().unwrap();
    assert!(!symbols.is_empty());
    assert_eq!(symbols[0]["symbol"], "BTCUSDT");
}

#[rstest]
#[tokio::test]
async fn test_depth() {
    let addr = start_test_server(TestServerState::default()).await.unwrap();
    let client = create_raw_client(&addr, None, None);

    let result: serde_json::Value = client
        .get("depth", None::<&()>, false, false)
        .await
        .unwrap();
    assert!(!result["bids"].as_array().unwrap().is_empty());
    assert!(!result["asks"].as_array().unwrap().is_empty());
}

#[rstest]
#[tokio::test]
async fn test_account_requires_credentials() {
    let addr = start_test_server(TestServerState::default()).await.unwrap();
    let client = create_raw_client(&addr, None, None);

    let result: Result<serde_json::Value, _> = client
        .get("/fapi/v2/account", None::<&()>, true, false)
        .await;
    assert!(result.is_err());
}

#[rstest]
#[tokio::test]
async fn test_account_with_credentials() {
    let addr = start_test_server(TestServerState::default()).await.unwrap();
    let client = create_raw_client(
        &addr,
        Some("test-key".to_string()),
        Some("test-secret".to_string()),
    );

    let result: serde_json::Value = client
        .get("/fapi/v2/account", None::<&()>, true, false)
        .await
        .unwrap();
    assert_eq!(result["canTrade"], true);
}

#[rstest]
#[tokio::test]
async fn test_position_risk_with_credentials() {
    let addr = start_test_server(TestServerState::default()).await.unwrap();
    let client = create_raw_client(
        &addr,
        Some("test-key".to_string()),
        Some("test-secret".to_string()),
    );

    let result: serde_json::Value = client
        .get("/fapi/v2/positionRisk", None::<&()>, true, false)
        .await
        .unwrap();
    assert!(result.as_array().is_some());
}

#[rstest]
#[tokio::test]
async fn test_open_orders_with_credentials() {
    let addr = start_test_server(TestServerState::default()).await.unwrap();
    let client = create_raw_client(
        &addr,
        Some("test-key".to_string()),
        Some("test-secret".to_string()),
    );

    let result: serde_json::Value = client
        .get("openOrders", None::<&()>, true, false)
        .await
        .unwrap();
    assert!(result.as_array().is_some());
}

#[rstest]
#[tokio::test]
async fn test_listen_key_creation() {
    let addr = start_test_server(TestServerState::default()).await.unwrap();
    let client = create_raw_client(
        &addr,
        Some("test-key".to_string()),
        Some("test-secret".to_string()),
    );

    let result: serde_json::Value = client
        .post("listenKey", None::<&()>, None, false, false)
        .await
        .unwrap();
    assert_eq!(result["listenKey"], "test-listen-key-12345");
}

#[rstest]
#[tokio::test]
async fn test_hedge_mode_query() {
    let addr = start_test_server(TestServerState::default()).await.unwrap();
    let client = create_raw_client(
        &addr,
        Some("test-key".to_string()),
        Some("test-secret".to_string()),
    );

    let result: serde_json::Value = client
        .get("positionSide/dual", None::<&()>, true, false)
        .await
        .unwrap();
    assert_eq!(result["dualSidePosition"], false);
}

#[rstest]
#[tokio::test]
async fn test_order_submission() {
    let addr = start_test_server(TestServerState::default()).await.unwrap();
    let client = create_raw_client(
        &addr,
        Some("test-key".to_string()),
        Some("test-secret".to_string()),
    );

    let params = BinanceNewOrderParamsBuilder::default()
        .symbol("BTCUSDT")
        .side(BinanceSide::Buy)
        .order_type(BinanceFuturesOrderType::Limit)
        .time_in_force(BinanceTimeInForce::Gtc)
        .quantity("0.001")
        .price("50000.00")
        .build()
        .unwrap();

    let result: serde_json::Value = client
        .post("order", Some(&params), None, true, true)
        .await
        .unwrap();
    assert!(result["orderId"].as_i64().is_some());
}

#[rstest]
#[tokio::test]
async fn test_order_query() {
    let addr = start_test_server(TestServerState::default()).await.unwrap();
    let client = create_raw_client(
        &addr,
        Some("test-key".to_string()),
        Some("test-secret".to_string()),
    );

    let result: serde_json::Value = client.get("order", None::<&()>, true, false).await.unwrap();
    assert!(result["orderId"].as_i64().is_some());
}

#[rstest]
#[tokio::test]
async fn test_order_cancellation() {
    let addr = start_test_server(TestServerState::default()).await.unwrap();
    let client = create_raw_client(
        &addr,
        Some("test-key".to_string()),
        Some("test-secret".to_string()),
    );

    let result: serde_json::Value = client
        .request_delete("order", None::<&()>, true, true)
        .await
        .unwrap();
    assert!(result["orderId"].as_i64().is_some());
}

#[rstest]
#[tokio::test]
async fn test_cancel_all_orders() {
    let addr = start_test_server(TestServerState::default()).await.unwrap();
    let client = create_raw_client(
        &addr,
        Some("test-key".to_string()),
        Some("test-secret".to_string()),
    );

    let result: serde_json::Value = client
        .request_delete("allOpenOrders", None::<&()>, true, false)
        .await
        .unwrap();
    assert_eq!(result["code"], 200);
}

#[rstest]
#[tokio::test]
async fn test_all_orders_history() {
    let addr = start_test_server(TestServerState::default()).await.unwrap();
    let client = create_raw_client(
        &addr,
        Some("test-key".to_string()),
        Some("test-secret".to_string()),
    );

    let result: serde_json::Value = client
        .get("allOrders", None::<&()>, true, false)
        .await
        .unwrap();
    assert!(result.as_array().is_some());
}

#[rstest]
#[tokio::test]
async fn test_user_trades() {
    let addr = start_test_server(TestServerState::default()).await.unwrap();
    let client = create_raw_client(
        &addr,
        Some("test-key".to_string()),
        Some("test-secret".to_string()),
    );

    let result: serde_json::Value = client
        .get("userTrades", None::<&()>, true, false)
        .await
        .unwrap();
    assert!(result.as_array().is_some());
}

#[rstest]
#[tokio::test]
async fn test_rate_limit_triggers() {
    let state = TestServerState::default().with_rate_limit(0);
    let addr = start_test_server(state).await.unwrap();
    let client = create_raw_client(
        &addr,
        Some("test-key".to_string()),
        Some("test-secret".to_string()),
    );

    let result: Result<serde_json::Value, _> = client
        .get("/fapi/v2/account", None::<&()>, true, false)
        .await;
    assert!(result.is_err());
}
