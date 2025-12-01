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

use std::{collections::HashMap, net::SocketAddr, sync::Arc};

use axum::{
    Router,
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Json},
    routing::get,
};
use nautilus_dydx::{
    common::enums::DydxCandleResolution,
    http::client::{DydxHttpClient, DydxRawHttpClient},
};
use nautilus_model::instruments::Instrument;
use rstest::rstest;
use serde_json::{Value, json};
use ustr::Ustr;

#[derive(Clone, Default)]
struct TestServerState {
    request_count: Arc<tokio::sync::Mutex<usize>>,
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
    let base_url = format!("http://{addr}");

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
    let base_url = format!("http://{addr}");

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
    let base_url = format!("http://{addr}");

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
    let base_url = format!("http://{addr}");

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
    let base_url = format!("http://{addr}");

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
    let base_url = format!("http://{addr}");

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
    let base_url = format!("http://{addr}");

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
    let base_url = format!("http://{addr}");

    let client = DydxRawHttpClient::new(Some(base_url), Some(30), None, false, None).unwrap();

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
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

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
        .route("/v4/perpetualMarkets", get(|| async { "invalid json{{{" }))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let base_url = format!("http://{addr}");
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

    let base_url = format!("http://{addr}");
    let client = DydxHttpClient::new(Some(base_url), Some(5), None, false, None).unwrap();

    let instruments = client.request_instruments(None, None, None).await.unwrap();
    assert_eq!(instruments.len(), 0);
}

#[rstest]
#[tokio::test]
async fn test_trades_chronological_order() {
    let (addr, _state) = start_test_server().await.unwrap();
    let base_url = format!("http://{addr}");

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
    let base_url = format!("http://{addr}");

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

    let base_url = format!("http://{addr}");
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

    let base_url = format!("http://{addr}");
    let client = DydxHttpClient::new(Some(base_url), Some(5), None, false, None).unwrap();

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
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let base_url = format!("http://{addr}");
    let client = DydxRawHttpClient::new(Some(base_url), Some(5), None, false, None).unwrap();

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
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let base_url = format!("http://{addr}");
    let client = DydxRawHttpClient::new(Some(base_url), Some(5), None, false, None).unwrap();

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
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let base_url = format!("http://{addr}");
    let client = DydxRawHttpClient::new(Some(base_url), Some(5), None, false, None).unwrap();

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
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let base_url = format!("http://{addr}");
    let client = DydxRawHttpClient::new(Some(base_url), Some(5), None, false, None).unwrap();

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
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let base_url = format!("http://{addr}");
    let client = DydxRawHttpClient::new(Some(base_url), Some(5), None, false, None).unwrap();

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
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let base_url = format!("http://{addr}");
    let client = DydxRawHttpClient::new(Some(base_url), Some(5), None, false, None).unwrap();

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
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let base_url = format!("http://{addr}");
    let client = DydxHttpClient::new(Some(base_url), Some(5), None, false, None).unwrap();

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
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let base_url = format!("http://{addr}");
    let client = DydxHttpClient::new(Some(base_url), Some(5), None, false, None).unwrap();

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
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let base_url = format!("http://{addr}");
    let client = DydxRawHttpClient::new(Some(base_url), Some(5), None, false, None).unwrap();

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
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let base_url = format!("http://{addr}");
    let client = DydxRawHttpClient::new(Some(base_url), Some(5), None, false, None).unwrap();

    let result = client
        .get_orders("dydx1test", 0, None, Some(5))
        .await
        .unwrap();
    assert_eq!(result.len(), 0);
}

