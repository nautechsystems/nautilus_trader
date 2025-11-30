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

//! Integration tests for the Kraken HTTP client using a mock Axum server.

use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

use axum::{
    Router,
    body::Body,
    extract::{Query, Request},
    http::{HeaderMap, StatusCode},
    response::Response,
};
use nautilus_kraken::{
    common::enums::{KrakenApiResult, KrakenEnvironment, KrakenOrderStatus},
    http::{KrakenFuturesRawHttpClient, KrakenSpotHttpClient, KrakenSpotRawHttpClient},
};
use nautilus_model::{
    data::BarType,
    identifiers::{InstrumentId, Symbol},
    instruments::{CryptoPerpetual, InstrumentAny},
    types::{Currency, Price, Quantity},
};
use rstest::rstest;
use serde_json::Value;

#[derive(Clone, Default)]
struct TestServerState {
    request_count: Arc<AtomicUsize>,
    last_trades_query: Arc<tokio::sync::Mutex<Option<HashMap<String, String>>>>,
    last_ohlc_query: Arc<tokio::sync::Mutex<Option<HashMap<String, String>>>>,
}

#[allow(dead_code)]
fn create_test_futures_instrument() -> InstrumentAny {
    let instrument_id = InstrumentId::from("PF_XBTUSD.KRAKEN");
    let raw_symbol = Symbol::new("PF_XBTUSD");
    let btc = Currency::BTC();
    let usd = Currency::USD();

    // price_precision must match price_increment.precision (0 for "1")
    // size_precision must match size_increment.precision (4 for "0.0001")
    InstrumentAny::CryptoPerpetual(CryptoPerpetual::new(
        instrument_id,
        raw_symbol,
        btc,
        usd,
        usd,
        false, // is_inverse
        0,     // price_precision (matches "1" increment)
        4,     // size_precision (matches "0.0001" increment)
        Price::from("1"),
        Quantity::from("0.0001"),
        None, // multiplier
        None, // lot_size
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
        0.into(),
        0.into(),
    ))
}

fn manifest_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn load_test_data(filename: &str) -> Value {
    let path = manifest_path().join("test_data").join(filename);
    let content = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("Failed to load test data from {path:?}: {e}"));
    serde_json::from_str(&content)
        .unwrap_or_else(|e| panic!("Failed to parse test data from {path:?}: {e}"))
}

fn has_auth_headers(headers: &HeaderMap) -> bool {
    headers.contains_key("API-Key") && headers.contains_key("API-Sign")
}

async fn mock_server_time() -> Response {
    let data = load_test_data("http_server_time.json");
    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "application/json")
        .body(Body::from(data.to_string()))
        .unwrap()
}

async fn mock_system_status() -> Response {
    let data = load_test_data("http_system_status.json");
    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "application/json")
        .body(Body::from(data.to_string()))
        .unwrap()
}

async fn mock_asset_pairs() -> Response {
    let data = load_test_data("http_asset_pairs.json");
    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "application/json")
        .body(Body::from(data.to_string()))
        .unwrap()
}

async fn mock_ticker() -> Response {
    let data = load_test_data("http_ticker.json");
    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "application/json")
        .body(Body::from(data.to_string()))
        .unwrap()
}

async fn mock_ohlc(
    Query(params): Query<HashMap<String, String>>,
    state: Arc<TestServerState>,
) -> Response {
    *state.last_ohlc_query.lock().await = Some(params);
    let data = load_test_data("http_ohlc.json");
    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "application/json")
        .body(Body::from(data.to_string()))
        .unwrap()
}

async fn mock_order_book() -> Response {
    let data = load_test_data("http_order_book.json");
    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "application/json")
        .body(Body::from(data.to_string()))
        .unwrap()
}

async fn mock_trades(
    Query(params): Query<HashMap<String, String>>,
    state: Arc<TestServerState>,
) -> Response {
    *state.last_trades_query.lock().await = Some(params);
    let data = load_test_data("http_trades.json");
    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "application/json")
        .body(Body::from(data.to_string()))
        .unwrap()
}

async fn mock_websockets_token(headers: HeaderMap) -> Response {
    if !has_auth_headers(&headers) {
        let error_response = r#"{"error":["EAPI:Invalid key"]}"#;
        return Response::builder()
            .status(StatusCode::UNAUTHORIZED)
            .header("content-type", "application/json")
            .body(Body::from(error_response))
            .unwrap();
    }

    let response = r#"{
        "error": [],
        "result": {
            "token": "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
            "expires": 900
        }
    }"#;

    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "application/json")
        .body(Body::from(response))
        .unwrap()
}

async fn mock_rate_limit_error() -> Response {
    let error_response = r#"{"error":["EAPI:Rate limit exceeded"]}"#;
    Response::builder()
        .status(StatusCode::TOO_MANY_REQUESTS)
        .header("content-type", "application/json")
        .body(Body::from(error_response))
        .unwrap()
}

