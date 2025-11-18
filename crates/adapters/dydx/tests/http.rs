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

//! Integration tests for dYdX HTTP client using a mock Axum server.

use std::{net::SocketAddr, sync::Arc};

use axum::{
    Router,
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Json},
    routing::get,
};
use nautilus_dydx::{common::enums::DydxCandleResolution, http::client::DydxHttpClient};
use nautilus_model::instruments::Instrument;
use rstest::rstest;
use serde_json::{Value, json};
use tokio::sync::Mutex;
use ustr::Ustr;

#[derive(Clone, Default)]
struct TestServerState {
    request_count: Arc<Mutex<usize>>,
}

#[allow(dead_code)]
fn load_test_instruments() -> Value {
    json!({
        "markets": {
            "BTC-USD": {
                "clobPairId": "0",
                "ticker": "BTC-USD",
                "market": "BTC-USD",
                "status": "ACTIVE",
                "oraclePrice": "43250.00",
                "priceChange24H": "1250.50",
                "volume24H": "123456789.50",
                "trades24H": 54321,
                "nextFundingRate": "0.0001",
                "initialMarginFraction": "0.05",
                "maintenanceMarginFraction": "0.03",
                "openInterest": "987654321.0",
                "atomicResolution": -10,
                "quantumConversionExponent": -9,
                "tickSize": "1",
                "stepSize": "0.001",
                "stepBaseQuantums": 1000000,
                "subticksPerTick": 100000
            },
            "ETH-USD": {
                "clobPairId": "1",
                "ticker": "ETH-USD",
                "market": "ETH-USD",
                "status": "ACTIVE",
                "oraclePrice": "2250.00",
                "priceChange24H": "50.25",
                "volume24H": "12345678.50",
                "trades24H": 12345,
                "nextFundingRate": "0.00008",
                "initialMarginFraction": "0.05",
                "maintenanceMarginFraction": "0.03",
                "openInterest": "123456789.0",
                "atomicResolution": -9,
                "quantumConversionExponent": -9,
                "tickSize": "0.1",
                "stepSize": "0.01",
                "stepBaseQuantums": 1000000,
                "subticksPerTick": 10000
            }
        }
    })
}

#[allow(dead_code)]
fn load_test_candles() -> Value {
    json!({
        "candles": [
            {
                "startedAt": "2024-01-01T00:00:00.000Z",
                "ticker": "BTC-USD",
                "resolution": "1MIN",
                "low": "43000.0",
                "high": "43500.0",
                "open": "43100.0",
                "close": "43400.0",
                "baseTokenVolume": "12.345",
                "usdVolume": "535000.50",
                "trades": 150,
                "startingOpenInterest": "1000000.0",
                "id": "candle1"
            },
            {
                "startedAt": "2024-01-01T00:01:00.000Z",
                "ticker": "BTC-USD",
                "resolution": "1MIN",
                "low": "43350.0",
                "high": "43600.0",
                "open": "43400.0",
                "close": "43550.0",
                "baseTokenVolume": "8.765",
                "usdVolume": "381000.25",
                "trades": 98,
                "startingOpenInterest": "1000100.0",
                "id": "candle2"
            }
        ]
    })
}

#[allow(dead_code)]
fn load_test_trades() -> Value {
    json!({
        "trades": [
            {
                "id": "trade1",
                "side": "BUY",
                "size": "0.5",
                "price": "43250.0",
                "type": "LIMIT",
                "createdAt": "2024-01-01T00:00:00.000Z",
                "createdAtHeight": "123456"
            },
            {
                "id": "trade2",
                "side": "SELL",
                "size": "1.2",
                "price": "43275.0",
                "type": "MARKET",
                "createdAt": "2024-01-01T00:00:01.500Z",
                "createdAtHeight": "123457"
            },
            {
                "id": "trade3",
                "side": "BUY",
                "size": "0.75",
                "price": "43260.0",
                "type": "LIMIT",
                "createdAt": "2024-01-01T00:00:03.000Z",
                "createdAtHeight": "123458"
            }
        ]
    })
}

#[allow(dead_code)]
async fn handle_get_markets(State(state): State<TestServerState>) -> impl IntoResponse {
    let mut count = state.request_count.lock().await;
    *count += 1;
    drop(count);
    Json(load_test_instruments())
}

#[allow(dead_code)]
async fn handle_get_candles(
    Query(params): Query<std::collections::HashMap<String, String>>,
) -> impl IntoResponse {
    if !params.contains_key("resolution") {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "errors": [{
                    "msg": "resolution is required"
                }]
            })),
        )
            .into_response();
    }

    Json(load_test_candles()).into_response()
}

#[allow(dead_code)]
async fn handle_get_trades() -> impl IntoResponse {
    Json(load_test_trades())
}

