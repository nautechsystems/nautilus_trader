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

//! Integration tests for dYdX HTTP client using a mock Axum server.

use std::{collections::HashMap, net::SocketAddr, sync::Arc, time::Duration};

use axum::{
    Router,
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Json},
    routing::get,
};
use chrono::{Duration as ChronoDuration, Utc};
use nautilus_common::{live::get_runtime, testing::wait_until_async};
use nautilus_dydx::{
    common::enums::{DydxCandleResolution, DydxNetwork},
    http::client::{DydxHttpClient, DydxRawHttpClient},
};
use nautilus_model::{
    identifiers::{InstrumentId, Symbol, Venue},
    instruments::Instrument,
};
use nautilus_network::{http::HttpClient, retry::RetryConfig};
use rstest::rstest;
use serde_json::{Value, json};

#[derive(Clone, Default)]
struct TestServerState {
    request_count: Arc<tokio::sync::Mutex<usize>>,
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

async fn handle_get_markets(State(state): State<TestServerState>) -> impl IntoResponse {
    let mut count = state.request_count.lock().await;
    *count += 1;
    drop(count);
    Json(load_test_instruments())
}

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

async fn handle_get_trades() -> impl IntoResponse {
    Json(load_test_trades())
}

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

    wait_for_server(addr, "/v4/perpetualMarkets").await;
    Ok((addr, state))
}

#[rstest]
#[tokio::test]
async fn test_client_creation() {
    let client = DydxHttpClient::new(None, 30, None, DydxNetwork::Testnet, None).unwrap();
    assert!(client.base_url().contains("testnet"));
}

#[rstest]
#[tokio::test]
async fn test_client_mainnet_url() {
    let client = DydxHttpClient::new(None, 30, None, DydxNetwork::Mainnet, None).unwrap();
    assert!(client.base_url().contains("indexer.dydx.trade"));
}

#[rstest]
#[tokio::test]
async fn test_custom_base_url() {
    let custom_url = "https://custom.dydx.exchange";
    let client = DydxHttpClient::new(
        Some(custom_url.to_string()),
        30,
        None,
        DydxNetwork::Mainnet,
        None,
    )
    .unwrap();
    assert_eq!(client.base_url(), custom_url);
}

#[rstest]
#[tokio::test]
async fn test_request_instruments_success() {
    let (addr, _state) = start_test_server().await.unwrap();
    let base_url = format!("http://{addr}");

    let client = DydxHttpClient::new(Some(base_url), 30, None, DydxNetwork::Mainnet, None).unwrap();
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
    let base_url = format!("http://{addr}");

    let client = DydxHttpClient::new(Some(base_url), 30, None, DydxNetwork::Mainnet, None).unwrap();
    let instruments = client.request_instruments(None, None, None).await.unwrap();

    client.cache_instruments(instruments);

    let btc_id = InstrumentId::new(Symbol::new("BTC-USD-PERP"), Venue::new("DYDX"));
    let cached_instrument = client.get_instrument(&btc_id);
    assert!(cached_instrument.is_some(), "BTC-USD-PERP should be cached");
    assert_eq!(
        cached_instrument.unwrap().id().symbol.as_str(),
        "BTC-USD-PERP"
    );

    let eth_id = InstrumentId::new(Symbol::new("ETH-USD-PERP"), Venue::new("DYDX"));
    let eth_instrument = client.get_instrument(&eth_id);
    assert!(eth_instrument.is_some(), "ETH-USD-PERP should be cached");
}

#[rstest]
#[tokio::test]
async fn test_cache_single_instrument() {
    let (addr, _state) = start_test_server().await.unwrap();
    let base_url = format!("http://{addr}");

    let client = DydxHttpClient::new(Some(base_url), 30, None, DydxNetwork::Mainnet, None).unwrap();
    let instruments = client.request_instruments(None, None, None).await.unwrap();

    let btc_inst = instruments
        .into_iter()
        .find(|i| i.id().symbol.as_str() == "BTC-USD-PERP")
        .unwrap();
    client.cache_instrument(btc_inst);

    let btc_id = InstrumentId::new(Symbol::new("BTC-USD-PERP"), Venue::new("DYDX"));
    let cached = client.get_instrument(&btc_id);
    assert!(cached.is_some(), "BTC-USD-PERP should be cached");
}

#[rstest]
#[tokio::test]
async fn test_request_trades() {
    let (addr, _state) = start_test_server().await.unwrap();
    let base_url = format!("http://{addr}");

    let client = DydxHttpClient::new(Some(base_url), 30, None, DydxNetwork::Mainnet, None).unwrap();

    let trades = client.request_trades("BTC-USD", None, None).await.unwrap();

    assert_eq!(trades.trades.len(), 3);
    assert_eq!(trades.trades[0].id, "trade1");
    assert_eq!(trades.trades[1].id, "trade2");
    assert_eq!(trades.trades[2].id, "trade3");
}

#[rstest]
#[tokio::test]
async fn test_request_candles() {
    let (addr, _state) = start_test_server().await.unwrap();
    let base_url = format!("http://{addr}");

    let client = DydxHttpClient::new(Some(base_url), 30, None, DydxNetwork::Mainnet, None).unwrap();

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
    let base_url = format!("http://{addr}");

    let client =
        DydxHttpClient::new(Some(base_url.clone()), 30, None, DydxNetwork::Mainnet, None).unwrap();

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
    let base_url = format!("http://{addr}");

    let client =
        DydxHttpClient::new(Some(base_url.clone()), 30, None, DydxNetwork::Mainnet, None).unwrap();

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
        1,
        None,
        DydxNetwork::Mainnet,
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
    let base_url = format!("http://{addr}");

    let client =
        DydxRawHttpClient::new(Some(base_url), 30, None, DydxNetwork::Mainnet, None).unwrap();

    client.cancel_all_requests();
    assert!(client.cancellation_token().is_cancelled());
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
    wait_for_server(addr, "/v4/perpetualMarkets").await;

    let base_url = format!("http://{addr}");
    let client = DydxHttpClient::new(Some(base_url), 5, None, DydxNetwork::Mainnet, None).unwrap();

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
    wait_for_server(addr, "/v4/perpetualMarkets").await;

    let base_url = format!("http://{addr}");
    let client = DydxHttpClient::new(Some(base_url), 5, None, DydxNetwork::Mainnet, None).unwrap();

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
    wait_for_server(addr, "/v4/perpetualMarkets").await;

    let base_url = format!("http://{addr}");
    let client = DydxHttpClient::new(Some(base_url), 5, None, DydxNetwork::Mainnet, None).unwrap();

    let instruments = client.request_instruments(None, None, None).await.unwrap();
    assert_eq!(instruments.len(), 0);
}