async fn mock_futures_instruments() -> Response {
    let data = load_test_data("http_futures_instruments.json");
    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "application/json")
        .body(Body::from(data.to_string()))
        .unwrap()
}

async fn mock_futures_tickers() -> Response {
    let data = load_test_data("http_futures_tickers.json");
    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "application/json")
        .body(Body::from(data.to_string()))
        .unwrap()
}

async fn mock_futures_candles(req: Request) -> Response {
    let path = req.uri().path();
    let parts: Vec<&str> = path.split('/').collect();

    if parts.len() >= 6 {
        let tick_type = parts[4];
        let filename = match tick_type {
            "trade" => "http_futures_candles_trade.json",
            "mark" => "http_futures_candles_mark.json",
            "spot" => "http_futures_candles_spot.json",
            _ => "http_futures_candles_trade.json",
        };

        let data = load_test_data(filename);
        Response::builder()
            .status(StatusCode::OK)
            .header("content-type", "application/json")
            .body(Body::from(data.to_string()))
            .unwrap()
    } else {
        Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Body::from("Invalid candles path"))
            .unwrap()
    }
}

async fn mock_open_orders() -> Response {
    let data = load_test_data("http_open_orders.json");
    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "application/json")
        .body(Body::from(data.to_string()))
        .unwrap()
}

async fn mock_closed_orders() -> Response {
    let data = load_test_data("http_closed_orders.json");
    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "application/json")
        .body(Body::from(data.to_string()))
        .unwrap()
}

async fn mock_trades_history() -> Response {
    let data = load_test_data("http_trades_history.json");
    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "application/json")
        .body(Body::from(data.to_string()))
        .unwrap()
}

async fn mock_futures_open_orders() -> Response {
    let data = load_test_data("http_futures_open_orders.json");
    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "application/json")
        .body(Body::from(data.to_string()))
        .unwrap()
}

async fn mock_futures_order_events() -> Response {
    let data = load_test_data("http_futures_order_events.json");
    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "application/json")
        .body(Body::from(data.to_string()))
        .unwrap()
}

async fn mock_futures_fills() -> Response {
    let data = load_test_data("http_futures_fills.json");
    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "application/json")
        .body(Body::from(data.to_string()))
        .unwrap()
}

async fn mock_futures_open_positions() -> Response {
    let data = load_test_data("http_futures_open_positions.json");
    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "application/json")
        .body(Body::from(data.to_string()))
        .unwrap()
}

// Order Execution Mock Handlers

async fn mock_add_order_spot() -> Response {
    let data = load_test_data("http_add_order_spot.json");
    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "application/json")
        .body(Body::from(data.to_string()))
        .unwrap()
}

async fn mock_cancel_order_spot() -> Response {
    let data = load_test_data("http_cancel_order_spot.json");
    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "application/json")
        .body(Body::from(data.to_string()))
        .unwrap()
}

async fn mock_send_order_futures() -> Response {
    let data = load_test_data("http_send_order_futures.json");
    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "application/json")
        .body(Body::from(data.to_string()))
        .unwrap()
}

async fn mock_cancel_order_futures() -> Response {
    let data = load_test_data("http_cancel_order_futures.json");
    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "application/json")
        .body(Body::from(data.to_string()))
        .unwrap()
}

async fn mock_futures_public_executions() -> Response {
    let data = load_test_data("http_futures_public_executions.json");
    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "application/json")
        .body(Body::from(data.to_string()))
        .unwrap()
}

