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

//! Integration tests for the Kalshi HTTP client using a mock server.

use std::{net::SocketAddr, path::PathBuf, sync::Arc};

use axum::{Router, response::Json, routing::get};
use nautilus_kalshi::{
    common::enums::CandlestickInterval,
    config::KalshiDataClientConfig,
    http::client::KalshiHttpClient,
};
use serde_json::Value;

fn data_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test_data")
}

fn load_json(filename: &str) -> Value {
    let content = std::fs::read_to_string(data_path().join(filename))
        .unwrap_or_else(|_| panic!("missing test fixture: {filename}"));
    serde_json::from_str(&content).expect("invalid json")
}

/// Starts an axum mock server and returns the bound address.
async fn start_mock_server(markets: Value, trades: Value, candlesticks: Value) -> SocketAddr {
    let markets = Arc::new(markets);
    let trades = Arc::new(trades);
    let candlesticks = Arc::new(candlesticks);

    let app = Router::new()
        .route(
            "/trade-api/v2/markets",
            get({
                let markets = Arc::clone(&markets);
                move || {
                    let markets = Arc::clone(&markets);
                    async move { Json((*markets).clone()) }
                }
            }),
        )
        .route(
            "/trade-api/v2/markets/trades",
            get({
                let trades = Arc::clone(&trades);
                move || {
                    let trades = Arc::clone(&trades);
                    async move { Json((*trades).clone()) }
                }
            }),
        )
        .route(
            "/trade-api/v2/historical/markets/{ticker}/candlesticks",
            get({
                let candlesticks = Arc::clone(&candlesticks);
                move |_params: axum::extract::Path<String>| {
                    let candlesticks = Arc::clone(&candlesticks);
                    async move { Json((*candlesticks).clone()) }
                }
            }),
        );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(axum::serve(listener, app).into_future());
    addr
}

fn make_client(addr: SocketAddr) -> KalshiHttpClient {
    let mut config = KalshiDataClientConfig::new();
    config.base_url = Some(format!("http://{addr}/trade-api/v2"));
    KalshiHttpClient::new(config)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_get_markets_returns_parsed_response() {
    let markets_json = load_json("http_markets.json");
    let trades_json = load_json("http_trades.json");
    let candlesticks_json = load_json("http_candlesticks.json");
    let addr = start_mock_server(markets_json, trades_json, candlesticks_json).await;

    let client = make_client(addr);
    let markets = client.get_markets(&[], &["KXBTC"]).await.unwrap();

    assert_eq!(markets.len(), 1);
    assert_eq!(markets[0].ticker.as_str(), "KXBTC-25MAR15-B100000");
    assert_eq!(markets[0].event_ticker.as_str(), "KXBTC-25MAR15");
}

#[tokio::test]
async fn test_get_trades_returns_parsed_response() {
    let markets_json = load_json("http_markets.json");
    let trades_json = load_json("http_trades.json");
    let candlesticks_json = load_json("http_candlesticks.json");
    let addr = start_mock_server(markets_json, trades_json, candlesticks_json).await;

    let client = make_client(addr);
    let (trades, next_cursor) = client
        .get_trades("KXBTC-25MAR15-B100000", None, None, None)
        .await
        .unwrap();

    assert_eq!(trades.len(), 1);
    assert_eq!(trades[0].ticker.as_str(), "KXBTC-25MAR15-B100000");
    assert_eq!(trades[0].yes_price_dollars, "0.3600");
    // Fixture has cursor "" — should be normalised to None.
    assert_eq!(next_cursor, None);
}

#[tokio::test]
async fn test_get_candlesticks_returns_parsed_response() {
    let markets_json = load_json("http_markets.json");
    let trades_json = load_json("http_trades.json");
    let candlesticks_json = load_json("http_candlesticks.json");
    let addr = start_mock_server(markets_json, trades_json, candlesticks_json).await;

    let client = make_client(addr);
    let candles = client
        .get_candlesticks(
            "KXBTC-25MAR15-B100000",
            1_741_046_400,
            1_741_132_800,
            CandlestickInterval::Hours1,
        )
        .await
        .unwrap();

    assert_eq!(candles.len(), 1);
    assert_eq!(candles[0].end_period_ts, 1_741_046_400);
    assert_eq!(candles[0].volume, "1250.00");
}

#[tokio::test]
async fn test_get_orderbook_requires_credential() {
    let markets_json = load_json("http_markets.json");
    let trades_json = load_json("http_trades.json");
    let candlesticks_json = load_json("http_candlesticks.json");
    let addr = start_mock_server(markets_json, trades_json, candlesticks_json).await;

    // Client with no credentials.
    let client = make_client(addr);
    let err = client
        .get_orderbook("KXBTC-25MAR15-B100000", None)
        .await
        .unwrap_err();

    assert!(err.is_auth_error(), "expected auth error, got: {err}");
}