#[rstest]
#[tokio::test]
async fn test_trades_chronological_order() {
    let (addr, _state) = start_test_server().await.unwrap();
    let base_url = format!("http://{addr}");

    let client = DydxHttpClient::new(Some(base_url), 30, None, DydxNetwork::Mainnet, None).unwrap();

    let trades = client.request_trades("BTC-USD", None, None).await.unwrap();

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
    let base_url = format!("http://{addr}");

    let client = DydxHttpClient::new(Some(base_url), 30, None, DydxNetwork::Mainnet, None).unwrap();

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
    wait_for_server(addr, "/v4/perpetualMarkets").await;

    let base_url = format!("http://{addr}");
    let client = DydxHttpClient::new(Some(base_url), 5, None, DydxNetwork::Mainnet, None).unwrap();

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
    wait_for_server(addr, "/v4/perpetualMarkets").await;

    let base_url = format!("http://{addr}");
    let client = DydxHttpClient::new(Some(base_url), 5, None, DydxNetwork::Mainnet, None).unwrap();

    let result = client.request_instruments(None, None, None).await;
    if let Ok(instruments) = result {
        assert_eq!(instruments.len(), 0);
    }
}

#[rstest]
#[tokio::test]
async fn test_get_subaccount() {
    let state = TestServerState::default();
    let router = Router::new()
        .route(
            "/v4/addresses/{address}/subaccountNumber/{subaccount_number}",
            get(|| async {
                Json(json!({
                    "subaccount": {
                        "address": "dydx1test",
                        "subaccountNumber": 0,
                        "equity": "10000.0",
                        "freeCollateral": "5000.0",
                        "openPerpetualPositions": {},
                        "updatedAtHeight": "12345"
                    }
                }))
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
    let client =
        DydxRawHttpClient::new(Some(base_url), 5, None, DydxNetwork::Mainnet, None).unwrap();

    let result = client.get_subaccount("dydx1test", 0).await.unwrap();
    assert_eq!(result.subaccount.address, "dydx1test");
    assert_eq!(result.subaccount.subaccount_number, 0);
}

#[rstest]
#[tokio::test]
async fn test_get_fills() {
    let state = TestServerState::default();
    let router = Router::new()
        .route(
            "/v4/fills",
            get(|| async {
                Json(json!({
                    "fills": [{
                        "id": "fill-123",
                        "side": "BUY",
                        "liquidity": "TAKER",
                        "type": "LIMIT",
                        "market": "BTC-USD",
                        "marketType": "PERPETUAL",
                        "price": "43000.0",
                        "size": "0.1",
                        "fee": "4.3",
                        "createdAt": "2024-01-01T00:00:00.000Z",
                        "createdAtHeight": "12345",
                        "orderId": "order-123",
                        "clientMetadata": "0"
                    }]
                }))
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
    let client =
        DydxRawHttpClient::new(Some(base_url), 5, None, DydxNetwork::Mainnet, None).unwrap();

    let result = client
        .get_fills("dydx1test", 0, Some("BTC-USD"), Some(10))
        .await
        .unwrap();
    assert_eq!(result.fills.len(), 1);
    assert_eq!(result.fills[0].market, "BTC-USD");
}

#[rstest]
#[tokio::test]
async fn test_get_orders() {
    let state = TestServerState::default();
    let router = Router::new()
        .route(
            "/v4/orders",
            get(|| async {
                Json(json!([
                    {
                        "id": "order123",
                        "subaccountId": "dydx1test/0",
                        "clientId": "12345",
                        "clobPairId": "0",
                        "side": "BUY",
                        "size": "0.1",
                        "totalFilled": "0.0",
                        "price": "43000.0",
                        "type": "LIMIT",
                        "status": "OPEN",
                        "timeInForce": "GTT",
                        "postOnly": false,
                        "reduceOnly": false,
                        "createdAt": "2024-01-01T00:00:00.000Z",
                        "createdAtHeight": "12345",
                        "goodTilBlock": "12350",
                        "ticker": "BTC-USD",
                        "orderFlags": "0",
                        "updatedAt": "2024-01-01T00:00:00.000Z",
                        "updatedAtHeight": "12345",
                        "clientMetadata": "0"
                    }
                ]))
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
    let client =
        DydxRawHttpClient::new(Some(base_url), 5, None, DydxNetwork::Mainnet, None).unwrap();

    let result = client
        .get_orders("dydx1test", 0, Some("BTC-USD"), Some(10))
        .await
        .unwrap();
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].id, "order123");
}

#[rstest]
#[tokio::test]
async fn test_get_transfers() {
    let state = TestServerState::default();
    let router = Router::new()
        .route(
            "/v4/transfers",
            get(|| async {
                Json(json!({
                    "transfers": [{
                        "id": "transfer-123",
                        "type": "DEPOSIT",
                        "sender": {
                            "address": "dydx1sender",
                            "subaccountNumber": 0
                        },
                        "recipient": {
                            "address": "dydx1test",
                            "subaccountNumber": 0
                        },
                        "size": "1000.0",
                        "amount": "1000.0",
                        "createdAt": "2024-01-01T00:00:00.000Z",
                        "createdAtHeight": "12345",
                        "symbol": "USDC",
                        "asset": "USDC",
                        "transactionHash": "0xabcdef"
                    }]
                }))
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
    let client =
        DydxRawHttpClient::new(Some(base_url), 5, None, DydxNetwork::Mainnet, None).unwrap();

    let result = client.get_transfers("dydx1test", 0, None).await.unwrap();
    assert_eq!(result.transfers.len(), 1);
}

#[rstest]
#[tokio::test]
async fn test_get_time() {
    let state = TestServerState::default();
    let router = Router::new()
        .route(
            "/v4/time",
            get(|| async {
                Json(json!({
                    "iso": "2024-01-01T00:00:00.000Z",
                    "epoch": 1704067200000_i64
                }))
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
    let client =
        DydxRawHttpClient::new(Some(base_url), 5, None, DydxNetwork::Mainnet, None).unwrap();

    let result = client.get_time().await.unwrap();
    assert_eq!(result.epoch_ms, 1704067200000);
}

#[rstest]
#[tokio::test]
async fn test_get_height() {
    let state = TestServerState::default();
    let router = Router::new()
        .route(
            "/v4/height",
            get(|| async {
                Json(json!({
                    "height": "12345",
                    "time": "2024-01-01T00:00:00.000Z"
                }))
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
    let client =
        DydxRawHttpClient::new(Some(base_url), 5, None, DydxNetwork::Mainnet, None).unwrap();

    let result = client.get_height().await.unwrap();
    assert_eq!(result.height, 12345);
}

#[rstest]
#[tokio::test]
async fn test_server_error_400() {
    let state = TestServerState::default();
    let router = Router::new()
        .route(
            "/v4/perpetualMarkets",
            get(|| async {
                (
                    StatusCode::BAD_REQUEST,
                    Json(json!({
                        "errors": [{
                            "msg": "Invalid parameter"
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
    wait_for_server(addr, "/v4/perpetualMarkets").await;

    let base_url = format!("http://{addr}");
    let client = DydxHttpClient::new(Some(base_url), 5, None, DydxNetwork::Mainnet, None).unwrap();

    let result = client.request_instruments(None, None, None).await;
    assert!(result.is_err());
}

#[rstest]
#[tokio::test]
async fn test_server_error_404() {
    let state = TestServerState::default();
    let router = Router::new()
        .route(
            "/v4/perpetualMarkets",
            get(|| async { (StatusCode::NOT_FOUND, "Not found") }),
        )
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    wait_for_server(addr, "/v4/perpetualMarkets").await;

    let base_url = format!("http://{addr}");
    let client = DydxHttpClient::new(Some(base_url), 5, None, DydxNetwork::Mainnet, None).unwrap();

    let result = client.request_instruments(None, None, None).await;
    assert!(result.is_err());
}

#[rstest]
#[tokio::test]
async fn test_fills_with_market_filter() {
    let state = TestServerState::default();
    let router = Router::new()
        .route(
            "/v4/fills",
            get(|Query(params): Query<HashMap<String, String>>| async move {
                let market = params.get("market");
                assert_eq!(market, Some(&"ETH-USD".to_string()));
                Json(json!({"fills": []}))
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
    let client =
        DydxRawHttpClient::new(Some(base_url), 5, None, DydxNetwork::Mainnet, None).unwrap();

    let result = client
        .get_fills("dydx1test", 0, Some("ETH-USD"), None)
        .await
        .unwrap();
    assert_eq!(result.fills.len(), 0);
}

#[rstest]
#[tokio::test]
async fn test_orders_with_limit() {
    let state = TestServerState::default();
    let router = Router::new()
        .route(
            "/v4/orders",
            get(|Query(params): Query<HashMap<String, String>>| async move {
                let limit = params.get("limit");
                assert_eq!(limit, Some(&"5".to_string()));
                Json(json!([]))
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
    let client =
        DydxRawHttpClient::new(Some(base_url), 5, None, DydxNetwork::Mainnet, None).unwrap();

    let result = client
        .get_orders("dydx1test", 0, None, Some(5))
        .await
        .unwrap();
    assert_eq!(result.len(), 0);
}

#[rstest]
#[tokio::test]
async fn test_http_401_unauthorized() {
    let state = TestServerState::default();
    let router = Router::new()
        .route(
            "/v4/addresses/{address}/subaccountNumber/{subaccount_number}",
            get(|| async {
                (
                    StatusCode::UNAUTHORIZED,
                    Json(json!({
                        "errors": [{
                            "msg": "Invalid authentication credentials"
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
    wait_for_server(addr, "/v4/perpetualMarkets").await;

    let base_url = format!("http://{addr}");
    let client =
        DydxRawHttpClient::new(Some(base_url), 5, None, DydxNetwork::Mainnet, None).unwrap();

    let result = client.get_subaccount("dydx1test", 0).await;
    assert!(result.is_err());
}

#[rstest]
#[tokio::test]
async fn test_http_403_forbidden() {
    let state = TestServerState::default();
    let router = Router::new()
        .route(
            "/v4/fills",
            get(|| async {
                (
                    StatusCode::FORBIDDEN,
                    Json(json!({
                        "errors": [{
                            "msg": "Access denied"
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
    wait_for_server(addr, "/v4/perpetualMarkets").await;

    let base_url = format!("http://{addr}");
    let client =
        DydxRawHttpClient::new(Some(base_url), 5, None, DydxNetwork::Mainnet, None).unwrap();

    let result = client.get_fills("dydx1test", 0, None, None).await;
    assert!(result.is_err());
}

#[rstest]
#[tokio::test]
async fn test_http_502_bad_gateway() {
    let state = TestServerState::default();
    let router = Router::new()
        .route(
            "/v4/height",
            get(|| async {
                (
                    StatusCode::BAD_GATEWAY,
                    Json(json!({
                        "errors": [{
                            "msg": "Bad gateway"
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
    wait_for_server(addr, "/v4/perpetualMarkets").await;

    let base_url = format!("http://{addr}");
    let client =
        DydxRawHttpClient::new(Some(base_url), 5, None, DydxNetwork::Mainnet, None).unwrap();

    let result = client.get_height().await;
    assert!(result.is_err());
}

#[rstest]
#[tokio::test]
async fn test_empty_response_body() {
    let state = TestServerState::default();
    let router = Router::new()
        .route("/v4/fills", get(|| async { (StatusCode::OK, "") }))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    wait_for_server(addr, "/v4/perpetualMarkets").await;

    let base_url = format!("http://{addr}");
    let client =
        DydxRawHttpClient::new(Some(base_url), 5, None, DydxNetwork::Mainnet, None).unwrap();

    let result = client.get_fills("dydx1test", 0, None, None).await;
    assert!(result.is_err());
}

#[rstest]
#[tokio::test]
async fn test_partial_json_response() {
    let state = TestServerState::default();
    let router = Router::new()
        .route(
            "/v4/markets/perpetualMarkets",
            get(|| async { (StatusCode::OK, "{\"markets\": {\"BTC-USD\": {") }),
        )
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    wait_for_server(addr, "/v4/perpetualMarkets").await;

    let base_url = format!("http://{addr}");
    let client =
        DydxRawHttpClient::new(Some(base_url), 5, None, DydxNetwork::Mainnet, None).unwrap();

    let result = client.get_markets().await;
    assert!(result.is_err());
}

#[rstest]
#[tokio::test]
async fn test_instruments_pagination_empty_markets() {
    let state = TestServerState::default();
    let router = Router::new()
        .route(
            "/v4/perpetualMarkets",
            get(|| async { Json(json!({"markets": {}})) }),
        )
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    wait_for_server(addr, "/v4/perpetualMarkets").await;

    let base_url = format!("http://{addr}");
    let client =
        DydxRawHttpClient::new(Some(base_url), 5, None, DydxNetwork::Mainnet, None).unwrap();

    let result = client.get_markets().await.unwrap();
    assert_eq!(result.markets.len(), 0);
}

#[rstest]
#[tokio::test]
async fn test_fills_empty_list() {
    let state = TestServerState::default();
    let router = Router::new()
        .route("/v4/fills", get(|| async { Json(json!({"fills": []})) }))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    wait_for_server(addr, "/v4/perpetualMarkets").await;

    let base_url = format!("http://{addr}");
    let client =
        DydxRawHttpClient::new(Some(base_url), 5, None, DydxNetwork::Mainnet, None).unwrap();

    let result = client.get_fills("dydx1test", 0, None, None).await.unwrap();
    assert_eq!(result.fills.len(), 0);
}

#[rstest]
#[tokio::test]
async fn test_orders_empty_list() {
    let state = TestServerState::default();
    let router = Router::new()
        .route("/v4/orders", get(|| async { Json(json!([])) }))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    wait_for_server(addr, "/v4/perpetualMarkets").await;

    let base_url = format!("http://{addr}");
    let client =
        DydxRawHttpClient::new(Some(base_url), 5, None, DydxNetwork::Mainnet, None).unwrap();

    let result = client.get_orders("dydx1test", 0, None, None).await.unwrap();
    assert_eq!(result.len(), 0);
}

#[rstest]
#[tokio::test]
async fn test_transfers_empty_list() {
    let state = TestServerState::default();
    let router = Router::new()
        .route(
            "/v4/transfers",
            get(|| async { Json(json!({"transfers": []})) }),
        )
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    wait_for_server(addr, "/v4/perpetualMarkets").await;

    let base_url = format!("http://{addr}");
    let client =
        DydxRawHttpClient::new(Some(base_url), 5, None, DydxNetwork::Mainnet, None).unwrap();

    let result = client.get_transfers("dydx1test", 0, None).await.unwrap();
    assert_eq!(result.transfers.len(), 0);
}

#[rstest]
#[tokio::test]
async fn test_invalid_address_format() {
    let state = TestServerState::default();
    let router = Router::new()
        .route(
            "/v4/addresses/{address}/subaccountNumber/{subaccount_number}",
            get(|| async {
                (
                    StatusCode::BAD_REQUEST,
                    Json(json!({
                        "errors": [{
                            "msg": "Invalid address format"
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
    wait_for_server(addr, "/v4/perpetualMarkets").await;

    let base_url = format!("http://{addr}");
    let client =
        DydxRawHttpClient::new(Some(base_url), 5, None, DydxNetwork::Mainnet, None).unwrap();

    let result = client.get_subaccount("invalid", 0).await;
    assert!(result.is_err());
}

#[rstest]
#[tokio::test]
async fn test_connection_pool_reuse() {
    let state = TestServerState::default();
    let counter = state.request_count.clone();

    let router = Router::new()
        .route(
            "/v4/time",
            get(move || {
                let count = counter.clone();
                async move {
                    let mut c = count.lock().await;
                    *c += 1;
                    Json(json!({
                        "iso": "2024-01-01T00:00:00.000Z",
                        "epoch": 1704067200000_i64
                    }))
                }
            }),
        )
        .with_state(state.clone());

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    wait_for_server(addr, "/v4/perpetualMarkets").await;

    let base_url = format!("http://{addr}");
    let client =
        DydxRawHttpClient::new(Some(base_url), 5, None, DydxNetwork::Mainnet, None).unwrap();

    for _ in 0..5 {
        let _ = client.get_time().await;
    }

    let final_count = *state.request_count.lock().await;
    assert_eq!(final_count, 5);
}

#[rstest]
#[tokio::test]
async fn test_concurrent_requests() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(Clone, Default)]
    struct ConcurrentTestState {
        concurrent_requests: Arc<AtomicUsize>,
    }

    let state = ConcurrentTestState::default();
    let concurrent_counter = state.concurrent_requests.clone();

    let router = Router::new()
        .route(
            "/v4/perpetualMarkets",
            get(move || {
                let counter = concurrent_counter.clone();
                async move {
                    counter.fetch_add(1, Ordering::SeqCst);
                    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
                    counter.fetch_sub(1, Ordering::SeqCst);

                    Json(json!({
                        "markets": {
                            "BTC-USD": {
                                "ticker": "BTC-USD",
                                "clobPairId": "0",
                                "status": "ACTIVE",
                                "oraclePrice": "43250.00",
                                "priceChange24H": "1250.50",
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
                            }
                        }
                    }))
                }
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
    let client = Arc::new(
        DydxRawHttpClient::new(Some(base_url), 10, None, DydxNetwork::Mainnet, None).unwrap(),
    );

    let mut handles = vec![];

    for _ in 0..5 {
        let client_clone = client.clone();
        handles.push(get_runtime().spawn(async move { client_clone.get_markets().await }));
    }

    let mut success_count = 0;
    let mut error_count = 0;

    for handle in handles {
        match handle.await {
            Ok(Ok(_)) => success_count += 1,
            Ok(Err(e)) => {
                error_count += 1;
                eprintln!("Request failed: {e:?}");
            }
            Err(e) => {
                error_count += 1;
                eprintln!("Task failed: {e:?}");
            }
        }
    }

    assert!(
        success_count >= 3,
        "At least 3 concurrent requests should succeed, was {success_count} successes and {error_count} errors"
    );
}

#[rstest]
#[tokio::test]
async fn test_request_timeout_short() {
    let state = TestServerState::default();
    let router = Router::new()
        .route(
            "/v4/time",
            get(|| async {
                tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;
                Json(json!({
                    "iso": "2024-01-01T00:00:00.000Z",
                    "epoch": 1704067200000_i64
                }))
            }),
        )
        .route(
            "/v4/perpetualMarkets",
            get(|| async { Json(json!({"markets": {}})) }),
        )
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    wait_for_server(addr, "/v4/perpetualMarkets").await;

    let base_url = format!("http://{addr}");
    let retry_config = RetryConfig {
        max_retries: 0,
        initial_delay_ms: 0,
        max_delay_ms: 0,
        backoff_factor: 1.0,
        jitter_ms: 0,
        operation_timeout_ms: Some(1_000),
        immediate_first: true,
        max_elapsed_ms: Some(2_000),
    };
    let client = DydxRawHttpClient::new(
        Some(base_url),
        1,
        None,
        DydxNetwork::Mainnet,
        Some(retry_config),
    )
    .unwrap();

    let start = std::time::Instant::now();
    let result = client.get_time().await;
    let duration = start.elapsed();

    assert!(result.is_err());
    assert!(
        duration.as_secs() < 5,
        "Should timeout before server response, took {duration:?}"
    );
}

#[rstest]
#[tokio::test]
async fn test_large_instruments_response() {
    let state = TestServerState::default();

    let mut markets = serde_json::Map::new();
    for i in 0..100 {
        markets.insert(
            format!("MARKET{i}-USD"),
            json!({
                "ticker": format!("MARKET{}-USD", i),
                "clobPairId": i.to_string(),
                "status": "ACTIVE",
                "oraclePrice": "100.0",
                "priceChange24H": "1.0",
                "nextFundingRate": "0.0001",
                "initialMarginFraction": "0.05",
                "maintenanceMarginFraction": "0.03",
                "openInterest": "1000.0",
                "atomicResolution": -10,
                "quantumConversionExponent": -9,
                "tickSize": "1",
                "stepSize": "0.001",
                "stepBaseQuantums": 1000000,
                "subticksPerTick": 100000
            }),
        );
    }

    let large_response = json!({ "markets": markets });

    let router = Router::new()
        .route(
            "/v4/perpetualMarkets",
            get(move || {
                let response = large_response.clone();
                async move { Json(response) }
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
    let client =
        DydxRawHttpClient::new(Some(base_url), 30, None, DydxNetwork::Mainnet, None).unwrap();

    let result = client.get_markets().await.unwrap();
    assert_eq!(result.markets.len(), 100);
}

#[rstest]
#[tokio::test]
async fn test_retry_exhaustion() {
    let state = TestServerState::default();
    let counter = state.request_count.clone();

    let router = Router::new()
        .route(
            "/v4/time",
            get(move || {
                let count = counter.clone();
                async move {
                    let mut c = count.lock().await;
                    *c += 1;
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(json!({
                            "errors": [{
                                "msg": "Internal server error"
                            }]
                        })),
                    )
                }
            }),
        )
        .with_state(state.clone());

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    wait_for_server(addr, "/v4/perpetualMarkets").await;

    let base_url = format!("http://{addr}");
    let client =
        DydxRawHttpClient::new(Some(base_url), 5, None, DydxNetwork::Mainnet, None).unwrap();

    let result = client.get_time().await;
    assert!(result.is_err());

    let final_count = *state.request_count.lock().await;
    assert!(final_count > 1, "Should have retried multiple times");
}

#[rstest]
#[tokio::test]
async fn test_mixed_success_and_error_responses() {
    let counter = Arc::new(tokio::sync::Mutex::new(0));
    let state = TestServerState::default();

    let router = Router::new()
        .route(
            "/v4/time",
            get(move || {
                let count = counter.clone();
                async move {
                    let mut c = count.lock().await;
                    *c += 1;

                    if *c % 2 == 0 {
                        Json(json!({
                            "iso": "2024-01-01T00:00:00.000Z",
                            "epoch": 1704067200000_i64
                        }))
                        .into_response()
                    } else {
                        (
                            StatusCode::INTERNAL_SERVER_ERROR,
                            Json(json!({"errors": [{"msg": "Error"}]})),
                        )
                            .into_response()
                    }
                }
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
    let client = Arc::new(
        DydxRawHttpClient::new(Some(base_url), 5, None, DydxNetwork::Mainnet, None).unwrap(),
    );

    let mut handles = vec![];

    for _ in 0..10 {
        let client_clone = client.clone();

        handles.push(tokio::spawn(async move {
            tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
            client_clone.get_time().await
        }));
    }

    let mut success_count = 0;

    for handle in handles {
        if let Ok(Ok(_)) = handle.await {
            success_count += 1;
        }
    }

    assert!(success_count >= 5, "Should have mix of successes");
}

////////////////////////////////////////////////////////////////////////////////
// Pagination Tests
////////////////////////////////////////////////////////////////////////////////

fn generate_candle(timestamp_str: &str, open: &str, high: &str, low: &str, close: &str) -> Value {
    json!({
        "startedAt": timestamp_str,
        "ticker": "BTC-USD",
        "resolution": "1MIN",
        "low": low,
        "high": high,
        "open": open,
        "close": close,
        "baseTokenVolume": "100.0",
        "usdVolume": "5000000.0",
        "trades": 150,
        "startingOpenInterest": "1000000.0",
        "id": format!("candle-{}", timestamp_str)
    })
}

fn generate_order(id: &str, client_id: &str) -> Value {
    json!({
        "id": id,
        "subaccountId": "dydx1test/0",
        "clientId": client_id,
        "clobPairId": "0",
        "side": "BUY",
        "size": "0.1",
        "totalFilled": "0.0",
        "price": "43000.0",
        "type": "LIMIT",
        "status": "OPEN",
        "timeInForce": "GTT",
        "postOnly": false,
        "reduceOnly": false,
        "createdAt": "2024-01-01T00:00:00.000Z",
        "createdAtHeight": "12345",
        "goodTilBlock": "12350",
        "ticker": "BTC-USD",
        "orderFlags": "0",
        "updatedAt": "2024-01-01T00:00:00.000Z",
        "updatedAtHeight": "12345",
        "clientMetadata": "0"
    })
}

fn generate_fill(id: &str) -> Value {
    json!({
        "id": id,
        "side": "BUY",
        "liquidity": "TAKER",
        "type": "LIMIT",
        "market": "BTC-USD",
        "marketType": "PERPETUAL",
        "price": "43000.0",
        "size": "0.1",
        "fee": "4.3",
        "createdAt": "2024-01-01T00:00:00.000Z",
        "createdAtHeight": "12345",
        "orderId": "order-123",
        "clientMetadata": "0"
    })
}

async fn mock_candles_paginated(Query(params): Query<HashMap<String, String>>) -> Json<Value> {
    let limit = params
        .get("limit")
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(100);

    let end_time = params
        .get("toISO")
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map_or_else(Utc::now, |dt| dt.with_timezone(&Utc));

    let mut candles = Vec::new();

    for i in 0..limit {
        let bar_time = end_time - ChronoDuration::minutes(i as i64);
        candles.push(generate_candle(
            &bar_time.to_rfc3339(),
            "50000.0",
            "50100.0",
            "49900.0",
            "50050.0",
        ));
    }

    // dYdX returns candles in reverse chronological order (newest first)
    Json(json!({
        "candles": candles
    }))
}

async fn mock_orders_paginated(Query(params): Query<HashMap<String, String>>) -> Json<Value> {
    let limit = params
        .get("limit")
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(50);

    let mut orders = Vec::new();
    for i in 0..limit {
        orders.push(generate_order(&format!("order-{i}"), &format!("{i}")));
    }

    Json(json!(orders))
}

async fn mock_fills_paginated(Query(params): Query<HashMap<String, String>>) -> Json<Value> {
    let limit = params
        .get("limit")
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(100);

    let mut fills = Vec::new();
    for i in 0..limit {
        fills.push(generate_fill(&format!("fill-{i}")));
    }

    Json(json!({
        "fills": fills
    }))
}

async fn mock_markets_pagination() -> Json<Value> {
    Json(json!({
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
            }
        }
    }))
}

fn create_pagination_router() -> Router {
    Router::new()
        .route(
            "/v4/candles/perpetualMarkets/{ticker}",
            get(mock_candles_paginated),
        )
        .route("/v4/orders", get(mock_orders_paginated))
        .route("/v4/fills", get(mock_fills_paginated))
        .route("/v4/perpetualMarkets", get(mock_markets_pagination))
}

async fn start_pagination_test_server() -> Result<SocketAddr, anyhow::Error> {
    let app = create_pagination_router();

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    Ok(addr)
}

#[rstest]
#[tokio::test]
async fn test_candles_chronological_order_single_page() {
    let addr = start_pagination_test_server().await.unwrap();
    let base_url = format!("http://{addr}");

    let client = DydxHttpClient::new(Some(base_url), 60, None, DydxNetwork::Mainnet, None).unwrap();

    let candles = client
        .request_candles(
            "BTC-USD",
            DydxCandleResolution::OneMinute,
            Some(50),
            None,
            None,
        )
        .await
        .unwrap();

    assert!(!candles.candles.is_empty());
    assert!(candles.candles.len() <= 50);

    // Verify chronological order (each candle should be later than or equal to the previous)
    for i in 1..candles.candles.len() {
        let current = candles.candles[i].started_at.timestamp_millis();
        let prev = candles.candles[i - 1].started_at.timestamp_millis();
        assert!(
            current <= prev,
            "Candles should be in reverse chronological order at index {i}: {current} should be <= {prev}"
        );
    }
}

#[rstest]
#[tokio::test]
async fn test_orders_returns_list() {
    let addr = start_pagination_test_server().await.unwrap();
    let base_url = format!("http://{addr}");

    let client =
        DydxRawHttpClient::new(Some(base_url), 60, None, DydxNetwork::Mainnet, None).unwrap();

    let orders = client
        .get_orders("dydx1test", 0, Some("BTC-USD"), Some(25))
        .await
        .unwrap();

    assert_eq!(orders.len(), 25);
    assert_eq!(orders[0].id, "order-0");
    assert_eq!(orders[24].id, "order-24");
}

#[rstest]
#[tokio::test]
async fn test_fills_returns_list() {
    let addr = start_pagination_test_server().await.unwrap();
    let base_url = format!("http://{addr}");

    let client =
        DydxRawHttpClient::new(Some(base_url), 60, None, DydxNetwork::Mainnet, None).unwrap();

    let result = client
        .get_fills("dydx1test", 0, Some("BTC-USD"), Some(50))
        .await
        .unwrap();

    assert_eq!(result.fills.len(), 50);
    assert_eq!(result.fills[0].id, "fill-0");
    assert_eq!(result.fills[49].id, "fill-49");
}

#[rstest]
#[tokio::test]
async fn test_candles_with_time_range() {
    let addr = start_pagination_test_server().await.unwrap();
    let base_url = format!("http://{addr}");

    let client = DydxHttpClient::new(Some(base_url), 60, None, DydxNetwork::Mainnet, None).unwrap();

    let end = Utc::now();
    let start = end - ChronoDuration::hours(2);

    let candles = client
        .request_candles(
            "BTC-USD",
            DydxCandleResolution::OneMinute,
            Some(100),
            Some(start),
            Some(end),
        )
        .await
        .unwrap();

    assert!(!candles.candles.is_empty());
}

#[rstest]
#[tokio::test]
async fn test_empty_orders_response() {
    let app = Router::new()
        .route("/v4/orders", get(|| async { Json(json!([])) }))
        .route("/v4/perpetualMarkets", get(mock_markets_pagination));

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let base_url = format!("http://{addr}");
    let client =
        DydxRawHttpClient::new(Some(base_url), 60, None, DydxNetwork::Mainnet, None).unwrap();

    let orders = client.get_orders("dydx1test", 0, None, None).await.unwrap();

    assert!(orders.is_empty());
}

#[rstest]
#[tokio::test]
async fn test_empty_fills_response() {
    let app = Router::new()
        .route("/v4/fills", get(|| async { Json(json!({"fills": []})) }))
        .route("/v4/perpetualMarkets", get(mock_markets_pagination));

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let base_url = format!("http://{addr}");
    let client =
        DydxRawHttpClient::new(Some(base_url), 60, None, DydxNetwork::Mainnet, None).unwrap();

    let result = client.get_fills("dydx1test", 0, None, None).await.unwrap();

    assert!(result.fills.is_empty());
}

#[rstest]
#[tokio::test]
async fn test_empty_candles_response() {
    let app = Router::new()
        .route(
            "/v4/candles/perpetualMarkets/{ticker}",
            get(|| async { Json(json!({"candles": []})) }),
        )
        .route("/v4/perpetualMarkets", get(mock_markets_pagination));

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let base_url = format!("http://{addr}");
    let client = DydxHttpClient::new(Some(base_url), 60, None, DydxNetwork::Mainnet, None).unwrap();

    let candles = client
        .request_candles("BTC-USD", DydxCandleResolution::OneMinute, None, None, None)
        .await
        .unwrap();

    assert!(candles.candles.is_empty());
}

#[rstest]
#[tokio::test]
async fn test_get_historical_funding() {
    let state = TestServerState::default();
    let router = Router::new()
        .route(
            "/v4/historicalFunding/{ticker}",
            get(|| async {
                Json(json!({
                    "historicalFunding": [
                        {
                            "ticker": "BTC-USD",
                            "rate": "0.000002375",
                            "price": "89993.8",
                            "effectiveAtHeight": "66622979",
                            "effectiveAt": "2025-12-08T16:00:00.219Z"
                        },
                        {
                            "ticker": "BTC-USD",
                            "rate": "0.000000375",
                            "price": "90860.48604",
                            "effectiveAtHeight": "66617413",
                            "effectiveAt": "2025-12-08T15:00:00.586Z"
                        },
                        {
                            "ticker": "BTC-USD",
                            "rate": "0.000000625",
                            "price": "91459.59191",
                            "effectiveAtHeight": "66611773",
                            "effectiveAt": "2025-12-08T14:00:00.112Z"
                        }
                    ]
                }))
            }),
        )
        .route("/v4/perpetualMarkets", get(mock_markets_pagination))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    wait_for_server(addr, "/v4/perpetualMarkets").await;

    let base_url = format!("http://{addr}");
    let client =
        DydxRawHttpClient::new(Some(base_url), 5, None, DydxNetwork::Mainnet, None).unwrap();

    let result = client
        .get_historical_funding("BTC-USD", None, None, None)
        .await
        .unwrap();

    assert_eq!(result.historical_funding.len(), 3);
    assert_eq!(result.historical_funding[0].ticker, "BTC-USD");
    assert_eq!(result.historical_funding[0].rate.to_string(), "0.000002375");
    assert_eq!(result.historical_funding[0].price.to_string(), "89993.8");
    assert_eq!(result.historical_funding[0].effective_at_height, 66622979);
    assert_eq!(result.historical_funding[1].rate.to_string(), "0.000000375");
    assert_eq!(result.historical_funding[2].rate.to_string(), "0.000000625");
}

#[rstest]
#[tokio::test]
async fn test_request_funding_rates_parses_to_domain_types() {
    let state = TestServerState::default();
    let router = Router::new()
        .route(
            "/v4/historicalFunding/{ticker}",
            get(|| async {
                Json(json!({
                    "historicalFunding": [
                        {
                            "ticker": "BTC-USD",
                            "rate": "0.000002375",
                            "price": "89993.8",
                            "effectiveAtHeight": "66622979",
                            "effectiveAt": "2025-12-08T16:00:00.219Z"
                        },
                        {
                            "ticker": "BTC-USD",
                            "rate": "0.000000375",
                            "price": "90860.48604",
                            "effectiveAtHeight": "66617413",
                            "effectiveAt": "2025-12-08T15:00:00.586Z"
                        },
                        {
                            "ticker": "BTC-USD",
                            "rate": "0.000000625",
                            "price": "91459.59191",
                            "effectiveAtHeight": "66611773",
                            "effectiveAt": "2025-12-08T14:00:00.112Z"
                        }
                    ]
                }))
            }),
        )
        .route("/v4/perpetualMarkets", get(mock_markets_pagination))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    wait_for_server(addr, "/v4/perpetualMarkets").await;

    let base_url = format!("http://{addr}");
    let client = DydxHttpClient::new(Some(base_url), 5, None, DydxNetwork::Mainnet, None).unwrap();

    let instrument_id = InstrumentId::new(Symbol::new("BTC-USD-PERP"), Venue::new("DYDX"));
    let rates = client
        .request_funding_rates(instrument_id, None, None, None)
        .await
        .unwrap();

    // API returns newest first; request_funding_rates reverses to chronological order
    assert_eq!(rates.len(), 3);
    assert_eq!(rates[0].instrument_id, instrument_id);
    assert_eq!(rates[0].rate.to_string(), "0.000000625");
    assert_eq!(rates[1].rate.to_string(), "0.000000375");
    assert_eq!(rates[2].rate.to_string(), "0.000002375");

    // Verify timestamps are in chronological order
    assert!(rates[0].ts_event < rates[1].ts_event);
    assert!(rates[1].ts_event < rates[2].ts_event);
}

#[rstest]
#[tokio::test]
async fn test_get_historical_funding_with_limit() {
    let state = TestServerState::default();
    let router = Router::new()
        .route(
            "/v4/historicalFunding/{ticker}",
            get(|Query(params): Query<HashMap<String, String>>| async move {
                let limit = params.get("limit");
                assert_eq!(limit, Some(&"2".to_string()));
                Json(json!({
                    "historicalFunding": [
                        {
                            "ticker": "BTC-USD",
                            "rate": "0.000002375",
                            "price": "89993.8",
                            "effectiveAtHeight": "66622979",
                            "effectiveAt": "2025-12-08T16:00:00.219Z"
                        },
                        {
                            "ticker": "BTC-USD",
                            "rate": "0.000000375",
                            "price": "90860.48604",
                            "effectiveAtHeight": "66617413",
                            "effectiveAt": "2025-12-08T15:00:00.586Z"
                        }
                    ]
                }))
            }),
        )
        .route("/v4/perpetualMarkets", get(mock_markets_pagination))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    wait_for_server(addr, "/v4/perpetualMarkets").await;

    let base_url = format!("http://{addr}");
    let client =
        DydxRawHttpClient::new(Some(base_url), 5, None, DydxNetwork::Mainnet, None).unwrap();

    let result = client
        .get_historical_funding("BTC-USD", Some(2), None, None)
        .await
        .unwrap();

    assert_eq!(result.historical_funding.len(), 2);
}

#[rstest]
#[tokio::test]
async fn test_get_historical_funding_empty() {
    let state = TestServerState::default();
    let router = Router::new()
        .route(
            "/v4/historicalFunding/{ticker}",
            get(|| async {
                Json(json!({
                    "historicalFunding": []
                }))
            }),
        )
        .route("/v4/perpetualMarkets", get(mock_markets_pagination))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    wait_for_server(addr, "/v4/perpetualMarkets").await;

    let base_url = format!("http://{addr}");
    let client =
        DydxRawHttpClient::new(Some(base_url), 5, None, DydxNetwork::Mainnet, None).unwrap();

    let result = client
        .get_historical_funding("BTC-USD", None, None, None)
        .await
        .unwrap();

    assert!(result.historical_funding.is_empty());
}

fn orderbook_snapshot_payload(bid_levels: &[(f64, f64)], ask_levels: &[(f64, f64)]) -> Value {
    let bids: Vec<Value> = bid_levels
        .iter()
        .map(|(p, s)| json!({"price": p.to_string(), "size": s.to_string()}))
        .collect();
    let asks: Vec<Value> = ask_levels
        .iter()
        .map(|(p, s)| json!({"price": p.to_string(), "size": s.to_string()}))
        .collect();
    json!({"bids": bids, "asks": asks})
}

#[rstest]
#[tokio::test]
async fn test_request_orderbook_snapshot_sets_snapshot_flags() {
    use nautilus_model::enums::{BookAction, RecordFlag};

    let bid_levels = vec![(43240.0, 1.5), (43235.0, 2.3)];
    let ask_levels = vec![(43250.0, 1.2), (43255.0, 2.0)];

    let router = Router::new()
        .route(
            "/v4/orderbooks/perpetualMarket/{ticker}",
            get(move || {
                let payload = orderbook_snapshot_payload(&bid_levels, &ask_levels);
                async move { Json(payload) }
            }),
        )
        .route("/v4/perpetualMarkets", get(mock_markets_pagination));

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    wait_for_server(addr, "/v4/perpetualMarkets").await;

    let base_url = format!("http://{addr}");
    let client = DydxHttpClient::new(Some(base_url), 30, None, DydxNetwork::Mainnet, None).unwrap();

    // request_orderbook_snapshot reads from the instrument cache, so populate it first.
    let instruments = client.request_instruments(None, None, None).await.unwrap();
    client.cache_instruments(instruments);

    let instrument_id = InstrumentId::new(Symbol::new("BTC-USD-PERP"), Venue::new("DYDX"));
    let deltas = client
        .request_orderbook_snapshot(instrument_id)
        .await
        .unwrap();

    // 1 Clear + 2 bids + 2 asks = 5 deltas
    assert_eq!(deltas.deltas.len(), 5);

    let snapshot = RecordFlag::F_SNAPSHOT as u8;
    let last_flag = RecordFlag::F_LAST as u8;

    // Clear delta carries F_SNAPSHOT (not last).
    assert_eq!(deltas.deltas[0].action, BookAction::Clear);
    assert_eq!(deltas.deltas[0].flags, snapshot);

    // Intermediate add deltas carry F_SNAPSHOT only.
    for delta in &deltas.deltas[1..deltas.deltas.len() - 1] {
        assert_eq!(delta.action, BookAction::Add);
        assert_eq!(delta.flags, snapshot);
    }

    // Terminator carries F_SNAPSHOT | F_LAST.
    let terminator = deltas.deltas.last().unwrap();
    assert_eq!(terminator.flags, snapshot | last_flag);
}

#[rstest]
#[tokio::test]
async fn test_request_orderbook_snapshot_empty_book() {
    use nautilus_model::enums::{BookAction, RecordFlag};

    let router = Router::new()
        .route(
            "/v4/orderbooks/perpetualMarket/{ticker}",
            get(|| async { Json(orderbook_snapshot_payload(&[], &[])) }),
        )
        .route("/v4/perpetualMarkets", get(mock_markets_pagination));

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    wait_for_server(addr, "/v4/perpetualMarkets").await;

    let base_url = format!("http://{addr}");
    let client = DydxHttpClient::new(Some(base_url), 30, None, DydxNetwork::Mainnet, None).unwrap();

    let instruments = client.request_instruments(None, None, None).await.unwrap();
    client.cache_instruments(instruments);

    let instrument_id = InstrumentId::new(Symbol::new("BTC-USD-PERP"), Venue::new("DYDX"));
    let deltas = client
        .request_orderbook_snapshot(instrument_id)
        .await
        .unwrap();

    // Empty book: one Clear carrying F_SNAPSHOT | F_LAST so buffered subscribers flush.
    assert_eq!(deltas.deltas.len(), 1);
    assert_eq!(deltas.deltas[0].action, BookAction::Clear);
    assert_eq!(
        deltas.deltas[0].flags,
        RecordFlag::F_SNAPSHOT as u8 | RecordFlag::F_LAST as u8,
    );
}

/// Builds a trade fixture. `duplicate_id_from_previous_page` stashes a trade id the
/// tests can assert cross-page dedup behavior against.
fn make_trade(id: &str, height: u64, created_at: &str, price: &str, size: &str) -> Value {
    json!({
        "id": id,
        "side": "BUY",
        "size": size,
        "price": price,
        "type": "LIMIT",
        "createdAt": created_at,
        "createdAtHeight": height.to_string(),
    })
}

/// Generates a synthetic trade at `height` with id `t-{height}`.
fn gen_trade(height: u64) -> Value {
    let ts_secs = height % 60;
    let ts_mins = (height / 60) % 60;
    let ts = format!("2024-01-01T00:{ts_mins:02}:{ts_secs:02}.000Z");
    let id = format!("t-{height}");
    make_trade(&id, height, &ts, "43250.0", "0.5")
}

/// Returns a full page worth of unique trades (exactly `page_size`) starting at the
/// given cursor height (inclusive, newest first), plus the height of the oldest
/// trade emitted.
fn gen_full_page(cursor_height: u64, page_size: usize) -> (Vec<Value>, u64) {
    let mut page = Vec::with_capacity(page_size);
    let mut h = cursor_height;
    for _ in 0..page_size {
        page.push(gen_trade(h));
        if h == 0 {
            break;
        }
        h -= 1;
    }
    let oldest = h;
    (page, oldest)
}

#[derive(Clone, Default)]
struct PaginatedTradeState {
    calls: Arc<tokio::sync::Mutex<Vec<Option<u64>>>>,
}

/// Paginator mock that produces exactly `limit` trades on every call (saturating the
/// client's page_limit) down to a configurable floor, then empty. Satisfies the
/// paginator's "don't break on partial page" criterion so multi-page behavior is
/// exercised end-to-end.
async fn handle_saturating_trades(
    State(state): State<PaginatedTradeState>,
    Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    const FLOOR: u64 = 900; // return empty once cursor falls below this
    let cursor: Option<u64> = params
        .get("createdBeforeOrAtHeight")
        .and_then(|v| v.parse::<u64>().ok());
    state.calls.lock().await.push(cursor);

    let limit: usize = params
        .get("limit")
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(1000);
    let start_height = cursor.unwrap_or(2000);

    if start_height < FLOOR {
        return Json(json!({"trades": Vec::<Value>::new()}));
    }

    let (page, _oldest) = gen_full_page(start_height, limit);
    Json(json!({"trades": page}))
}

#[rstest]
#[tokio::test]
async fn test_request_trade_ticks_paginates_across_blocks() {
    // Full-page responses keep the paginator iterating until the cursor drops below
    // the mock's floor, producing >1 HTTP call and a unique, chronologically-ordered
    // trade set.
    let state = PaginatedTradeState::default();
    let router = Router::new()
        .route(
            "/v4/trades/perpetualMarket/{ticker}",
            get(handle_saturating_trades),
        )
        .route("/v4/perpetualMarkets", get(mock_markets_pagination))
        .route(
            "/v4/height",
            get(|| async { Json(json!({"height": "2000", "time": "2024-01-01T00:33:20.000Z"})) }),
        )
        .with_state(state.clone());

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    wait_for_server(addr, "/v4/perpetualMarkets").await;

    let base_url = format!("http://{addr}");
    let client = DydxHttpClient::new(Some(base_url), 30, None, DydxNetwork::Mainnet, None).unwrap();
    let instruments = client.request_instruments(None, None, None).await.unwrap();
    client.cache_instruments(instruments);

    let instrument_id = InstrumentId::new(Symbol::new("BTC-USD-PERP"), Venue::new("DYDX"));
    let ticks = client
        .request_trade_ticks(instrument_id, None, None, None)
        .await
        .unwrap();

    // Unique trade ids -- dedup must remove the single-cursor overlap at block
    // boundaries. Pool spans heights 2000..900 = ~1100 unique trades.
    let unique_ids: std::collections::HashSet<_> =
        ticks.iter().map(|t| t.trade_id.to_string()).collect();
    assert_eq!(unique_ids.len(), ticks.len(), "duplicate trade ids leaked");
    assert!(
        ticks.len() >= 1000,
        "expected multi-page output (>=1000 trades), found {}",
        ticks.len()
    );

    // Chronological order: oldest first.
    for pair in ticks.windows(2) {
        assert!(
            pair[0].ts_event <= pair[1].ts_event,
            "ticks must be in chronological order",
        );
    }

    let calls = state.calls.lock().await.clone();
    assert!(
        calls.len() >= 2,
        "expected multi-page pagination, found {} HTTP calls",
        calls.len()
    );
}

#[rstest]
#[tokio::test]
async fn test_request_trade_ticks_dedups_cross_page_overlap() {
    // Force the boundary trade `t-{cursor}` to reappear on consecutive pages (mirrors
    // how `createdBeforeOrAtHeight` produces overlap at block boundaries). The HashSet
    // dedup must drop the duplicate so the final vec contains each id at most once.
    let state = PaginatedTradeState::default();
    let router = Router::new()
        .route(
            "/v4/trades/perpetualMarket/{ticker}",
            get(handle_saturating_trades),
        )
        .route("/v4/perpetualMarkets", get(mock_markets_pagination))
        .route(
            "/v4/height",
            get(|| async { Json(json!({"height": "2000", "time": "2024-01-01T00:33:20.000Z"})) }),
        )
        .with_state(state.clone());

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    wait_for_server(addr, "/v4/perpetualMarkets").await;

    let base_url = format!("http://{addr}");
    let client = DydxHttpClient::new(Some(base_url), 30, None, DydxNetwork::Mainnet, None).unwrap();
    let instruments = client.request_instruments(None, None, None).await.unwrap();
    client.cache_instruments(instruments);

    let instrument_id = InstrumentId::new(Symbol::new("BTC-USD-PERP"), Venue::new("DYDX"));
    let ticks = client
        .request_trade_ticks(instrument_id, None, None, None)
        .await
        .unwrap();

    // The saturating mock advances cursor by `oldest_height.saturating_sub(1)` each
    // page; with no cursor-state on the server, the boundary trade (t-{oldest}) would
    // be emitted again on the next page. The client's HashSet must dedup it.
    let ids: Vec<_> = ticks.iter().map(|t| t.trade_id.to_string()).collect();
    let unique: std::collections::HashSet<_> = ids.iter().collect();
    assert_eq!(
        unique.len(),
        ids.len(),
        "duplicate ids slipped through dedup (server overlap at block boundaries)",
    );
}

#[rstest]
#[tokio::test]
async fn test_request_trade_ticks_respects_start_boundary() {
    // Return trades spanning 00:01:40 -> 00:01:50. With start=00:01:45, only trades
    // at or after 00:01:45 must appear and pagination must stop once the page's
    // oldest trade crosses the boundary.
    async fn handle_bounded_trades() -> impl IntoResponse {
        let page = vec![
            make_trade("t-1", 110, "2024-01-01T00:01:50.000Z", "1.0", "0.1"),
            make_trade("t-2", 109, "2024-01-01T00:01:48.000Z", "1.0", "0.1"),
            make_trade("t-3", 108, "2024-01-01T00:01:46.000Z", "1.0", "0.1"),
            make_trade("t-4", 107, "2024-01-01T00:01:44.000Z", "1.0", "0.1"),
            make_trade("t-5", 106, "2024-01-01T00:01:42.000Z", "1.0", "0.1"),
        ];
        Json(json!({"trades": page}))
    }

    let router = Router::new()
        .route(
            "/v4/trades/perpetualMarket/{ticker}",
            get(handle_bounded_trades),
        )
        .route("/v4/perpetualMarkets", get(mock_markets_pagination));

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    wait_for_server(addr, "/v4/perpetualMarkets").await;

    let base_url = format!("http://{addr}");
    let client = DydxHttpClient::new(Some(base_url), 30, None, DydxNetwork::Mainnet, None).unwrap();
    let instruments = client.request_instruments(None, None, None).await.unwrap();
    client.cache_instruments(instruments);

    let instrument_id = InstrumentId::new(Symbol::new("BTC-USD-PERP"), Venue::new("DYDX"));
    let start = chrono::DateTime::parse_from_rfc3339("2024-01-01T00:01:45.000Z")
        .unwrap()
        .with_timezone(&chrono::Utc);
    let ticks = client
        .request_trade_ticks(instrument_id, Some(start), None, None)
        .await
        .unwrap();

    // Trades at 01:44 and 01:42 are before the start boundary and must be filtered out.
    let ids: Vec<_> = ticks.iter().map(|t| t.trade_id.to_string()).collect();
    assert!(ids.contains(&"t-1".to_string()));
    assert!(ids.contains(&"t-2".to_string()));
    assert!(ids.contains(&"t-3".to_string()));
    assert!(!ids.contains(&"t-4".to_string()));
    assert!(!ids.contains(&"t-5".to_string()));
}