async fn mock_handler(req: Request, state: Arc<TestServerState>) -> Response {
    state.request_count.fetch_add(1, Ordering::Relaxed);

    let path = req.uri().path();

    if path.starts_with("/derivatives/api/v3/") {
        return match path {
            "/derivatives/api/v3/instruments" => mock_futures_instruments().await,
            "/derivatives/api/v3/tickers" => mock_futures_tickers().await,
            "/derivatives/api/v3/fills" => mock_futures_fills().await,
            "/derivatives/api/v3/openpositions" => mock_futures_open_positions().await,
            "/derivatives/api/v3/openorders" => mock_futures_open_orders().await,
            "/derivatives/api/v3/sendorder" => mock_send_order_futures().await,
            "/derivatives/api/v3/cancelorder" => mock_cancel_order_futures().await,
            "/derivatives/api/v3/editorder" => mock_cancel_order_futures().await,
            "/derivatives/api/v3/batchorder" => mock_send_order_futures().await,
            "/derivatives/api/v3/cancelallorders" => mock_cancel_order_futures().await,
            _ => Response::builder()
                .status(StatusCode::NOT_FOUND)
                .body(Body::from("Futures endpoint not found"))
                .unwrap(),
        };
    }

    if path.starts_with("/api/history/v2/") {
        return match path {
            p if p.starts_with("/api/history/v2/orders") => mock_futures_order_events().await,
            p if p.starts_with("/api/history/v2/market/") && p.contains("/executions") => {
                mock_futures_public_executions().await
            }
            _ => Response::builder()
                .status(StatusCode::NOT_FOUND)
                .body(Body::from("Futures history endpoint not found"))
                .unwrap(),
        };
    }

    if path.starts_with("/api/charts/v1/") {
        return mock_futures_candles(req).await;
    }

    match path {
        "/0/public/Time" => mock_server_time().await,
        "/0/public/SystemStatus" => mock_system_status().await,
        "/0/public/AssetPairs" => mock_asset_pairs().await,
        "/0/public/Ticker" => mock_ticker().await,
        "/0/public/OHLC" => {
            let query =
                Query::<HashMap<String, String>>::try_from_uri(req.uri()).unwrap_or_default();
            mock_ohlc(query, state.clone()).await
        }
        "/0/public/Depth" => mock_order_book().await,
        "/0/public/Trades" => {
            let query =
                Query::<HashMap<String, String>>::try_from_uri(req.uri()).unwrap_or_default();
            mock_trades(query, state.clone()).await
        }
        "/0/private/GetWebSocketsToken" => mock_websockets_token(req.headers().clone()).await,
        "/0/private/OpenOrders" => mock_open_orders().await,
        "/0/private/ClosedOrders" => mock_closed_orders().await,
        "/0/private/TradesHistory" => mock_trades_history().await,
        "/0/private/AddOrder" => mock_add_order_spot().await,
        "/0/private/CancelOrder" => mock_cancel_order_spot().await,
        "/0/private/CancelAll" => mock_cancel_order_spot().await,
        "/0/private/EditOrder" => mock_add_order_spot().await,
        "/0/test/rate_limit" => mock_rate_limit_error().await,
        _ => Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Body::from("Not found"))
            .unwrap(),
    }
}

fn create_router(state: Arc<TestServerState>) -> Router {
    Router::new().fallback(move |req| {
        let state = state.clone();
        async move { mock_handler(req, state).await }
    })
}

// =============================================================================
// Spot Raw HTTP Client Tests (KrakenSpotRawHttpClient)
// =============================================================================

