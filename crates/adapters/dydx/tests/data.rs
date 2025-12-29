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

//! Integration tests for dYdX data client.

use std::{collections::HashMap, net::SocketAddr, path::PathBuf, sync::Arc, time::Duration};

use axum::{
    Router,
    extract::{Path, Query},
    http::StatusCode,
    response::IntoResponse,
    routing::get,
};
use nautilus_common::testing::wait_until_async;
use nautilus_dydx::{common::enums::DydxCandleResolution, http::client::DydxHttpClient};
use nautilus_model::instruments::Instrument;
use nautilus_network::http::HttpClient;
use rstest::rstest;
use serde_json::{Value, json};
use ustr::Ustr;

fn test_data_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test_data")
}

fn load_json_fixture(filename: &str) -> Value {
    let path = test_data_path().join(filename);
    let content = std::fs::read_to_string(&path)
        .unwrap_or_else(|_| panic!("Failed to read test data file: {}", path.display()));
    serde_json::from_str(&content).expect("Invalid JSON in test data file")
}

fn load_json_result_fixture(filename: &str) -> Value {
    let json = load_json_fixture(filename);
    json.get("result").cloned().unwrap_or(json)
}

#[derive(Clone, Default)]
struct TestServerState {
    request_count: Arc<tokio::sync::Mutex<usize>>,
    last_candle_params: Arc<tokio::sync::Mutex<Option<HashMap<String, String>>>>,
    last_trades_params: Arc<tokio::sync::Mutex<Option<HashMap<String, String>>>>,
}

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

fn load_test_instruments() -> Value {
    load_json_result_fixture("http_get_perpetual_markets.json")
}

fn load_test_candles() -> Value {
    load_json_result_fixture("http_get_candles.json")
}

fn load_test_trades() -> Value {
    load_json_result_fixture("http_get_trades.json")
}

async fn handle_get_markets(
    axum::extract::State(state): axum::extract::State<TestServerState>,
) -> impl IntoResponse {
    let mut count = state.request_count.lock().await;
    *count += 1;
    drop(count);
    axum::response::Json(load_test_instruments())
}

async fn handle_get_candles(
    axum::extract::State(state): axum::extract::State<TestServerState>,
    Path(_ticker): Path<String>,
    Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    *state.last_candle_params.lock().await = Some(params.clone());

    if !params.contains_key("resolution") {
        return (
            StatusCode::BAD_REQUEST,
            axum::response::Json(json!({
                "errors": [{"msg": "resolution is required"}]
            })),
        )
            .into_response();
    }

    axum::response::Json(load_test_candles()).into_response()
}

async fn handle_get_trades(
    axum::extract::State(state): axum::extract::State<TestServerState>,
    Path(_ticker): Path<String>,
    Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    *state.last_trades_params.lock().await = Some(params);
    axum::response::Json(load_test_trades())
}

fn create_test_router(state: TestServerState) -> Router {
    Router::new()
        .route("/v4/perpetualMarkets", get(handle_get_markets))
        .route(
            "/v4/candles/perpetualMarkets/{ticker}",
            get(handle_get_candles),
        )
        .route(
            "/v4/trades/perpetualMarket/{ticker}",
            get(handle_get_trades),
        )
        .with_state(state)
}

async fn start_test_server()
-> Result<(SocketAddr, TestServerState), Box<dyn std::error::Error + Send + Sync>> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let state = TestServerState::default();
    let router = create_test_router(state.clone());

    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });

    wait_for_server(addr, "/v4/perpetualMarkets").await;
    Ok((addr, state))
}

