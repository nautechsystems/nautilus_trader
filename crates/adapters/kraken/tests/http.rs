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
use nautilus_kraken::http::client::{KrakenHttpClient, KrakenRawHttpClient};
use nautilus_model::{data::BarType, identifiers::InstrumentId, instruments::InstrumentAny};
use rstest::rstest;
use serde_json::Value;
use tokio::sync::Mutex;

#[derive(Clone, Default)]
struct TestServerState {
    request_count: Arc<AtomicUsize>,
    last_trades_query: Arc<Mutex<Option<HashMap<String, String>>>>,
    last_ohlc_query: Arc<Mutex<Option<HashMap<String, String>>>>,
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

async fn mock_handler(req: Request, state: Arc<TestServerState>) -> Response {
    state.request_count.fetch_add(1, Ordering::Relaxed);

    let path = req.uri().path();

    if path.starts_with("/derivatives/api/v3/") {
        return match path {
            "/derivatives/api/v3/instruments" => mock_futures_instruments().await,
            "/derivatives/api/v3/tickers" => mock_futures_tickers().await,
            _ => Response::builder()
                .status(StatusCode::NOT_FOUND)
                .body(Body::from("Futures endpoint not found"))
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

// Tests

#[rstest]
#[tokio::test]
async fn test_http_get_server_time() {
    let state = Arc::new(TestServerState::default());
    let app = create_router(state.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base_url = format!("http://{addr}");

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let client =
        KrakenRawHttpClient::new(Some(base_url), Some(10), None, None, None, None).unwrap();

    let result = client.get_server_time().await;
    assert!(result.is_ok(), "Failed to get server time: {result:?}");

    let server_time = result.unwrap();
    assert!(server_time.unixtime > 0);
    assert!(!server_time.rfc1123.is_empty());
}

#[rstest]
#[tokio::test]
async fn test_http_get_system_status() {
    let state = Arc::new(TestServerState::default());
    let app = create_router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base_url = format!("http://{addr}");

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let client =
        KrakenRawHttpClient::new(Some(base_url), Some(10), None, None, None, None).unwrap();

    let result = client.get_system_status().await;
    assert!(result.is_ok(), "Failed to get system status: {result:?}");

    let status = result.unwrap();
    assert!(!status.timestamp.is_empty());
}

#[rstest]
#[tokio::test]
async fn test_http_get_asset_pairs() {
    let state = Arc::new(TestServerState::default());
    let app = create_router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base_url = format!("http://{addr}");

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let client =
        KrakenRawHttpClient::new(Some(base_url), Some(10), None, None, None, None).unwrap();

    let result = client.get_asset_pairs(None).await;
    assert!(result.is_ok(), "Failed to get asset pairs: {result:?}");

    let pairs = result.unwrap();
    assert!(!pairs.is_empty());
    assert!(pairs.contains_key("XBTUSDT"));
}

#[rstest]
#[tokio::test]
async fn test_http_request_instruments() {
    let state = Arc::new(TestServerState::default());
    let app = create_router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base_url = format!("http://{addr}");

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let client = KrakenHttpClient::new(Some(base_url), Some(10), None, None, None, None).unwrap();

    let result = client.request_instruments(None).await;
    assert!(result.is_ok(), "Failed to request instruments: {result:?}");

    let instruments: Vec<InstrumentAny> = result.unwrap();
    assert!(!instruments.is_empty());
}

#[rstest]
#[tokio::test]
async fn test_http_get_ticker() {
    let state = Arc::new(TestServerState::default());
    let app = create_router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base_url = format!("http://{addr}");

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let client =
        KrakenRawHttpClient::new(Some(base_url), Some(10), None, None, None, None).unwrap();

    let result = client.get_ticker(vec!["XBTUSDT".to_string()]).await;
    assert!(result.is_ok(), "Failed to get ticker: {result:?}");

    let ticker = result.unwrap();
    assert!(ticker.contains_key("XBTUSDT"));
}

#[rstest]
#[tokio::test]
async fn test_http_get_book_depth() {
    let state = Arc::new(TestServerState::default());
    let app = create_router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base_url = format!("http://{addr}");

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let client =
        KrakenRawHttpClient::new(Some(base_url), Some(10), None, None, None, None).unwrap();

    let result = client.get_book_depth("XBTUSDT", None).await;
    assert!(result.is_ok(), "Failed to get book depth: {result:?}");

    let book = result.unwrap();
    assert!(book.contains_key("XBTUSDT"));
}

#[rstest]
#[tokio::test]
async fn test_http_get_trades() {
    let state = Arc::new(TestServerState::default());
    let app = create_router(state.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base_url = format!("http://{addr}");

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let client =
        KrakenRawHttpClient::new(Some(base_url), Some(10), None, None, None, None).unwrap();

    let result = client.get_trades("XBTUSDT", None).await;
    assert!(result.is_ok(), "Failed to get trades: {result:?}");

    let response = result.unwrap();
    assert!(!response.data.is_empty());
}

#[rstest]
#[tokio::test]
async fn test_http_get_ohlc() {
    let state = Arc::new(TestServerState::default());
    let app = create_router(state.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base_url = format!("http://{addr}");

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let client =
        KrakenRawHttpClient::new(Some(base_url), Some(10), None, None, None, None).unwrap();

    let result = client.get_ohlc("XBTUSDT", Some(60), None).await;
    assert!(result.is_ok(), "Failed to get OHLC: {result:?}");

    let response = result.unwrap();
    assert!(!response.data.is_empty());
}

#[rstest]
#[tokio::test]
async fn test_http_get_trades_with_since() {
    let state = Arc::new(TestServerState::default());
    let app = create_router(state.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base_url = format!("http://{addr}");

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let client =
        KrakenRawHttpClient::new(Some(base_url), Some(10), None, None, None, None).unwrap();

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
async fn test_http_get_ohlc_with_interval() {
    let state = Arc::new(TestServerState::default());
    let app = create_router(state.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base_url = format!("http://{addr}");

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let client =
        KrakenRawHttpClient::new(Some(base_url), Some(10), None, None, None, None).unwrap();

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
async fn test_http_get_websockets_token_requires_credentials() {
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
    let client =
        KrakenRawHttpClient::new(Some(base_url), Some(10), None, None, None, None).unwrap();

    let result = client.get_websockets_token().await;
    assert!(result.is_err());
    assert!(
        matches!(result, Err(e) if e.to_string().contains("credentials")),
        "Expected authentication error"
    );
}

#[rstest]
#[tokio::test]
async fn test_http_get_websockets_token_with_credentials() {
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
    let client = KrakenRawHttpClient::with_credentials(
        "test_api_key".to_string(),
        "dGVzdF9hcGlfc2VjcmV0X2Jhc2U2NA==".to_string(),
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
async fn test_http_request_trades_with_domain_client() {
    let state = Arc::new(TestServerState::default());
    let app = create_router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base_url = format!("http://{addr}");

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let client = KrakenHttpClient::new(Some(base_url), Some(10), None, None, None, None).unwrap();

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
async fn test_http_request_bars_with_domain_client() {
    let state = Arc::new(TestServerState::default());
    let app = create_router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base_url = format!("http://{addr}");

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let client = KrakenHttpClient::new(Some(base_url), Some(10), None, None, None, None).unwrap();

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
async fn test_http_multiple_requests_increment_count() {
    let state = Arc::new(TestServerState::default());
    let app = create_router(state.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base_url = format!("http://{addr}");

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let client =
        KrakenRawHttpClient::new(Some(base_url), Some(10), None, None, None, None).unwrap();

    let initial_count = state.request_count.load(Ordering::Relaxed);

    client.get_server_time().await.unwrap();
    client.get_system_status().await.unwrap();
    client.get_asset_pairs(None).await.unwrap();

    let final_count = state.request_count.load(Ordering::Relaxed);
    assert_eq!(final_count - initial_count, 3);
}

#[rstest]
#[tokio::test]
async fn test_http_get_instruments_futures() {
    let state = Arc::new(TestServerState::default());
    let app = create_router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base_url = format!("http://{addr}");

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let client =
        KrakenRawHttpClient::new(Some(base_url), Some(10), None, None, None, None).unwrap();

    let result = client.get_instruments_futures().await;
    assert!(
        result.is_ok(),
        "Failed to get futures instruments: {result:?}"
    );

    let response = result.unwrap();
    assert_eq!(response.result, "success");
    assert!(!response.instruments.is_empty());
    assert_eq!(response.instruments[0].symbol, "PI_XBTUSD");
    assert_eq!(response.instruments[0].base, "BTC");
    assert_eq!(response.instruments[0].quote, "USD");
}

#[rstest]
#[tokio::test]
async fn test_http_get_tickers_futures() {
    let state = Arc::new(TestServerState::default());
    let app = create_router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base_url = format!("http://{addr}");

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let client =
        KrakenRawHttpClient::new(Some(base_url), Some(10), None, None, None, None).unwrap();

    let result = client.get_tickers_futures().await;
    assert!(result.is_ok(), "Failed to get futures tickers: {result:?}");

    let response = result.unwrap();
    assert_eq!(response.result, "success");
    assert!(!response.tickers.is_empty());

    let ticker = &response.tickers[0];
    assert_eq!(ticker.symbol, "PI_XBTUSD");
    assert!(ticker.mark_price > 0.0);
    assert!(ticker.index_price > 0.0);
}

#[rstest]
#[tokio::test]
async fn test_http_get_ohlc_futures_trade() {
    let state = Arc::new(TestServerState::default());
    let app = create_router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base_url = format!("http://{addr}");

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let client =
        KrakenRawHttpClient::new(Some(base_url), Some(10), None, None, None, None).unwrap();

    let result = client
        .get_ohlc_futures("trade", "PI_XBTUSD", "1h", None, None)
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
async fn test_http_get_ohlc_futures_mark() {
    let state = Arc::new(TestServerState::default());
    let app = create_router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base_url = format!("http://{addr}");

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let client =
        KrakenRawHttpClient::new(Some(base_url), Some(10), None, None, None, None).unwrap();

    let result = client
        .get_ohlc_futures("mark", "PI_XBTUSD", "1h", None, None)
        .await;
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
async fn test_http_get_ohlc_futures_spot() {
    let state = Arc::new(TestServerState::default());
    let app = create_router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base_url = format!("http://{addr}");

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let client =
        KrakenRawHttpClient::new(Some(base_url), Some(10), None, None, None, None).unwrap();

    let result = client
        .get_ohlc_futures("spot", "PI_XBTUSD", "1h", None, None)
        .await;
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
async fn test_http_request_mark_price_fails_on_spot_client() {
    let state = Arc::new(TestServerState::default());
    let app = create_router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base_url = format!("http://{addr}");

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // Create spot client (no "futures" in URL)
    let client = KrakenHttpClient::new(Some(base_url), Some(10), None, None, None, None).unwrap();

    let instrument_id = InstrumentId::from("BTC/USD.KRAKEN");
    let result = client.request_mark_price(instrument_id).await;

    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        err.to_string()
            .contains("Mark price is only available for futures"),
        "Expected futures-only error, got: {err}"
    );
}

#[rstest]
#[tokio::test]
async fn test_http_request_index_price_fails_on_spot_client() {
    let state = Arc::new(TestServerState::default());
    let app = create_router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base_url = format!("http://{addr}");

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // Create spot client (no "futures" in URL)
    let client = KrakenHttpClient::new(Some(base_url), Some(10), None, None, None, None).unwrap();

    let instrument_id = InstrumentId::from("BTC/USD.KRAKEN");
    let result = client.request_index_price(instrument_id).await;

    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        err.to_string()
            .contains("Index price is only available for futures"),
        "Expected futures-only error, got: {err}"
    );
}

#[rstest]
#[tokio::test]
async fn test_http_request_trades_fails_on_futures_client() {
    let state = Arc::new(TestServerState::default());
    let app = create_router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base_url = format!("http://{addr}/futures");

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // Create futures client ("futures" in URL)
    let client = KrakenHttpClient::new(Some(base_url), Some(10), None, None, None, None).unwrap();

    let instrument_id = InstrumentId::from("PI_XBTUSD.KRAKEN");
    let result = client.request_trades(instrument_id, None, None, None).await;

    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        err.to_string()
            .contains("Trade history is not yet implemented for futures"),
        "Expected not-implemented error, got: {err}"
    );
}