#[rstest]
#[tokio::test]
async fn test_spot_raw_get_server_time() {
    let state = Arc::new(TestServerState::default());
    let app = create_router(state.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base_url = format!("http://{addr}");

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let client = KrakenSpotRawHttpClient::new(
        KrakenEnvironment::Mainnet,
        Some(base_url),
        Some(10),
        None,
        None,
        None,
        None,
    )
    .unwrap();

    let result = client.get_server_time().await;
    assert!(result.is_ok(), "Failed to get server time: {result:?}");

    let server_time = result.unwrap();
    assert!(server_time.unixtime > 0);
    assert!(!server_time.rfc1123.is_empty());
}

#[rstest]
#[tokio::test]
async fn test_spot_raw_get_system_status() {
    let state = Arc::new(TestServerState::default());
    let app = create_router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base_url = format!("http://{addr}");

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let client = KrakenSpotRawHttpClient::new(
        KrakenEnvironment::Mainnet,
        Some(base_url),
        Some(10),
        None,
        None,
        None,
        None,
    )
    .unwrap();

    let result = client.get_system_status().await;
    assert!(result.is_ok(), "Failed to get system status: {result:?}");

    let status = result.unwrap();
    assert!(!status.timestamp.is_empty());
}

#[rstest]
#[tokio::test]
async fn test_spot_raw_get_asset_pairs() {
    let state = Arc::new(TestServerState::default());
    let app = create_router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base_url = format!("http://{addr}");

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let client = KrakenSpotRawHttpClient::new(
        KrakenEnvironment::Mainnet,
        Some(base_url),
        Some(10),
        None,
        None,
        None,
        None,
    )
    .unwrap();

    let result = client.get_asset_pairs(None).await;
    assert!(result.is_ok(), "Failed to get asset pairs: {result:?}");

    let pairs = result.unwrap();
    assert!(!pairs.is_empty());
    assert!(pairs.contains_key("XBTUSDT"));
}

// =============================================================================
// Spot Domain HTTP Client Tests (KrakenSpotHttpClient)
// =============================================================================

#[rstest]
#[tokio::test]
async fn test_spot_domain_request_instruments() {
    let state = Arc::new(TestServerState::default());
    let app = create_router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base_url = format!("http://{addr}");

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let client = KrakenSpotHttpClient::new(
        KrakenEnvironment::Mainnet,
        Some(base_url),
        Some(10),
        None,
        None,
        None,
        None,
    )
    .unwrap();

    let result = client.request_instruments(None).await;
    assert!(result.is_ok(), "Failed to request instruments: {result:?}");

    let instruments: Vec<InstrumentAny> = result.unwrap();
    assert!(!instruments.is_empty());
}

#[rstest]
#[tokio::test]
async fn test_spot_raw_get_ticker() {
    let state = Arc::new(TestServerState::default());
    let app = create_router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base_url = format!("http://{addr}");

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let client = KrakenSpotRawHttpClient::new(
        KrakenEnvironment::Mainnet,
        Some(base_url),
        Some(10),
        None,
        None,
        None,
        None,
    )
    .unwrap();

    let result = client.get_ticker(vec!["XBTUSDT".to_string()]).await;
    assert!(result.is_ok(), "Failed to get ticker: {result:?}");

    let ticker = result.unwrap();
    assert!(ticker.contains_key("XBTUSDT"));
}

#[rstest]
#[tokio::test]
async fn test_spot_raw_get_book_depth() {
    let state = Arc::new(TestServerState::default());
    let app = create_router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base_url = format!("http://{addr}");

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let client = KrakenSpotRawHttpClient::new(
        KrakenEnvironment::Mainnet,
        Some(base_url),
        Some(10),
        None,
        None,
        None,
        None,
    )
    .unwrap();

    let result = client.get_book_depth("XBTUSDT", None).await;
    assert!(result.is_ok(), "Failed to get book depth: {result:?}");

    let book = result.unwrap();
    assert!(book.contains_key("XBTUSDT"));
}

#[rstest]
#[tokio::test]
async fn test_spot_raw_get_trades() {
    let state = Arc::new(TestServerState::default());
    let app = create_router(state.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base_url = format!("http://{addr}");

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let client = KrakenSpotRawHttpClient::new(
        KrakenEnvironment::Mainnet,
        Some(base_url),
        Some(10),
        None,
        None,
        None,
        None,
    )
    .unwrap();

    let result = client.get_trades("XBTUSDT", None).await;
    assert!(result.is_ok(), "Failed to get trades: {result:?}");

    let response = result.unwrap();
    assert!(!response.data.is_empty());
}

#[rstest]
#[tokio::test]
async fn test_spot_raw_get_ohlc() {
    let state = Arc::new(TestServerState::default());
    let app = create_router(state.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base_url = format!("http://{addr}");

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let client = KrakenSpotRawHttpClient::new(
        KrakenEnvironment::Mainnet,
        Some(base_url),
        Some(10),
        None,
        None,
        None,
        None,
    )
    .unwrap();

    let result = client.get_ohlc("XBTUSDT", Some(60), None).await;
    assert!(result.is_ok(), "Failed to get OHLC: {result:?}");

    let response = result.unwrap();
    assert!(!response.data.is_empty());
}

#[rstest]
#[tokio::test]
async fn test_spot_raw_get_trades_with_since() {
    let state = Arc::new(TestServerState::default());
    let app = create_router(state.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base_url = format!("http://{addr}");

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let client = KrakenSpotRawHttpClient::new(
        KrakenEnvironment::Mainnet,
        Some(base_url),
        Some(10),
        None,
        None,
        None,
        None,
    )
    .unwrap();

    let since = "1234567890".to_string();
    let result = client.get_trades("XBTUSDT", Some(since.clone())).await;
    assert!(
        result.is_ok(),
        "Failed to get trades with since: {result:?}"
    );

    let query = state.last_trades_query.lock().await;
    assert!(query.is_some());
    let params = query.as_ref().unwrap();
    assert_eq!(params.get("since"), Some(&since));
}

#[rstest]
#[tokio::test]
async fn test_spot_raw_get_ohlc_with_interval() {
    let state = Arc::new(TestServerState::default());
    let app = create_router(state.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base_url = format!("http://{addr}");

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let client = KrakenSpotRawHttpClient::new(
        KrakenEnvironment::Mainnet,
        Some(base_url),
        Some(10),
        None,
        None,
        None,
        None,
    )
    .unwrap();

    let result = client.get_ohlc("XBTUSDT", Some(60), None).await;
    assert!(
        result.is_ok(),
        "Failed to get OHLC with interval: {result:?}"
    );

    let query = state.last_ohlc_query.lock().await;
    assert!(query.is_some());
    let params = query.as_ref().unwrap();
    assert_eq!(params.get("interval"), Some(&"60".to_string()));
}

#[rstest]
#[tokio::test]
async fn test_spot_raw_get_websockets_token_requires_credentials() {
    let state = Arc::new(TestServerState::default());
    let app = create_router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base_url = format!("http://{addr}");

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // Client without credentials
    let client = KrakenSpotRawHttpClient::new(
        KrakenEnvironment::Mainnet,
        Some(base_url),
        Some(10),
        None,
        None,
        None,
        None,
    )
    .unwrap();

    let result = client.get_websockets_token().await;
    assert!(result.is_err());
    assert!(
        matches!(result, Err(e) if e.to_string().contains("credentials")),
        "Expected authentication error"
    );
}

#[rstest]
#[tokio::test]
async fn test_spot_raw_get_websockets_token_with_credentials() {
    let state = Arc::new(TestServerState::default());
    let app = create_router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base_url = format!("http://{addr}");

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // Client with credentials (API secret must be base64-encoded)
    let client = KrakenSpotRawHttpClient::with_credentials(
        "test_api_key".to_string(),
        "dGVzdF9hcGlfc2VjcmV0X2Jhc2U2NA==".to_string(),
        KrakenEnvironment::Mainnet,
        Some(base_url),
        Some(10),
        None,
        None,
        None,
        None,
    )
    .unwrap();

    let result = client.get_websockets_token().await;
    assert!(result.is_ok(), "Failed to get websockets token: {result:?}");

    let token = result.unwrap();
    assert_eq!(token.token, "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA");
    assert_eq!(token.expires, 900);
}

#[rstest]
#[tokio::test]
async fn test_spot_domain_request_trades() {
    let state = Arc::new(TestServerState::default());
    let app = create_router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base_url = format!("http://{addr}");

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let client = KrakenSpotHttpClient::new(
        KrakenEnvironment::Mainnet,
        Some(base_url),
        Some(10),
        None,
        None,
        None,
        None,
    )
    .unwrap();

    // First load instruments to populate cache
    let instruments = client.request_instruments(None).await.unwrap();
    client.cache_instruments(instruments);

    // Create a valid instrument ID from cached instruments
    let instrument_id = InstrumentId::from("XBT/USDT.KRAKEN");

    let result = client.request_trades(instrument_id, None, None, None).await;
    assert!(result.is_ok(), "Failed to request trades: {result:?}");

    let trades = result.unwrap();
    assert!(!trades.is_empty());
}

#[rstest]
#[tokio::test]
async fn test_spot_domain_request_bars() {
    let state = Arc::new(TestServerState::default());
    let app = create_router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base_url = format!("http://{addr}");

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let client = KrakenSpotHttpClient::new(
        KrakenEnvironment::Mainnet,
        Some(base_url),
        Some(10),
        None,
        None,
        None,
        None,
    )
    .unwrap();

    // First load instruments to populate cache
    let instruments = client.request_instruments(None).await.unwrap();
    client.cache_instruments(instruments);

    // Create a BarType for 1-minute bars
    let bar_type = BarType::from("XBT/USDT.KRAKEN-1-MINUTE-LAST-INTERNAL");

    let result = client.request_bars(bar_type, None, None, None).await;
    assert!(result.is_ok(), "Failed to request bars: {result:?}");

    let bars = result.unwrap();
    assert!(!bars.is_empty());
}

#[rstest]
#[tokio::test]
async fn test_spot_raw_multiple_requests_increment_count() {
    let state = Arc::new(TestServerState::default());
    let app = create_router(state.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base_url = format!("http://{addr}");

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let client = KrakenSpotRawHttpClient::new(
        KrakenEnvironment::Mainnet,
        Some(base_url),
        Some(10),
        None,
        None,
        None,
        None,
    )
    .unwrap();

    let initial_count = state.request_count.load(Ordering::Relaxed);

    client.get_server_time().await.unwrap();
    client.get_system_status().await.unwrap();
    client.get_asset_pairs(None).await.unwrap();

    let final_count = state.request_count.load(Ordering::Relaxed);
    assert_eq!(final_count - initial_count, 3);
}

// =============================================================================
// Futures Raw HTTP Client Tests (KrakenFuturesRawHttpClient)
// =============================================================================

#[rstest]
#[tokio::test]
async fn test_futures_raw_get_instruments() {
    let state = Arc::new(TestServerState::default());
    let app = create_router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base_url = format!("http://{addr}");

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let client = KrakenFuturesRawHttpClient::new(
        KrakenEnvironment::Mainnet,
        Some(base_url),
        Some(10),
        None,
        None,
        None,
        None,
    )
    .unwrap();

    let result = client.get_instruments().await;
    assert!(
        result.is_ok(),
        "Failed to get futures instruments: {result:?}"
    );

    let response = result.unwrap();
    assert_eq!(response.result, KrakenApiResult::Success);
    assert!(!response.instruments.is_empty());
    assert_eq!(response.instruments[0].symbol, "PI_XBTUSD");
    assert_eq!(response.instruments[0].base, "BTC");
    assert_eq!(response.instruments[0].quote, "USD");
}

#[rstest]
#[tokio::test]
async fn test_futures_raw_get_tickers() {
    let state = Arc::new(TestServerState::default());
    let app = create_router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base_url = format!("http://{addr}");

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let client = KrakenFuturesRawHttpClient::new(
        KrakenEnvironment::Mainnet,
        Some(base_url),
        Some(10),
        None,
        None,
        None,
        None,
    )
    .unwrap();

    let result = client.get_tickers().await;
    assert!(result.is_ok(), "Failed to get futures tickers: {result:?}");

    let response = result.unwrap();
    assert_eq!(response.result, KrakenApiResult::Success);
    assert!(!response.tickers.is_empty());

    let ticker = &response.tickers[0];
    assert_eq!(ticker.symbol, "PI_XBTUSD");
    assert!(ticker.mark_price > 0.0);
    assert!(ticker.index_price > 0.0);
}

#[rstest]
#[tokio::test]
async fn test_futures_raw_get_ohlc_trade() {
    let state = Arc::new(TestServerState::default());
    let app = create_router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base_url = format!("http://{addr}");

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let client = KrakenFuturesRawHttpClient::new(
        KrakenEnvironment::Mainnet,
        Some(base_url),
        Some(10),
        None,
        None,
        None,
        None,
    )
    .unwrap();

    let result = client
        .get_ohlc("trade", "PI_XBTUSD", "1h", None, None)
        .await;
    assert!(
        result.is_ok(),
        "Failed to get futures trade candles: {result:?}"
    );

    let response = result.unwrap();
    assert!(!response.candles.is_empty());
    assert_eq!(response.candles.len(), 3);

    let candle = &response.candles[0];
    assert_eq!(candle.time, 1_731_715_200_000);
    assert_eq!(candle.open, "91069");
    assert_eq!(candle.close, "91045.5");
}

#[rstest]
#[tokio::test]
async fn test_futures_raw_get_ohlc_mark() {
    let state = Arc::new(TestServerState::default());
    let app = create_router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base_url = format!("http://{addr}");

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let client = KrakenFuturesRawHttpClient::new(
        KrakenEnvironment::Mainnet,
        Some(base_url),
        Some(10),
        None,
        None,
        None,
        None,
    )
    .unwrap();

    let result = client.get_ohlc("mark", "PI_XBTUSD", "1h", None, None).await;
    assert!(
        result.is_ok(),
        "Failed to get futures mark candles: {result:?}"
    );

    let response = result.unwrap();
    assert!(!response.candles.is_empty());

    let candle = &response.candles[0];
    assert_eq!(candle.time, 1_731_715_200_000);
    assert!(candle.open.contains('.'));
    assert_eq!(candle.volume, "0");
}

#[rstest]
#[tokio::test]
async fn test_futures_raw_get_ohlc_spot() {
    let state = Arc::new(TestServerState::default());
    let app = create_router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base_url = format!("http://{addr}");

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let client = KrakenFuturesRawHttpClient::new(
        KrakenEnvironment::Mainnet,
        Some(base_url),
        Some(10),
        None,
        None,
        None,
        None,
    )
    .unwrap();

    let result = client.get_ohlc("spot", "PI_XBTUSD", "1h", None, None).await;
    assert!(
        result.is_ok(),
        "Failed to get futures spot/index candles: {result:?}"
    );

    let response = result.unwrap();
    assert!(!response.candles.is_empty());

    let candle = &response.candles[0];
    assert_eq!(candle.time, 1_731_715_200_000);
    assert!(candle.open.contains('.'));
    assert_eq!(candle.volume, "0");
}

#[rstest]
#[tokio::test]
async fn test_futures_raw_get_public_executions() {
    let state = Arc::new(TestServerState::default());
    let app = create_router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base_url = format!("http://{addr}");

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let client = KrakenFuturesRawHttpClient::new(
        KrakenEnvironment::Mainnet,
        Some(base_url),
        Some(10),
        None,
        None,
        None,
        None,
    )
    .unwrap();

    let result = client
        .get_public_executions("PF_XBTUSD", None, None, None, None)
        .await;

    assert!(result.is_ok(), "Expected success, got: {:?}", result.err());
    let response = result.unwrap();
    assert!(
        !response.elements.is_empty(),
        "Expected at least one execution"
    );

    // Verify execution data
    let element = &response.elements[0];
    let execution = &element.event.execution.execution;
    assert!(!execution.uid.is_empty());
    assert!(!execution.price.is_empty());
    assert!(!execution.quantity.is_empty());
}

// =============================================================================
// Spot Raw HTTP Client Tests - Authenticated/Reconciliation
// =============================================================================

#[rstest]
#[tokio::test]
async fn test_spot_raw_get_open_orders() {
    let state = Arc::new(TestServerState::default());
    let app = create_router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base_url = format!("http://{addr}");

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let client = KrakenSpotRawHttpClient::with_credentials(
        "test".to_string(),
        "test".to_string(),
        KrakenEnvironment::Mainnet,
        Some(base_url),
        Some(10),
        None,
        None,
        None,
        None,
    )
    .unwrap();

    let result = client.get_open_orders(Some(true), None).await;
    assert!(result.is_ok(), "Failed to get open orders: {result:?}");

    let orders = result.unwrap();
    assert_eq!(orders.len(), 2);
    assert!(orders.contains_key("O26VBY-ISGAE-JP5TLU"));
    assert!(orders.contains_key("OYEQF4-FDE4C-NMUYUI"));

    let first_order = orders.get("O26VBY-ISGAE-JP5TLU").unwrap();
    assert_eq!(first_order.status, KrakenOrderStatus::Open);
    assert_eq!(first_order.descr.pair, "XBTUSDT");
    assert_eq!(first_order.vol, "0.50000000");
}

#[rstest]
#[tokio::test]
async fn test_spot_raw_get_closed_orders() {
    let state = Arc::new(TestServerState::default());
    let app = create_router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base_url = format!("http://{addr}");

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let client = KrakenSpotRawHttpClient::with_credentials(
        "test".to_string(),
        "test".to_string(),
        KrakenEnvironment::Mainnet,
        Some(base_url),
        Some(10),
        None,
        None,
        None,
        None,
    )
    .unwrap();

    let result = client
        .get_closed_orders(Some(true), None, None, None, None, None)
        .await;
    assert!(result.is_ok(), "Failed to get closed orders: {result:?}");

    let orders = result.unwrap();
    assert_eq!(orders.len(), 2);
    assert!(orders.contains_key("O5KZFT-GH3AD-LP6TLU"));
    assert!(orders.contains_key("OCLOSED-2-TESTID"));

    let first_order = orders.get("O5KZFT-GH3AD-LP6TLU").unwrap();
    assert_eq!(first_order.status, KrakenOrderStatus::Closed);
    assert_eq!(first_order.vol_exec, "0.50000000");
}

#[rstest]
#[tokio::test]
async fn test_spot_raw_get_trades_history() {
    let state = Arc::new(TestServerState::default());
    let app = create_router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base_url = format!("http://{addr}");

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let client = KrakenSpotRawHttpClient::with_credentials(
        "test".to_string(),
        "test".to_string(),
        KrakenEnvironment::Mainnet,
        Some(base_url),
        Some(10),
        None,
        None,
        None,
        None,
    )
    .unwrap();

    let result = client
        .get_trades_history(None, Some(true), None, None, None)
        .await;
    assert!(result.is_ok(), "Failed to get trades history: {result:?}");

    let trades = result.unwrap();
    assert!(!trades.is_empty());
}

// =============================================================================
// Futures Raw HTTP Client Tests - Authenticated/Reconciliation
// =============================================================================

#[rstest]
#[tokio::test]
async fn test_futures_raw_get_open_orders() {
    let state = Arc::new(TestServerState::default());
    let app = create_router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base_url = format!("http://{addr}");

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let client = KrakenFuturesRawHttpClient::with_credentials(
        "test".to_string(),
        "test".to_string(),
        KrakenEnvironment::Mainnet,
        Some(base_url),
        Some(10),
        None,
        None,
        None,
        None,
    )
    .unwrap();

    let result = client.get_open_orders().await;
    assert!(
        result.is_ok(),
        "Failed to get futures open orders: {result:?}"
    );

    let response = result.unwrap();
    assert_eq!(response.result, KrakenApiResult::Success);
    assert_eq!(response.open_orders.len(), 3);

    let first_order = &response.open_orders[0];
    assert_eq!(first_order.order_id, "2ce038ae-c144-4de7-a0f1-82f7f4fca864");
    assert_eq!(first_order.symbol, "PI_ETHUSD");
}

#[rstest]
#[tokio::test]
async fn test_futures_raw_get_order_events() {
    let state = Arc::new(TestServerState::default());
    let app = create_router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base_url = format!("http://{addr}");

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let client = KrakenFuturesRawHttpClient::with_credentials(
        "test".to_string(),
        "test".to_string(),
        KrakenEnvironment::Mainnet,
        Some(base_url),
        Some(10),
        None,
        None,
        None,
        None,
    )
    .unwrap();

    let result = client.get_order_events(None, None, None).await;
    assert!(
        result.is_ok(),
        "Failed to get futures order events: {result:?}"
    );

    let response = result.unwrap();
    assert_eq!(response.result, KrakenApiResult::Success);
    assert_eq!(response.elements.len(), 3);

    let first_event = &response.elements[0];
    assert_eq!(first_event.order_id, "c8a35168-8d52-4609-944f-3f32bb0d5c77");
    assert_eq!(first_event.symbol, "PI_XBTUSD");
    assert_eq!(first_event.filled, 5000.0);
    assert_eq!(first_event.quantity, 5000.0);

    let third_event = &response.elements[2];
    assert_eq!(third_event.filled, 0.0);
    assert!(third_event.reduce_only);
}

#[rstest]
#[tokio::test]
async fn test_futures_raw_get_fills() {
    let state = Arc::new(TestServerState::default());
    let app = create_router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base_url = format!("http://{addr}");

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let client = KrakenFuturesRawHttpClient::with_credentials(
        "test".to_string(),
        "test".to_string(),
        KrakenEnvironment::Mainnet,
        Some(base_url),
        Some(10),
        None,
        None,
        None,
        None,
    )
    .unwrap();

    let result = client.get_fills(None).await;
    assert!(result.is_ok(), "Failed to get futures fills: {result:?}");

    let response = result.unwrap();
    assert_eq!(response.result, KrakenApiResult::Success);
    assert_eq!(response.fills.len(), 3);

    let first_fill = &response.fills[0];
    assert_eq!(first_fill.fill_id, "cad76f07-814e-4dc6-8478-7867407b6bff");
    assert_eq!(first_fill.symbol, "PI_XBTUSD");
    assert_eq!(first_fill.size, 5000.0);
}

#[rstest]
#[tokio::test]
async fn test_futures_raw_get_open_positions() {
    let state = Arc::new(TestServerState::default());
    let app = create_router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base_url = format!("http://{addr}");

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let client = KrakenFuturesRawHttpClient::with_credentials(
        "test".to_string(),
        "test".to_string(),
        KrakenEnvironment::Mainnet,
        Some(base_url),
        Some(10),
        None,
        None,
        None,
        None,
    )
    .unwrap();

    let result = client.get_open_positions().await;
    assert!(
        result.is_ok(),
        "Failed to get futures open positions: {result:?}"
    );

    let response = result.unwrap();
    assert_eq!(response.result, KrakenApiResult::Success);
    assert_eq!(response.open_positions.len(), 2);

    let first_position = &response.open_positions[0];
    assert_eq!(first_position.symbol, "PI_XBTUSD");
    assert_eq!(first_position.size, 8000.0);
}

// =============================================================================
// Spot Raw HTTP Client Tests - Order Execution
// =============================================================================

#[rstest]
#[tokio::test]
async fn test_spot_raw_add_order() {
    let state = Arc::new(TestServerState::default());
    let app = create_router(state.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base_url = format!("http://{addr}");

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let client = KrakenSpotRawHttpClient::with_credentials(
        "test".to_string(),
        "test".to_string(),
        KrakenEnvironment::Mainnet,
        Some(base_url),
        Some(10),
        None,
        None,
        None,
        None,
    )
    .unwrap();

    let mut params = HashMap::new();
    params.insert("pair".to_string(), "XBTUSD".to_string());
    params.insert("type".to_string(), "buy".to_string());
    params.insert("ordertype".to_string(), "limit".to_string());
    params.insert("volume".to_string(), "0.01".to_string());
    params.insert("price".to_string(), "50000".to_string());

    let result = client.add_order(params).await;
    assert!(result.is_ok(), "Failed to add order: {result:?}");

    let response = result.unwrap();
    assert_eq!(response.txid.len(), 1);
    assert_eq!(response.txid[0], "OUF4EM-FRGI2-MQMWZD");
}

#[rstest]
#[tokio::test]
async fn test_spot_raw_cancel_order() {
    let state = Arc::new(TestServerState::default());
    let app = create_router(state.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base_url = format!("http://{addr}");

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let client = KrakenSpotRawHttpClient::with_credentials(
        "test".to_string(),
        "test".to_string(),
        KrakenEnvironment::Mainnet,
        Some(base_url),
        Some(10),
        None,
        None,
        None,
        None,
    )
    .unwrap();

    let result = client
        .cancel_order(Some("OUF4EM-FRGI2-MQMWZD".to_string()), None)
        .await;
    assert!(result.is_ok(), "Failed to cancel order: {result:?}");

    let response = result.unwrap();
    assert_eq!(response.count, 1);
}

// =============================================================================
// Futures Raw HTTP Client Tests - Order Execution
// =============================================================================

#[rstest]
#[tokio::test]
async fn test_futures_raw_send_order() {
    let state = Arc::new(TestServerState::default());
    let app = create_router(state.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base_url = format!("http://{addr}");

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let client = KrakenFuturesRawHttpClient::with_credentials(
        "test".to_string(),
        "test".to_string(),
        KrakenEnvironment::Mainnet,
        Some(base_url),
        Some(10),
        None,
        None,
        None,
        None,
    )
    .unwrap();

    let mut params = HashMap::new();
    params.insert("symbol".to_string(), "PI_XBTUSD".to_string());
    params.insert("side".to_string(), "buy".to_string());
    params.insert("orderType".to_string(), "lmt".to_string());
    params.insert("size".to_string(), "1".to_string());
    params.insert("limitPrice".to_string(), "50000".to_string());

    let result = client.send_order(params).await;
    assert!(result.is_ok(), "Failed to send order: {result:?}");

    let response = result.unwrap();
    assert_eq!(response.result, KrakenApiResult::Success);
    assert_eq!(response.send_status.unwrap().status, "placed");
}

#[rstest]
#[tokio::test]
async fn test_futures_raw_cancel_order() {
    let state = Arc::new(TestServerState::default());
    let app = create_router(state.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base_url = format!("http://{addr}");

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let client = KrakenFuturesRawHttpClient::with_credentials(
        "test".to_string(),
        "test".to_string(),
        KrakenEnvironment::Mainnet,
        Some(base_url),
        Some(10),
        None,
        None,
        None,
        None,
    )
    .unwrap();

    let result = client
        .cancel_order(
            Some("c8a35168-8d52-4609-944f-3f32bb0d5c77".to_string()),
            None,
        )
        .await;
    assert!(result.is_ok(), "Failed to cancel order: {result:?}");

    let response = result.unwrap();
    assert_eq!(response.result, KrakenApiResult::Success);
    assert_eq!(response.cancel_status.status, "cancelled");
}