#[rstest]
#[tokio::test]
async fn test_request_instruments_returns_all_active_markets() {
    let (addr, _state) = start_test_server().await.unwrap();
    let base_url = format!("http://{addr}");

    let client = DydxHttpClient::new(Some(base_url), Some(30), None, false, None).unwrap();
    let instruments = client.request_instruments(None, None, None).await.unwrap();

    // All active markets from fixture (BTC-USD, ETH-USD, SOL-USD)
    assert_eq!(instruments.len(), 3);
    assert!(
        instruments
            .iter()
            .any(|i| i.id().symbol.as_str() == "BTC-USD-PERP"),
        "BTC-USD-PERP should be present"
    );
    assert!(
        instruments
            .iter()
            .any(|i| i.id().symbol.as_str() == "ETH-USD-PERP"),
        "ETH-USD-PERP should be present"
    );
}

#[rstest]
#[tokio::test]
async fn test_instrument_properties_btc() {
    let (addr, _state) = start_test_server().await.unwrap();
    let base_url = format!("http://{addr}");

    let client = DydxHttpClient::new(Some(base_url), Some(30), None, false, None).unwrap();
    let instruments = client.request_instruments(None, None, None).await.unwrap();

    let btc = instruments
        .iter()
        .find(|i| i.id().symbol.as_str() == "BTC-USD-PERP")
        .expect("BTC-USD-PERP not found");

    // Check instrument properties from test data
    assert_eq!(btc.price_precision(), 0); // tick_size = 1
    assert_eq!(btc.size_precision(), 4); // step_size = 0.0001
    assert_eq!(btc.id().venue.as_str(), "DYDX");
}

#[rstest]
#[tokio::test]
async fn test_instrument_properties_eth() {
    let (addr, _state) = start_test_server().await.unwrap();
    let base_url = format!("http://{addr}");

    let client = DydxHttpClient::new(Some(base_url), Some(30), None, false, None).unwrap();
    let instruments = client.request_instruments(None, None, None).await.unwrap();

    let eth = instruments
        .iter()
        .find(|i| i.id().symbol.as_str() == "ETH-USD-PERP")
        .expect("ETH-USD-PERP not found");

    assert_eq!(eth.price_precision(), 1); // tick_size = 0.1
    assert_eq!(eth.size_precision(), 3); // step_size = 0.001
}

#[rstest]
#[tokio::test]
async fn test_instrument_caching() {
    let (addr, _state) = start_test_server().await.unwrap();
    let base_url = format!("http://{addr}");

    let client = DydxHttpClient::new(Some(base_url), Some(30), None, false, None).unwrap();
    let instruments = client.request_instruments(None, None, None).await.unwrap();

    // Cache instruments
    client.cache_instruments(instruments);

    // Retrieve from cache
    let btc_symbol = Ustr::from("BTC-USD-PERP");
    let cached = client.get_instrument(&btc_symbol);
    assert!(cached.is_some(), "BTC-USD-PERP should be cached");
    assert_eq!(cached.unwrap().id().symbol.as_str(), "BTC-USD-PERP");
}

#[rstest]
#[tokio::test]
async fn test_cache_single_instrument() {
    let (addr, _state) = start_test_server().await.unwrap();
    let base_url = format!("http://{addr}");

    let client = DydxHttpClient::new(Some(base_url), Some(30), None, false, None).unwrap();
    let instruments = client.request_instruments(None, None, None).await.unwrap();

    let btc = instruments
        .into_iter()
        .find(|i| i.id().symbol.as_str() == "BTC-USD-PERP")
        .unwrap();

    client.cache_instrument(btc);

    let btc_symbol = Ustr::from("BTC-USD-PERP");
    assert!(client.get_instrument(&btc_symbol).is_some());

    // ETH should not be cached
    let eth_symbol = Ustr::from("ETH-USD-PERP");
    assert!(client.get_instrument(&eth_symbol).is_none());
}

#[rstest]
#[tokio::test]
async fn test_request_trades_success() {
    let (addr, _state) = start_test_server().await.unwrap();
    let base_url = format!("http://{addr}");

    let client = DydxHttpClient::new(Some(base_url), Some(30), None, false, None).unwrap();
    let trades = client.request_trades("BTC-USD", None).await.unwrap();

    assert_eq!(trades.trades.len(), 3);
    assert_eq!(trades.trades[0].id, "03f89a550000000200000002");
    assert_eq!(trades.trades[1].id, "03f89a53000000020000000e");
    assert_eq!(trades.trades[2].id, "03f89a53000000020000000b");
}