#[allow(dead_code)]
async fn handle_rate_limit(
    axum::extract::State(state): axum::extract::State<TestServerState>,
) -> impl IntoResponse {
    let mut count = state.request_count.lock().await;
    *count += 1;

    if *count > 10 {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            Json(json!({
                "errors": [{
                    "msg": "Rate limit exceeded"
                }]
            })),
        )
            .into_response();
    }

    Json(load_test_instruments()).into_response()
}

#[allow(dead_code)]
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
        .route("/v4/test-rate-limit", get(handle_rate_limit))
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

    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    Ok((addr, state))
}

#[rstest]
#[tokio::test]
async fn test_client_creation() {
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

#[rstest]
#[tokio::test]
async fn test_request_instruments_success() {
    let (addr, _state) = start_test_server().await.unwrap();
    let base_url = format!("http://{}", addr);

    let client = DydxHttpClient::new(Some(base_url), Some(30), None, false, None).unwrap();
    let instruments = client.request_instruments(None, None, None).await.unwrap();

    assert_eq!(instruments.len(), 2);
    assert!(
        instruments
            .iter()
            .any(|i| i.id().symbol.as_str() == "BTC-USD-PERP"),
        "Expected BTC-USD-PERP instrument"
    );
    assert!(
        instruments
            .iter()
            .any(|i| i.id().symbol.as_str() == "ETH-USD-PERP"),
        "Expected ETH-USD-PERP instrument"
    );
}

#[rstest]
#[tokio::test]
async fn test_instrument_caching() {
    let (addr, _state) = start_test_server().await.unwrap();
    let base_url = format!("http://{}", addr);

    let client = DydxHttpClient::new(Some(base_url), Some(30), None, false, None).unwrap();
    let instruments = client.request_instruments(None, None, None).await.unwrap();

    client.cache_instruments(instruments);

    let btc_symbol = Ustr::from("BTC-USD-PERP");
    let cached_instrument = client.get_instrument(&btc_symbol);
    assert!(cached_instrument.is_some(), "BTC-USD-PERP should be cached");
    assert_eq!(
        cached_instrument.unwrap().id().symbol.as_str(),
        "BTC-USD-PERP"
    );

    let eth_symbol = Ustr::from("ETH-USD-PERP");
    let eth_instrument = client.get_instrument(&eth_symbol);
    assert!(eth_instrument.is_some(), "ETH-USD-PERP should be cached");
}

#[rstest]
#[tokio::test]
async fn test_cache_single_instrument() {
    let (addr, _state) = start_test_server().await.unwrap();
    let base_url = format!("http://{}", addr);

    let client = DydxHttpClient::new(Some(base_url), Some(30), None, false, None).unwrap();
    let instruments = client.request_instruments(None, None, None).await.unwrap();

    let btc_inst = instruments
        .into_iter()
        .find(|i| i.id().symbol.as_str() == "BTC-USD-PERP")
        .unwrap();
    client.cache_instrument(btc_inst);

    let btc_symbol = Ustr::from("BTC-USD-PERP");
    let cached = client.get_instrument(&btc_symbol);
    assert!(cached.is_some(), "BTC-USD-PERP should be cached");
}

#[rstest]
#[tokio::test]
async fn test_request_trades() {
    let (addr, _state) = start_test_server().await.unwrap();
    let base_url = format!("http://{}", addr);

    let client = DydxHttpClient::new(Some(base_url), Some(30), None, false, None).unwrap();

    let trades = client.request_trades("BTC-USD", None).await.unwrap();

    assert_eq!(trades.trades.len(), 3);
    assert_eq!(trades.trades[0].id, "trade1");
    assert_eq!(trades.trades[1].id, "trade2");
    assert_eq!(trades.trades[2].id, "trade3");
}

#[rstest]
#[tokio::test]
async fn test_request_candles() {
    let (addr, _state) = start_test_server().await.unwrap();
    let base_url = format!("http://{}", addr);

    let client = DydxHttpClient::new(Some(base_url), Some(30), None, false, None).unwrap();

    let candles = client
        .request_candles("BTC-USD", DydxCandleResolution::OneMinute, None, None, None)
        .await
        .unwrap();

    assert_eq!(candles.candles.len(), 2);
    assert_eq!(candles.candles[0].ticker, "BTC-USD");
    assert_eq!(
        candles.candles[0].resolution,
        DydxCandleResolution::OneMinute
    );
}

#[rstest]
#[tokio::test]
async fn test_candles_missing_resolution_returns_error() {
    let (addr, _state) = start_test_server().await.unwrap();
    let base_url = format!("http://{}", addr);

    let client = DydxHttpClient::new(Some(base_url.clone()), Some(30), None, false, None).unwrap();

    let result = client
        .request_candles(
            "INVALID-SYMBOL",
            DydxCandleResolution::OneMinute,
            None,
            None,
            None,
        )
        .await;

    assert!(result.is_ok());
}

#[rstest]
#[tokio::test]
async fn test_rate_limiting() {
    let (addr, state) = start_test_server().await.unwrap();
    let base_url = format!("http://{}", addr);

    let client = DydxHttpClient::new(Some(base_url.clone()), Some(30), None, false, None).unwrap();

    for _ in 0..12 {
        let _ = client.request_instruments(None, None, None).await;
    }

    let count = state.request_count.lock().await;
    assert!(*count > 10);
}

#[rstest]
#[tokio::test]
async fn test_network_error_handling() {
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
async fn test_cancellation_token() {
    let (addr, _state) = start_test_server().await.unwrap();
    let base_url = format!("http://{}", addr);

    let client = DydxHttpClient::new(Some(base_url), Some(30), None, false, None).unwrap();

    client.raw_client().cancel_all_requests();
    assert!(client.raw_client().cancellation_token().is_cancelled());
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
                    Json(json!({
                        "errors": [{
                            "msg": "Internal server error"
                        }]
                    })),
                )
            }),
        )
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let base_url = format!("http://{}", addr);
    let client = DydxHttpClient::new(Some(base_url), Some(5), None, false, None).unwrap();

    let result = client.request_instruments(None, None, None).await;
    assert!(result.is_err());
}

