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

//! Integration tests for the Kraken HTTP client using a mock Axum server.

use std::{
    collections::HashMap,
    net::SocketAddr,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use axum::{
    Router,
    body::Body,
    extract::{Query, Request},
    http::{HeaderMap, StatusCode},
    response::Response,
};
use nautilus_common::testing::wait_until_async;
use nautilus_kraken::{
    common::enums::{
        KrakenApiResult, KrakenEnvironment, KrakenOrderSide, KrakenOrderStatus, KrakenOrderType,
        KrakenSendStatus,
    },
    http::{
        KrakenFuturesHttpClient, KrakenFuturesRawHttpClient, KrakenSpotAddOrderParamsBuilder,
        KrakenSpotCancelOrderParamsBuilder, KrakenSpotHttpClient, KrakenSpotRawHttpClient,
    },
};
use nautilus_model::{
    data::BarType,
    enums::{
        MarketStatusAction, OrderSide as ModelOrderSide, OrderType as ModelOrderType, TimeInForce,
    },
    identifiers::{ClientOrderId, InstrumentId, Symbol, VenueOrderId},
    instruments::{CryptoPerpetual, CurrencyPair, Instrument, InstrumentAny},
    types::{Currency, Price, Quantity},
};
use nautilus_network::http::HttpClient;
use rstest::rstest;
use serde_json::Value;

#[derive(Clone)]
struct TestServerState {
    open_orders_count: Arc<AtomicUsize>,
    rate_limit_after: Arc<AtomicUsize>,
    last_trades_query: Arc<tokio::sync::Mutex<Option<HashMap<String, String>>>>,
    last_ohlc_query: Arc<tokio::sync::Mutex<Option<HashMap<String, String>>>>,
    add_order_calls: Arc<AtomicUsize>,
    add_order_batch_calls: Arc<AtomicUsize>,
}

impl Default for TestServerState {
    fn default() -> Self {
        Self {
            open_orders_count: Arc::new(AtomicUsize::new(0)),
            rate_limit_after: Arc::new(AtomicUsize::new(usize::MAX)), // No rate limit
            last_trades_query: Arc::new(tokio::sync::Mutex::new(None)),
            last_ohlc_query: Arc::new(tokio::sync::Mutex::new(None)),
            add_order_calls: Arc::new(AtomicUsize::new(0)),
            add_order_batch_calls: Arc::new(AtomicUsize::new(0)),
        }
    }
}

/// Wait for the test server to be ready by polling a health endpoint.
async fn wait_for_server(addr: SocketAddr, path: &str) {
    let health_url = format!("http://{addr}{path}");
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
        None,
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
        .unwrap_or_else(|e| panic!("Failed to load test data from {}: {e}", path.display()));
    serde_json::from_str(&content)
        .unwrap_or_else(|e| panic!("Failed to parse test data from {}: {e}", path.display()))
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

async fn mock_asset_pairs(aclass_base: Option<&str>) -> Response {
    let filename = match aclass_base {
        Some("tokenized_asset") => "http_asset_pairs_tokenized.json",
        _ => "http_asset_pairs.json",
    };
    let data = load_test_data(filename);
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

async fn mock_open_orders(state: Arc<TestServerState>) -> Response {
    let count = state.open_orders_count.fetch_add(1, Ordering::SeqCst) + 1;
    let limit = state.rate_limit_after.load(Ordering::SeqCst);

    if count > limit {
        return mock_rate_limit_error().await;
    }

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

async fn mock_futures_orderbook() -> Response {
    let data = load_test_data("http_futures_orderbook.json");
    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "application/json")
        .body(Body::from(data.to_string()))
        .unwrap()
}

async fn mock_futures_historical_funding_rates() -> Response {
    let data = load_test_data("http_futures_historical_funding_rates.json");
    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "application/json")
        .body(Body::from(data.to_string()))
        .unwrap()
}

async fn mock_handler(req: Request, state: Arc<TestServerState>) -> Response {
    let path = req.uri().path();

    if path.starts_with("/derivatives/api/v3/") || path.starts_with("/derivatives/api/v4/") {
        // Strip query string for matching (some endpoints embed params in the path)
        let match_path = path.split('?').next().unwrap_or(path);
        return match match_path {
            "/derivatives/api/v3/instruments" => mock_futures_instruments().await,
            "/derivatives/api/v3/tickers" => mock_futures_tickers().await,
            "/derivatives/api/v3/fills" => mock_futures_fills().await,
            "/derivatives/api/v3/openpositions" => mock_futures_open_positions().await,
            "/derivatives/api/v3/openorders" => mock_futures_open_orders().await,
            "/derivatives/api/v3/sendorder" => mock_send_order_futures().await,
            "/derivatives/api/v3/cancelorder" => mock_cancel_order_futures().await,
            "/derivatives/api/v3/editorder" => mock_cancel_order_futures().await,
            "/derivatives/api/v3/batchorder" => mock_batch_order_futures().await,
            "/derivatives/api/v3/cancelallorders" => mock_cancel_order_futures().await,
            "/derivatives/api/v3/orderbook" => mock_futures_orderbook().await,
            "/derivatives/api/v4/historicalfundingrates" => {
                mock_futures_historical_funding_rates().await
            }
            _ => Response::builder()
                .status(StatusCode::NOT_FOUND)
                .body(Body::from("Futures endpoint not found"))
                .unwrap(),
        };
    }

    if path.starts_with("/api/history/v2/") || path.starts_with("/api/history/v3/") {
        return match path {
            p if p.starts_with("/api/history/v2/orders") => mock_futures_order_events().await,
            p if p.contains("/market/") && p.contains("/executions") => {
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
        "/0/public/AssetPairs" => {
            let query =
                Query::<HashMap<String, String>>::try_from_uri(req.uri()).unwrap_or_default();
            let aclass_base = query.get("aclass_base").map(|s| s.as_str());
            mock_asset_pairs(aclass_base).await
        }
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
        "/0/private/OpenOrders" => mock_open_orders(state.clone()).await,
        "/0/private/ClosedOrders" => mock_closed_orders().await,
        "/0/private/TradesHistory" => mock_trades_history().await,
        "/0/private/AddOrder" => {
            state.add_order_calls.fetch_add(1, Ordering::Relaxed);
            mock_add_order_spot().await
        }
        "/0/private/AddOrderBatch" => {
            state.add_order_batch_calls.fetch_add(1, Ordering::Relaxed);
            mock_add_order_batch_spot().await
        }
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

async fn mock_batch_order_futures() -> Response {
    let response = r#"{
        "result": "success",
        "serverTime": "2024-01-01T00:00:00.000Z",
        "batchStatus": [
            {"status": "edited", "order_id": "batch-edit-1"},
            {"status": "insufficientAvailableFunds", "order_id": "batch-edit-2"}
        ]
    }"#;

    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "application/json")
        .body(Body::from(response))
        .unwrap()
}

async fn mock_add_order_batch_spot() -> Response {
    let response = r#"{
        "error": [],
        "result": {
            "orders": [
                {"txid": "batch-spot-1"},
                {"error": "EOrder:Post only order"}
            ]
        }
    }"#;

    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "application/json")
        .body(Body::from(response))
        .unwrap()
}

fn create_router(state: Arc<TestServerState>) -> Router {
    Router::new().fallback(move |req| {
        let state = state.clone();
        async move { mock_handler(req, state).await }
    })
}

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

    wait_for_server(addr, "/0/public/Time").await;

    let client = KrakenSpotRawHttpClient::new(
        KrakenEnvironment::Mainnet,
        Some(base_url),
        10,
        None,
        None,
        None,
        None,
        5,
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

    wait_for_server(addr, "/0/public/Time").await;

    let client = KrakenSpotRawHttpClient::new(
        KrakenEnvironment::Mainnet,
        Some(base_url),
        10,
        None,
        None,
        None,
        None,
        5,
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

    wait_for_server(addr, "/0/public/Time").await;

    let client = KrakenSpotRawHttpClient::new(
        KrakenEnvironment::Mainnet,
        Some(base_url),
        10,
        None,
        None,
        None,
        None,
        5,
    )
    .unwrap();

    let result = client.get_asset_pairs(None, None).await;
    assert!(result.is_ok(), "Failed to get asset pairs: {result:?}");

    let pairs = result.unwrap();
    assert!(!pairs.is_empty());
    assert!(pairs.contains_key("XBTUSDT"));
}

#[rstest]
#[tokio::test]
async fn test_spot_raw_get_asset_pairs_tokenized() {
    let state = Arc::new(TestServerState::default());
    let app = create_router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base_url = format!("http://{addr}");

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    wait_for_server(addr, "/0/public/Time").await;

    let client = KrakenSpotRawHttpClient::new(
        KrakenEnvironment::Mainnet,
        Some(base_url),
        10,
        None,
        None,
        None,
        None,
        5,
    )
    .unwrap();

    let result = client.get_asset_pairs(None, Some("tokenized_asset")).await;
    assert!(
        result.is_ok(),
        "Failed to get tokenized asset pairs: {result:?}"
    );

    let pairs = result.unwrap();
    assert!(!pairs.is_empty());
    assert!(pairs.contains_key("AAPLxUSD"));
}

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

    wait_for_server(addr, "/0/public/Time").await;

    let client = KrakenSpotHttpClient::new(
        KrakenEnvironment::Mainnet,
        Some(base_url),
        10,
        None,
        None,
        None,
        None,
        5,
    )
    .unwrap();

    let result = client.request_instruments(None).await;
    assert!(result.is_ok(), "Failed to request instruments: {result:?}");

    let instruments: Vec<InstrumentAny> = result.unwrap();
    assert!(!instruments.is_empty());

    let has_currency_pair = instruments
        .iter()
        .any(|i| matches!(i, InstrumentAny::CurrencyPair(_)));
    let has_tokenized = instruments
        .iter()
        .any(|i| matches!(i, InstrumentAny::TokenizedAsset(_)));

    assert!(has_currency_pair, "Expected at least one CurrencyPair");
    assert!(has_tokenized, "Expected at least one TokenizedAsset");
}

#[rstest]
#[tokio::test]
async fn test_spot_domain_request_instrument_statuses() {
    let state = Arc::new(TestServerState::default());
    let app = create_router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base_url = format!("http://{addr}");

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    wait_for_server(addr, "/0/public/Time").await;

    let client = KrakenSpotHttpClient::new(
        KrakenEnvironment::Mainnet,
        Some(base_url),
        10,
        None,
        None,
        None,
        None,
        5,
    )
    .unwrap();

    let statuses = client.request_instrument_statuses(None).await.unwrap();

    assert_eq!(
        statuses.get(&InstrumentId::from("BTC/USDT.KRAKEN")),
        Some(&MarketStatusAction::Trading),
    );
    assert_eq!(
        statuses.get(&InstrumentId::from("AAPLx/USD.KRAKEN")),
        Some(&MarketStatusAction::Trading),
    );
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

    wait_for_server(addr, "/0/public/Time").await;

    let client = KrakenSpotRawHttpClient::new(
        KrakenEnvironment::Mainnet,
        Some(base_url),
        10,
        None,
        None,
        None,
        None,
        5,
    )
    .unwrap();

    let result = client.get_ticker(vec!["XBTUSDT".to_string()], None).await;
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

    wait_for_server(addr, "/0/public/Time").await;

    let client = KrakenSpotRawHttpClient::new(
        KrakenEnvironment::Mainnet,
        Some(base_url),
        10,
        None,
        None,
        None,
        None,
        5,
    )
    .unwrap();

    let result = client.get_book_depth("XBTUSDT", None, None).await;
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

    wait_for_server(addr, "/0/public/Time").await;

    let client = KrakenSpotRawHttpClient::new(
        KrakenEnvironment::Mainnet,
        Some(base_url),
        10,
        None,
        None,
        None,
        None,
        5,
    )
    .unwrap();

    let result = client.get_trades("XBTUSDT", None, None).await;
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

    wait_for_server(addr, "/0/public/Time").await;

    let client = KrakenSpotRawHttpClient::new(
        KrakenEnvironment::Mainnet,
        Some(base_url),
        10,
        None,
        None,
        None,
        None,
        5,
    )
    .unwrap();

    let result = client.get_ohlc("XBTUSDT", Some(60), None, None).await;
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

    wait_for_server(addr, "/0/public/Time").await;

    let client = KrakenSpotRawHttpClient::new(
        KrakenEnvironment::Mainnet,
        Some(base_url),
        10,
        None,
        None,
        None,
        None,
        5,
    )
    .unwrap();

    let since = "1234567890".to_string();
    let result = client
        .get_trades("XBTUSDT", Some(since.clone()), None)
        .await;
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

    wait_for_server(addr, "/0/public/Time").await;

    let client = KrakenSpotRawHttpClient::new(
        KrakenEnvironment::Mainnet,
        Some(base_url),
        10,
        None,
        None,
        None,
        None,
        5,
    )
    .unwrap();

    let result = client.get_ohlc("XBTUSDT", Some(60), None, None).await;
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

    wait_for_server(addr, "/0/public/Time").await;

    // Client without credentials
    let client = KrakenSpotRawHttpClient::new(
        KrakenEnvironment::Mainnet,
        Some(base_url),
        10,
        None,
        None,
        None,
        None,
        5,
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

    wait_for_server(addr, "/0/public/Time").await;

    // Client with credentials (API secret must be base64-encoded)
    let client = KrakenSpotRawHttpClient::with_credentials(
        "test_api_key".to_string(),
        "dGVzdF9hcGlfc2VjcmV0X2Jhc2U2NA==".to_string(),
        KrakenEnvironment::Mainnet,
        Some(base_url),
        10,
        None,
        None,
        None,
        None,
        5,
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

    wait_for_server(addr, "/0/public/Time").await;

    let client = KrakenSpotHttpClient::new(
        KrakenEnvironment::Mainnet,
        Some(base_url),
        10,
        None,
        None,
        None,
        None,
        5,
    )
    .unwrap();

    // First load instruments to populate cache
    let instruments = client.request_instruments(None).await.unwrap();
    client.cache_instruments(&instruments);

    // Create a valid instrument ID from cached instruments (normalized to BTC)
    let instrument_id = InstrumentId::from("BTC/USDT.KRAKEN");

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

    wait_for_server(addr, "/0/public/Time").await;

    let client = KrakenSpotHttpClient::new(
        KrakenEnvironment::Mainnet,
        Some(base_url),
        10,
        None,
        None,
        None,
        None,
        5,
    )
    .unwrap();

    // First load instruments to populate cache
    let instruments = client.request_instruments(None).await.unwrap();
    client.cache_instruments(&instruments);

    // Create a BarType for 1-minute bars (normalized to BTC)
    let bar_type = BarType::from("BTC/USDT.KRAKEN-1-MINUTE-LAST-INTERNAL");

    let result = client.request_bars(bar_type, None, None, None).await;
    assert!(result.is_ok(), "Failed to request bars: {result:?}");

    let bars = result.unwrap();
    assert!(!bars.is_empty());
}

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

    wait_for_server(addr, "/0/public/Time").await;

    let client = KrakenFuturesRawHttpClient::new(
        KrakenEnvironment::Mainnet,
        Some(base_url),
        10,
        None,
        None,
        None,
        None,
        5,
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
async fn test_futures_domain_request_instrument_statuses() {
    let state = Arc::new(TestServerState::default());
    let app = create_router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base_url = format!("http://{addr}");

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    wait_for_server(addr, "/0/public/Time").await;

    let client = KrakenFuturesHttpClient::new(
        KrakenEnvironment::Mainnet,
        Some(base_url),
        10,
        None,
        None,
        None,
        None,
        5,
    )
    .unwrap();

    let statuses = client.request_instrument_statuses().await.unwrap();

    assert_eq!(
        statuses.get(&InstrumentId::from("PI_XBTUSD.KRAKEN")),
        Some(&MarketStatusAction::Trading),
    );
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

    wait_for_server(addr, "/0/public/Time").await;

    let client = KrakenFuturesRawHttpClient::new(
        KrakenEnvironment::Mainnet,
        Some(base_url),
        10,
        None,
        None,
        None,
        None,
        5,
    )
    .unwrap();

    let result = client.get_tickers().await;
    assert!(result.is_ok(), "Failed to get futures tickers: {result:?}");

    let response = result.unwrap();
    assert_eq!(response.result, KrakenApiResult::Success);
    assert!(!response.tickers.is_empty());

    let ticker = &response.tickers[0];
    assert_eq!(ticker.symbol, "PI_XBTUSD");
    assert!(ticker.mark_price.is_some());
    assert!(ticker.mark_price.unwrap() > 0.0);
    assert!(ticker.index_price.is_some());
    assert!(ticker.index_price.unwrap() > 0.0);
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

    wait_for_server(addr, "/0/public/Time").await;

    let client = KrakenFuturesRawHttpClient::new(
        KrakenEnvironment::Mainnet,
        Some(base_url),
        10,
        None,
        None,
        None,
        None,
        5,
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

    wait_for_server(addr, "/0/public/Time").await;

    let client = KrakenFuturesRawHttpClient::new(
        KrakenEnvironment::Mainnet,
        Some(base_url),
        10,
        None,
        None,
        None,
        None,
        5,
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

    wait_for_server(addr, "/0/public/Time").await;

    let client = KrakenFuturesRawHttpClient::new(
        KrakenEnvironment::Mainnet,
        Some(base_url),
        10,
        None,
        None,
        None,
        None,
        5,
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

    wait_for_server(addr, "/0/public/Time").await;

    let client = KrakenFuturesRawHttpClient::new(
        KrakenEnvironment::Mainnet,
        Some(base_url),
        10,
        None,
        None,
        None,
        None,
        5,
    )
    .unwrap();

    let result = client
        .get_public_executions("PF_XBTUSD", None, None, None, None)
        .await;

    assert!(result.is_ok(), "Expected success, was: {:?}", result.err());
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

    wait_for_server(addr, "/0/public/Time").await;

    let client = KrakenSpotRawHttpClient::with_credentials(
        "test".to_string(),
        "test".to_string(),
        KrakenEnvironment::Mainnet,
        Some(base_url),
        10,
        None,
        None,
        None,
        None,
        5,
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

    wait_for_server(addr, "/0/public/Time").await;

    let client = KrakenSpotRawHttpClient::with_credentials(
        "test".to_string(),
        "test".to_string(),
        KrakenEnvironment::Mainnet,
        Some(base_url),
        10,
        None,
        None,
        None,
        None,
        5,
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

    wait_for_server(addr, "/0/public/Time").await;

    let client = KrakenSpotRawHttpClient::with_credentials(
        "test".to_string(),
        "test".to_string(),
        KrakenEnvironment::Mainnet,
        Some(base_url),
        10,
        None,
        None,
        None,
        None,
        5,
    )
    .unwrap();

    let result = client
        .get_trades_history(None, Some(true), None, None, None)
        .await;
    assert!(result.is_ok(), "Failed to get trades history: {result:?}");

    let trades = result.unwrap();
    assert!(!trades.is_empty());
}

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

    wait_for_server(addr, "/0/public/Time").await;

    let client = KrakenFuturesRawHttpClient::with_credentials(
        "test".to_string(),
        "test".to_string(),
        KrakenEnvironment::Mainnet,
        Some(base_url),
        10,
        None,
        None,
        None,
        None,
        5,
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

    wait_for_server(addr, "/0/public/Time").await;

    let client = KrakenFuturesRawHttpClient::with_credentials(
        "test".to_string(),
        "test".to_string(),
        KrakenEnvironment::Mainnet,
        Some(base_url),
        10,
        None,
        None,
        None,
        None,
        5,
    )
    .unwrap();

    let result = client.get_order_events(None, None, None).await;
    assert!(
        result.is_ok(),
        "Failed to get futures order events: {result:?}"
    );

    let response = result.unwrap();
    assert_eq!(response.order_events.len(), 3);

    let first_event = &response.order_events[0].order;
    assert_eq!(first_event.order_id, "c8a35168-8d52-4609-944f-3f32bb0d5c77");
    assert_eq!(first_event.symbol, "PI_XBTUSD");
    assert_eq!(first_event.filled, 5000.0);
    assert_eq!(first_event.quantity, 5000.0);

    let third_event = &response.order_events[2].order;
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

    wait_for_server(addr, "/0/public/Time").await;

    let client = KrakenFuturesRawHttpClient::with_credentials(
        "test".to_string(),
        "test".to_string(),
        KrakenEnvironment::Mainnet,
        Some(base_url),
        10,
        None,
        None,
        None,
        None,
        5,
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

    wait_for_server(addr, "/0/public/Time").await;

    let client = KrakenFuturesRawHttpClient::with_credentials(
        "test".to_string(),
        "test".to_string(),
        KrakenEnvironment::Mainnet,
        Some(base_url),
        10,
        None,
        None,
        None,
        None,
        5,
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

    wait_for_server(addr, "/0/public/Time").await;

    let client = KrakenSpotRawHttpClient::with_credentials(
        "test".to_string(),
        "test".to_string(),
        KrakenEnvironment::Mainnet,
        Some(base_url),
        10,
        None,
        None,
        None,
        None,
        5,
    )
    .unwrap();

    let params = KrakenSpotAddOrderParamsBuilder::default()
        .pair("XBTUSD")
        .side(KrakenOrderSide::Buy)
        .order_type(KrakenOrderType::Limit)
        .volume("0.01")
        .price("50000")
        .build()
        .unwrap();

    let result = client.add_order(&params).await;
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

    wait_for_server(addr, "/0/public/Time").await;

    let client = KrakenSpotRawHttpClient::with_credentials(
        "test".to_string(),
        "test".to_string(),
        KrakenEnvironment::Mainnet,
        Some(base_url),
        10,
        None,
        None,
        None,
        None,
        5,
    )
    .unwrap();

    let params = KrakenSpotCancelOrderParamsBuilder::default()
        .txid("OUF4EM-FRGI2-MQMWZD")
        .build()
        .unwrap();

    let result = client.cancel_order(&params).await;
    assert!(result.is_ok(), "Failed to cancel order: {result:?}");

    let response = result.unwrap();
    assert_eq!(response.count, 1);
}

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

    wait_for_server(addr, "/0/public/Time").await;

    let client = KrakenFuturesRawHttpClient::with_credentials(
        "test".to_string(),
        "test".to_string(),
        KrakenEnvironment::Mainnet,
        Some(base_url),
        10,
        None,
        None,
        None,
        None,
        5,
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

    wait_for_server(addr, "/0/public/Time").await;

    let client = KrakenFuturesRawHttpClient::with_credentials(
        "test".to_string(),
        "test".to_string(),
        KrakenEnvironment::Mainnet,
        Some(base_url),
        10,
        None,
        None,
        None,
        None,
        5,
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
    assert_eq!(response.cancel_status.status, KrakenSendStatus::Cancelled);
}

#[rstest]
#[tokio::test]
async fn test_spot_raw_rate_limit_error() {
    let state = Arc::new(TestServerState::default());
    state.rate_limit_after.store(3, Ordering::SeqCst);

    let app = create_router(state.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base_url = format!("http://{addr}");

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    wait_for_server(addr, "/0/public/Time").await;

    // API secret must be base64-encoded
    let api_secret = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, "secret");
    let client = KrakenSpotRawHttpClient::with_credentials(
        "test_key".to_string(),
        api_secret,
        KrakenEnvironment::Mainnet,
        Some(base_url),
        10,
        None,
        None,
        None,
        None,
        5,
    )
    .unwrap();

    let mut last_error = None;
    for _ in 0..10 {
        match client.get_open_orders(None, None).await {
            Ok(_) => {}
            Err(e) => {
                last_error = Some(e);
                break;
            }
        }
    }

    assert!(last_error.is_some(), "Expected rate limit error");
    let error = last_error.unwrap();
    assert!(
        error.to_string().contains("Rate limit")
            || error.to_string().contains("429")
            || error.to_string().contains("TOO_MANY"),
        "Expected rate limit error message, was: {error}"
    );
}

#[rstest]
#[tokio::test]
async fn test_spot_raw_api_error_response() {
    let state = Arc::new(TestServerState::default());
    let app = create_router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base_url = format!("http://{addr}");

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    wait_for_server(addr, "/0/public/Time").await;

    let client = KrakenSpotRawHttpClient::new(
        KrakenEnvironment::Mainnet,
        Some(base_url),
        10,
        None,
        None,
        None,
        None,
        5,
    )
    .unwrap();

    let result = client.get_websockets_token().await;
    assert!(result.is_err());

    let error = result.unwrap_err();
    assert!(
        error.to_string().contains("credentials") || error.to_string().contains("Missing"),
        "Expected credentials error, was: {error}"
    );
}

#[rstest]
#[tokio::test]
async fn test_futures_domain_request_trades() {
    let state = Arc::new(TestServerState::default());
    let app = create_router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base_url = format!("http://{addr}");

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    wait_for_server(addr, "/0/public/Time").await;

    let client = KrakenFuturesHttpClient::new(
        KrakenEnvironment::Mainnet,
        Some(base_url),
        10,
        None,
        None,
        None,
        None,
        5,
    )
    .unwrap();

    let instruments = client.request_instruments().await.unwrap();
    client.cache_instruments(&instruments);

    // PF_ETHUSD is in mock instruments; trades may be partially parsed
    // due to mock execution data having BTC-level prices
    let instrument_id = InstrumentId::from("PF_ETHUSD.KRAKEN");

    let result = client.request_trades(instrument_id, None, None, None).await;
    assert!(
        result.is_ok(),
        "Failed to request futures trades: {result:?}"
    );
}

#[rstest]
#[tokio::test]
async fn test_futures_domain_request_instruments_includes_tokenized_contract() {
    let state = Arc::new(TestServerState::default());
    let app = create_router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base_url = format!("http://{addr}");

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    wait_for_server(addr, "/0/public/Time").await;

    let client = KrakenFuturesHttpClient::new(
        KrakenEnvironment::Mainnet,
        Some(base_url),
        10,
        None,
        None,
        None,
        None,
        5,
    )
    .unwrap();

    let instruments = client.request_instruments().await.unwrap();

    let tokenized_future = instruments
        .iter()
        .find(|instrument| instrument.raw_symbol().as_str() == "PF_AAPLxUSD")
        .expect("Expected tokenized futures instrument");

    match tokenized_future {
        InstrumentAny::CryptoPerpetual(perp) => {
            assert_eq!(perp.id.symbol.as_str(), "PF_AAPLxUSD");
            assert_eq!(perp.base_currency.code.as_str(), "AAPLx");
            assert_eq!(perp.quote_currency.code.as_str(), "USD");
            assert_eq!(perp.size_increment.as_f64(), 0.01);
        }
        _ => panic!("Expected CryptoPerpetual"),
    }
}

#[rstest]
#[tokio::test]
async fn test_futures_domain_request_bars() {
    let state = Arc::new(TestServerState::default());
    let app = create_router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base_url = format!("http://{addr}");

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    wait_for_server(addr, "/0/public/Time").await;

    let client = KrakenFuturesHttpClient::new(
        KrakenEnvironment::Mainnet,
        Some(base_url),
        10,
        None,
        None,
        None,
        None,
        5,
    )
    .unwrap();

    let instruments = client.request_instruments().await.unwrap();
    client.cache_instruments(&instruments);

    let bar_type = BarType::from("PI_XBTUSD.KRAKEN-1-HOUR-LAST-INTERNAL");

    let result = client.request_bars(bar_type, None, None, None).await;
    assert!(result.is_ok(), "Failed to request futures bars: {result:?}");

    let bars = result.unwrap();
    assert!(!bars.is_empty());
}

#[rstest]
#[tokio::test]
async fn test_spot_domain_request_book_snapshot() {
    let state = Arc::new(TestServerState::default());
    let app = create_router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base_url = format!("http://{addr}");

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    wait_for_server(addr, "/0/public/Time").await;

    let client = KrakenSpotHttpClient::new(
        KrakenEnvironment::Mainnet,
        Some(base_url),
        10,
        None,
        None,
        None,
        None,
        5,
    )
    .unwrap();

    let instruments = client.request_instruments(None).await.unwrap();
    client.cache_instruments(&instruments);

    let instrument_id = InstrumentId::from("BTC/USDT.KRAKEN");
    let result = client.request_book_snapshot(instrument_id, Some(5)).await;
    assert!(
        result.is_ok(),
        "Failed to request book snapshot: {result:?}"
    );

    let book = result.unwrap();
    assert!(book.best_bid_price().is_some());
    assert!(book.best_ask_price().is_some());
    // HTTP snapshot must not advance the book's high-water sequence; the WS
    // subscription owns sequencing once it starts streaming deltas.
    assert_eq!(book.sequence, 0);
}

#[rstest]
#[tokio::test]
async fn test_futures_domain_request_book_snapshot() {
    let state = Arc::new(TestServerState::default());
    let app = create_router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base_url = format!("http://{addr}");

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    wait_for_server(addr, "/0/public/Time").await;

    let client = KrakenFuturesHttpClient::new(
        KrakenEnvironment::Mainnet,
        Some(base_url),
        10,
        None,
        None,
        None,
        None,
        5,
    )
    .unwrap();

    let instruments = client.request_instruments().await.unwrap();
    client.cache_instruments(&instruments);

    let instrument_id = InstrumentId::from("PF_ETHUSD.KRAKEN");
    let result = client.request_book_snapshot(instrument_id, None).await;
    assert!(
        result.is_ok(),
        "Failed to request futures book snapshot: {result:?}"
    );

    let book = result.unwrap();
    assert_eq!(book.best_bid_price(), Some(Price::from("105900.0")));
    assert_eq!(book.best_ask_price(), Some(Price::from("105950.0")));
    // HTTP snapshot must not advance the book's high-water sequence; the WS
    // subscription owns sequencing once it starts streaming deltas.
    assert_eq!(book.sequence, 0);
}

#[rstest]
#[tokio::test]
async fn test_futures_domain_request_funding_rates() {
    let state = Arc::new(TestServerState::default());
    let app = create_router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base_url = format!("http://{addr}");

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    wait_for_server(addr, "/0/public/Time").await;

    let client = KrakenFuturesHttpClient::new(
        KrakenEnvironment::Mainnet,
        Some(base_url),
        10,
        None,
        None,
        None,
        None,
        5,
    )
    .unwrap();

    let instruments = client.request_instruments().await.unwrap();
    client.cache_instruments(&instruments);

    let instrument_id = InstrumentId::from("PF_ETHUSD.KRAKEN");
    let result = client
        .request_funding_rates(instrument_id, None, None, None)
        .await;
    assert!(
        result.is_ok(),
        "Failed to request funding rates: {result:?}"
    );

    let rates = result.unwrap();
    assert_eq!(rates.len(), 3);
    assert_eq!(rates[0].instrument_id, instrument_id);

    // Rates are returned in ascending chronological order (oldest first)
    assert!(rates[0].ts_event < rates[1].ts_event);
    assert!(rates[1].ts_event < rates[2].ts_event);
}

#[rstest]
#[tokio::test]
async fn test_spot_domain_submit_orders_batch_preserves_status_order() {
    let state = Arc::new(TestServerState::default());
    let app = create_router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base_url = format!("http://{addr}");

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    wait_for_server(addr, "/0/public/Time").await;

    let client = KrakenSpotHttpClient::with_credentials(
        "test".to_string(),
        "test".to_string(),
        KrakenEnvironment::Mainnet,
        Some(base_url),
        10,
        None,
        None,
        None,
        None,
        5,
    )
    .unwrap();

    let instrument_id = InstrumentId::from("XBT/USD.KRAKEN");
    client.cache_instrument(InstrumentAny::CurrencyPair(CurrencyPair::new(
        instrument_id,
        Symbol::new("XBTUSD"),
        Currency::BTC(),
        Currency::USD(),
        1,
        8,
        Price::from("0.1"),
        Quantity::from("0.00000001"),
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
        None,
        0.into(),
        0.into(),
    )));

    let statuses = client
        .submit_orders_batch(vec![
            (
                InstrumentId::from("ETH/USD.KRAKEN"),
                ClientOrderId::new("missing-cache"),
                ModelOrderSide::Buy,
                ModelOrderType::Limit,
                Quantity::from("0.01"),
                TimeInForce::Gtc,
                None,
                Some(Price::from("50000")),
                None,
                None,
                None,
                None,
                false,
                false,
                false,
                None,
            ),
            (
                instrument_id,
                ClientOrderId::new("batch-ok-1"),
                ModelOrderSide::Buy,
                ModelOrderType::Limit,
                Quantity::from("0.02"),
                TimeInForce::Gtc,
                None,
                Some(Price::from("50010")),
                None,
                None,
                None,
                None,
                false,
                false,
                false,
                None,
            ),
            (
                instrument_id,
                ClientOrderId::new("batch-ok-2"),
                ModelOrderSide::Sell,
                ModelOrderType::Limit,
                Quantity::from("0.03"),
                TimeInForce::Gtc,
                None,
                Some(Price::from("50020")),
                None,
                None,
                None,
                None,
                false,
                true,
                false,
                None,
            ),
        ])
        .await
        .unwrap();

    assert_eq!(statuses.len(), 3);
    assert!(statuses[0].starts_with("validation_error:"));
    assert_eq!(statuses[1], "placed");
    assert_eq!(statuses[2], "EOrder:Post only order");
}

#[rstest]
#[tokio::test]
async fn test_spot_domain_submit_orders_batch_singleton_falls_back_to_add_order() {
    let state = Arc::new(TestServerState::default());
    let app = create_router(state.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base_url = format!("http://{addr}");

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    wait_for_server(addr, "/0/public/Time").await;

    let client = KrakenSpotHttpClient::with_credentials(
        "test".to_string(),
        "test".to_string(),
        KrakenEnvironment::Mainnet,
        Some(base_url),
        10,
        None,
        None,
        None,
        None,
        5,
    )
    .unwrap();

    let instrument_id = InstrumentId::from("XBT/USD.KRAKEN");
    client.cache_instrument(InstrumentAny::CurrencyPair(CurrencyPair::new(
        instrument_id,
        Symbol::new("XBTUSD"),
        Currency::BTC(),
        Currency::USD(),
        1,
        8,
        Price::from("0.1"),
        Quantity::from("0.00000001"),
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
        None,
        0.into(),
        0.into(),
    )));

    let statuses = client
        .submit_orders_batch(vec![(
            instrument_id,
            ClientOrderId::new("batch-singleton"),
            ModelOrderSide::Buy,
            ModelOrderType::Limit,
            Quantity::from("0.01"),
            TimeInForce::Gtc,
            None,
            Some(Price::from("50000")),
            None,
            None,
            None,
            None,
            false,
            false,
            false,
            None,
        )])
        .await
        .unwrap();

    assert_eq!(statuses, vec!["placed".to_string()]);
    assert_eq!(state.add_order_calls.load(Ordering::Relaxed), 1);
    assert_eq!(state.add_order_batch_calls.load(Ordering::Relaxed), 0);
}

#[rstest]
#[tokio::test]
async fn test_futures_domain_edit_orders_batch_preserves_status_order() {
    let state = Arc::new(TestServerState::default());
    let app = create_router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base_url = format!("http://{addr}");

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    wait_for_server(addr, "/0/public/Time").await;

    let client = KrakenFuturesHttpClient::with_credentials(
        "test".to_string(),
        "test".to_string(),
        KrakenEnvironment::Mainnet,
        Some(base_url),
        10,
        None,
        None,
        None,
        None,
        5,
    )
    .unwrap();

    let instrument = create_test_futures_instrument();
    client.cache_instrument(instrument);

    let instrument_id = InstrumentId::from("PF_XBTUSD.KRAKEN");
    let statuses = client
        .edit_orders_batch(vec![
            (
                instrument_id,
                None,
                None,
                Some(Quantity::from("10")),
                Some(Price::from("45000")),
                None,
            ),
            (
                instrument_id,
                Some(ClientOrderId::new("edit-order-1")),
                None,
                Some(Quantity::from("20")),
                Some(Price::from("45100")),
                None,
            ),
            (
                instrument_id,
                None,
                Some(VenueOrderId::new("venue-order-2")),
                None,
                Some(Price::from("45200")),
                Some(Price::from("44900")),
            ),
        ])
        .await
        .unwrap();

    assert_eq!(statuses.len(), 3);
    assert!(statuses[0].starts_with("validation_error:"));
    assert_eq!(statuses[1], "edited");
    assert_eq!(statuses[2], "insufficientAvailableFunds");
}