#[rstest]
#[tokio::test]
async fn test_trades_chronological_order() {
    let (addr, _state) = start_test_server().await.unwrap();
    let base_url = format!("http://{addr}");

    let client = DydxHttpClient::new(Some(base_url), Some(30), None, false, None).unwrap();
    let trades = client.request_trades("BTC-USD", None).await.unwrap();

    // dYdX returns trades in reverse chronological order (newest first)
    for i in 0..trades.trades.len() - 1 {
        let current = trades.trades[i].created_at.timestamp_millis();
        let next = trades.trades[i + 1].created_at.timestamp_millis();
        assert!(
            current >= next,
            "Trades should be in reverse chronological order (newest first)"
        );
    }
}

#[rstest]
#[tokio::test]
async fn test_trades_with_limit() {
    let (addr, state) = start_test_server().await.unwrap();
    let base_url = format!("http://{addr}");

    let client = DydxHttpClient::new(Some(base_url), Some(30), None, false, None).unwrap();
    let _trades = client.request_trades("BTC-USD", Some(10)).await.unwrap();

    let params = state.last_trades_params.lock().await;
    assert!(params.is_some());
    assert_eq!(
        params.as_ref().unwrap().get("limit"),
        Some(&"10".to_string())
    );
}

#[rstest]
#[tokio::test]
async fn test_request_candles_success() {
    let (addr, _state) = start_test_server().await.unwrap();
    let base_url = format!("http://{addr}");

    let client = DydxHttpClient::new(Some(base_url), Some(30), None, false, None).unwrap();
    let candles = client
        .request_candles("BTC-USD", DydxCandleResolution::OneMinute, None, None, None)
        .await
        .unwrap();

    assert_eq!(candles.candles.len(), 3);
    assert_eq!(candles.candles[0].ticker, "BTC-USD");
    assert_eq!(
        candles.candles[0].resolution,
        DydxCandleResolution::OneMinute
    );
}

#[rstest]
#[tokio::test]
async fn test_candles_ohlcv_values() {
    let (addr, _state) = start_test_server().await.unwrap();
    let base_url = format!("http://{addr}");

    let client = DydxHttpClient::new(Some(base_url), Some(30), None, false, None).unwrap();
    let candles = client
        .request_candles("BTC-USD", DydxCandleResolution::OneMinute, None, None, None)
        .await
        .unwrap();

    let first = &candles.candles[0];
    assert_eq!(first.open.to_string(), "89934");
    assert_eq!(first.high.to_string(), "89970");
    assert_eq!(first.low.to_string(), "89911");
    assert_eq!(first.close.to_string(), "89941");
}

#[rstest]
#[tokio::test]
async fn test_candles_resolution_param() {
    let (addr, state) = start_test_server().await.unwrap();
    let base_url = format!("http://{addr}");

    let client = DydxHttpClient::new(Some(base_url), Some(30), None, false, None).unwrap();
    let _candles = client
        .request_candles(
            "BTC-USD",
            DydxCandleResolution::FiveMinutes,
            None,
            None,
            None,
        )
        .await
        .unwrap();

    let params = state.last_candle_params.lock().await;
    assert!(params.is_some());
    assert_eq!(
        params.as_ref().unwrap().get("resolution"),
        Some(&"5MINS".to_string())
    );
}

#[rstest]
#[tokio::test]
async fn test_candles_chronological_order() {
    let (addr, _state) = start_test_server().await.unwrap();
    let base_url = format!("http://{addr}");

    let client = DydxHttpClient::new(Some(base_url), Some(30), None, false, None).unwrap();
    let candles = client
        .request_candles("BTC-USD", DydxCandleResolution::OneMinute, None, None, None)
        .await
        .unwrap();

    // dYdX returns candles in reverse chronological order (newest first)
    for i in 0..candles.candles.len() - 1 {
        let current = candles.candles[i].started_at.timestamp_millis();
        let next = candles.candles[i + 1].started_at.timestamp_millis();
        assert!(
            current >= next,
            "Candles should be in reverse chronological order (newest first)"
        );
    }
}