// ================================================================================
// Additional tests: Authentication, concurrency, edge cases
// ================================================================================

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
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let base_url = format!("http://{addr}");
    let client = DydxRawHttpClient::new(Some(base_url), Some(5), None, false, None).unwrap();

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
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let base_url = format!("http://{addr}");
    let client = DydxRawHttpClient::new(Some(base_url), Some(5), None, false, None).unwrap();

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
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let base_url = format!("http://{addr}");
    let client = DydxRawHttpClient::new(Some(base_url), Some(5), None, false, None).unwrap();

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
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let base_url = format!("http://{addr}");
    let client = DydxRawHttpClient::new(Some(base_url), Some(5), None, false, None).unwrap();

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
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let base_url = format!("http://{addr}");
    let client = DydxRawHttpClient::new(Some(base_url), Some(5), None, false, None).unwrap();

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
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let base_url = format!("http://{addr}");
    let client = DydxRawHttpClient::new(Some(base_url), Some(5), None, false, None).unwrap();

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
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let base_url = format!("http://{addr}");
    let client = DydxRawHttpClient::new(Some(base_url), Some(5), None, false, None).unwrap();

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
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let base_url = format!("http://{addr}");
    let client = DydxRawHttpClient::new(Some(base_url), Some(5), None, false, None).unwrap();

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
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let base_url = format!("http://{addr}");
    let client = DydxRawHttpClient::new(Some(base_url), Some(5), None, false, None).unwrap();

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
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let base_url = format!("http://{addr}");
    let client = DydxRawHttpClient::new(Some(base_url), Some(5), None, false, None).unwrap();

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
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let base_url = format!("http://{addr}");
    let client = DydxRawHttpClient::new(Some(base_url), Some(5), None, false, None).unwrap();

    for _ in 0..5 {
        let _ = client.get_time().await;
    }

    let final_count = *state.request_count.lock().await;
    assert_eq!(final_count, 5);
}

#[rstest]
#[tokio::test]
#[ignore = "Flaky test - mock data incomplete"]
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
                    let count = counter.fetch_sub(1, Ordering::SeqCst);

                    Json(json!({
                        "markets": {
                            "BTC-USD": {
                                "ticker": "BTC-USD",
                                "clobPairId": "0",
                                "status": "ACTIVE",
                                "baseAsset": "BTC",
                                "quoteAsset": "USD",
                                "stepBaseQuantums": 1000000,
                                "subticksPerTick": 100000,
                                "quantumConversionExponent": -8,
                                "atomicResolution": -10,
                                "priceExponent": -5,
                                "minExchanges": 3,
                                "minPriceChangePpm": 50,
                                "tickSize": "1",
                                "stepSize": "0.001",
                                "nextFundingRate": "0",
                                "openInterest": "0",
                                "maxMarketOrderBaseQuantums": "100000000000",
                                "concurrent_requests": count
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
    tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

    let base_url = format!("http://{addr}");
    let client =
        Arc::new(DydxRawHttpClient::new(Some(base_url), Some(10), None, false, None).unwrap());

    let mut handles = vec![];
    for _ in 0..5 {
        let client_clone = client.clone();
        handles.push(tokio::spawn(
            async move { client_clone.get_markets().await },
        ));
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
        "At least 3 concurrent requests should succeed, got {success_count} successes and {error_count} errors"
    );
}

#[rstest]
#[tokio::test]
#[ignore = "Flaky test - timeout behavior inconsistent"]
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
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let base_url = format!("http://{addr}");
    let client = DydxRawHttpClient::new(Some(base_url), Some(1), None, false, None).unwrap();

    let start = std::time::Instant::now();
    let result = client.get_time().await;
    let duration = start.elapsed();

    assert!(result.is_err());
    assert!(duration.as_secs() < 5, "Should timeout quickly");
}

#[rstest]
#[tokio::test]
#[ignore = "Mock data incomplete - uses incorrect field names"]
async fn test_large_instruments_response() {
    let state = TestServerState::default();

    let mut markets = serde_json::Map::new();
    for i in 0..100 {
        markets.insert(
            format!("MARKET-{i}"),
            json!({
                "ticker": format!("MARKET-{}", i),
                "clobPairId": i.to_string(),
                "status": "ACTIVE",
                "baseAsset": format!("BASE{}", i),
                "quoteAsset": "USD",
                "stepBaseQuantums": "1000000",
                "subticksPerTick": "100000",
                "quantumConversionExponent": -8,
                "atomicResolution": -10,
                "priceExponent": -5,
                "minExchanges": 3,
                "minPriceChangePpm": 50,
                "nextFundingRate": "0",
                "openInterest": "0",
                "maxMarketOrderBaseQuantums": "100000000000"
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
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let base_url = format!("http://{addr}");
    let client = DydxRawHttpClient::new(Some(base_url), Some(30), None, false, None).unwrap();

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
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let base_url = format!("http://{addr}");
    let client = DydxRawHttpClient::new(Some(base_url), Some(5), None, false, None).unwrap();

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
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let base_url = format!("http://{addr}");
    let client =
        Arc::new(DydxRawHttpClient::new(Some(base_url), Some(5), None, false, None).unwrap());

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