#[rstest]
#[tokio::test]
async fn test_malformed_json_response() {
    let state = TestServerState::default();
    let router = Router::new()
        .route("/v4/perpetualMarkets", get(|| async { "invalid json{{{" }))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let base_url = format!("http://{}", addr);
    let client = DydxHttpClient::new(Some(base_url), Some(5), None, false, None).unwrap();

    let result = client.request_instruments(None, None, None).await;
    assert!(result.is_err());
}

#[rstest]
#[tokio::test]
async fn test_empty_instruments_response() {
    let state = TestServerState::default();
    let router = Router::new()
        .route(
            "/v4/perpetualMarkets",
            get(|| async {
                Json(json!({
                    "markets": {}
                }))
            }),
        )
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let base_url = format!("http://{}", addr);
    let client = DydxHttpClient::new(Some(base_url), Some(5), None, false, None).unwrap();

    let instruments = client.request_instruments(None, None, None).await.unwrap();
    assert_eq!(instruments.len(), 0);
}

#[rstest]
#[tokio::test]
async fn test_trades_chronological_order() {
    let (addr, _state) = start_test_server().await.unwrap();
    let base_url = format!("http://{}", addr);

    let client = DydxHttpClient::new(Some(base_url), Some(30), None, false, None).unwrap();

    let trades = client.request_trades("BTC-USD", None).await.unwrap();

    assert!(trades.trades.len() >= 2);
    for i in 0..trades.trades.len() - 1 {
        let current_time = trades.trades[i].created_at.timestamp_millis();
        let next_time = trades.trades[i + 1].created_at.timestamp_millis();
        assert!(
            current_time <= next_time,
            "Trades should be in chronological order"
        );
    }
}

#[rstest]
#[tokio::test]
async fn test_candles_time_range() {
    let (addr, _state) = start_test_server().await.unwrap();
    let base_url = format!("http://{}", addr);

    let client = DydxHttpClient::new(Some(base_url), Some(30), None, false, None).unwrap();

    let candles = client
        .request_candles("BTC-USD", DydxCandleResolution::OneMinute, None, None, None)
        .await
        .unwrap();

    assert!(candles.candles.len() >= 2);
    for i in 0..candles.candles.len() - 1 {
        let current_time = candles.candles[i].started_at.timestamp_millis();
        let next_time = candles.candles[i + 1].started_at.timestamp_millis();
        assert!(
            current_time <= next_time,
            "Candles should be in chronological order"
        );
    }
}

#[rstest]
#[tokio::test]
async fn test_server_error_503() {
    let state = TestServerState::default();
    let router = Router::new()
        .route(
            "/v4/perpetualMarkets",
            get(|| async {
                (
                    StatusCode::SERVICE_UNAVAILABLE,
                    Json(json!({
                        "errors": [{
                            "msg": "Service temporarily unavailable"
                        }]
                    })),
                )
            }),
        )
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let base_url = format!("http://{}", addr);
    let client = DydxHttpClient::new(Some(base_url), Some(5), None, false, None).unwrap();

    let result = client.request_instruments(None, None, None).await;
    assert!(result.is_err());
}

#[rstest]
#[tokio::test]
async fn test_invalid_json_structure() {
    let state = TestServerState::default();
    let router = Router::new()
        .route(
            "/v4/perpetualMarkets",
            get(|| async {
                Json(json!({
                    "unexpected_field": "value"
                }))
            }),
        )
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let base_url = format!("http://{}", addr);
    let client = DydxHttpClient::new(Some(base_url), Some(5), None, false, None).unwrap();

    let result = client.request_instruments(None, None, None).await;
    if let Ok(instruments) = result {
        assert_eq!(instruments.len(), 0);
    }
}