#[rstest]
#[tokio::test]
async fn test_empty_instruments_response() {
    let state = TestServerState::default();
    let router = Router::new()
        .route(
            "/v4/perpetualMarkets",
            get(|| async { axum::response::Json(json!({"markets": {}})) }),
        )
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    wait_for_server(addr, "/v4/perpetualMarkets").await;

    let base_url = format!("http://{addr}");
    let client = DydxHttpClient::new(Some(base_url), Some(5), None, false, None).unwrap();

    let instruments = client.request_instruments(None, None, None).await.unwrap();
    assert!(instruments.is_empty());
}

#[rstest]
#[tokio::test]
async fn test_network_error() {
    let client = DydxHttpClient::new(
        Some("http://localhost:1".to_string()),
        Some(1),
        None,
        false,
        None,
    )
    .unwrap();

    let result = client.request_instruments(None, None, None).await;
    assert!(result.is_err());
}

#[rstest]
#[tokio::test]
async fn test_server_error_500() {
    let state = TestServerState::default();
    let router = Router::new()
        .route(
            "/v4/perpetualMarkets",
            get(|| async {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    axum::response::Json(json!({"errors": [{"msg": "Internal error"}]})),
                )
            }),
        )
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    wait_for_server(addr, "/v4/perpetualMarkets").await;

    let base_url = format!("http://{addr}");
    let client = DydxHttpClient::new(Some(base_url), Some(5), None, false, None).unwrap();

    let result = client.request_instruments(None, None, None).await;
    assert!(result.is_err());
}

#[rstest]
#[tokio::test]
async fn test_server_error_429_rate_limit() {
    let state = TestServerState::default();
    let router = Router::new()
        .route(
            "/v4/perpetualMarkets",
            get(|| async {
                (
                    StatusCode::TOO_MANY_REQUESTS,
                    axum::response::Json(json!({"errors": [{"msg": "Rate limit exceeded"}]})),
                )
            }),
        )
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    wait_for_server(addr, "/v4/perpetualMarkets").await;

    let base_url = format!("http://{addr}");
    let client = DydxHttpClient::new(Some(base_url), Some(5), None, false, None).unwrap();

    let result = client.request_instruments(None, None, None).await;
    assert!(result.is_err());
}

#[rstest]
#[tokio::test]
async fn test_malformed_json_response() {
    let state = TestServerState::default();
    let router = Router::new()
        .route(
            "/v4/perpetualMarkets",
            get(|| async { "not valid json {{{{" }),
        )
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    wait_for_server(addr, "/v4/perpetualMarkets").await;

    let base_url = format!("http://{addr}");
    let client = DydxHttpClient::new(Some(base_url), Some(5), None, false, None).unwrap();

    let result = client.request_instruments(None, None, None).await;
    assert!(result.is_err());
}

#[rstest]
#[tokio::test]
async fn test_client_testnet_url() {
    let client = DydxHttpClient::new(None, Some(30), None, true, None).unwrap();
    assert!(client.base_url().contains("testnet"));
}

#[rstest]
#[tokio::test]
async fn test_client_mainnet_url() {
    let client = DydxHttpClient::new(None, Some(30), None, false, None).unwrap();
    assert!(client.base_url().contains("indexer.dydx.trade"));
}

#[rstest]
#[tokio::test]
async fn test_custom_base_url() {
    let custom_url = "https://custom.dydx.exchange";
    let client =
        DydxHttpClient::new(Some(custom_url.to_string()), Some(30), None, false, None).unwrap();
    assert_eq!(client.base_url(), custom_url);
}
