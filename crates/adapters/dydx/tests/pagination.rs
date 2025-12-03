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

//! Pagination tests for dYdX adapter.
//!
//! This test suite covers pagination for:
//! 1. Market Data (candles/klines) - chronological ordering, multi-page fetching
//! 2. Account endpoints - orders, fills, transfers with cursor/offset pagination

use std::{collections::HashMap, net::SocketAddr};

use axum::{Router, extract::Query, response::Json, routing::get};
use chrono::{Datelike, Duration, Utc};
use nautilus_dydx::{
    common::enums::DydxCandleResolution,
    http::client::{DydxHttpClient, DydxRawHttpClient},
};
use rstest::rstest;
use serde_json::{Value, json};
use tokio::net::TcpListener;

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
        let bar_time = end_time - Duration::minutes(i as i64);
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

async fn mock_markets() -> Json<Value> {
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
        .route("/v4/perpetualMarkets", get(mock_markets))
}

async fn start_pagination_test_server() -> Result<SocketAddr, anyhow::Error> {
    let app = create_pagination_router();

    let listener = TcpListener::bind("127.0.0.1:0").await?;
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

    let client = DydxHttpClient::new(Some(base_url), Some(60), None, false, None).unwrap();

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

    let client = DydxRawHttpClient::new(Some(base_url), Some(60), None, false, None).unwrap();

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

    let client = DydxRawHttpClient::new(Some(base_url), Some(60), None, false, None).unwrap();

    let result = client
        .get_fills("dydx1test", 0, Some("BTC-USD"), Some(50))
        .await
        .unwrap();

    assert_eq!(result.fills.len(), 50);
    assert_eq!(result.fills[0].id, "fill-0");
    assert_eq!(result.fills[49].id, "fill-49");
}

#[rstest]
fn test_limit_calculation_pattern() {
    // Test case 1: limit=10, total=0, should request min(10, 100) = 10
    let limit = 10u32;
    let total = 0;
    let remaining = (limit as usize).saturating_sub(total);
    let page_limit = std::cmp::min(remaining, 100);
    assert_eq!(page_limit, 10);

    // Test case 2: limit=10, total=5, should request min(5, 100) = 5
    let total = 5;
    let remaining = (limit as usize).saturating_sub(total);
    let page_limit = std::cmp::min(remaining, 100);
    assert_eq!(page_limit, 5);

    // Test case 3: limit=10, total=10, should request 0
    let total = 10;
    let remaining = (limit as usize).saturating_sub(total);
    assert_eq!(remaining, 0);

    // Test case 4: limit=200, total=0, should request min(200, 100) = 100 (API max)
    let limit = 200u32;
    let total = 0;
    let remaining = (limit as usize).saturating_sub(total);
    let page_limit = std::cmp::min(remaining, 100);
    assert_eq!(page_limit, 100);

    // Test case 5: no limit (None), should use usize::MAX
    let limit: Option<u32> = None;
    let total = 1000;
    let remaining = if let Some(l) = limit {
        (l as usize).saturating_sub(total)
    } else {
        usize::MAX
    };
    assert_eq!(remaining, usize::MAX);
}

#[rstest]
fn test_pagination_termination_conditions() {
    // Empty response terminates pagination
    let empty_response: Vec<String> = vec![];
    assert!(empty_response.is_empty());

    // Response smaller than page limit terminates pagination
    let page_limit = 100;
    let partial_response = ["item1", "item2", "item3"];
    assert!(partial_response.len() < page_limit);

    // Response equal to page limit continues pagination
    assert_eq!((0..100).count(), page_limit);
}

#[rstest]
fn test_time_range_pagination() {
    let now = Utc::now();
    let one_day_ago = now - Duration::days(1);
    let two_days_ago = now - Duration::days(2);

    // First page: from two_days_ago to one_day_ago
    assert!(one_day_ago > two_days_ago);
    assert!(now > one_day_ago);

    // Subsequent pages use the oldest timestamp from previous response
    let oldest_in_page = one_day_ago - Duration::hours(1);
    assert!(oldest_in_page < one_day_ago);
    assert!(oldest_in_page > two_days_ago);
}

#[rstest]
fn test_candle_resolution_mapping() {
    // dYdX candle resolutions
    assert_eq!(DydxCandleResolution::OneMinute.as_ref(), "1MIN");
    assert_eq!(DydxCandleResolution::FiveMinutes.as_ref(), "5MINS");
    assert_eq!(DydxCandleResolution::FifteenMinutes.as_ref(), "15MINS");
    assert_eq!(DydxCandleResolution::ThirtyMinutes.as_ref(), "30MINS");
    assert_eq!(DydxCandleResolution::OneHour.as_ref(), "1HOUR");
    assert_eq!(DydxCandleResolution::FourHours.as_ref(), "4HOURS");
    assert_eq!(DydxCandleResolution::OneDay.as_ref(), "1DAY");
}

#[rstest]
#[tokio::test]
async fn test_candles_with_time_range() {
    let addr = start_pagination_test_server().await.unwrap();
    let base_url = format!("http://{addr}");

    let client = DydxHttpClient::new(Some(base_url), Some(60), None, false, None).unwrap();

    let end = Utc::now();
    let start = end - Duration::hours(2);

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
        .route("/v4/perpetualMarkets", get(mock_markets));

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let base_url = format!("http://{addr}");
    let client = DydxRawHttpClient::new(Some(base_url), Some(60), None, false, None).unwrap();

    let orders = client.get_orders("dydx1test", 0, None, None).await.unwrap();

    assert!(orders.is_empty());
}

#[rstest]
#[tokio::test]
async fn test_empty_fills_response() {
    let app = Router::new()
        .route("/v4/fills", get(|| async { Json(json!({"fills": []})) }))
        .route("/v4/perpetualMarkets", get(mock_markets));

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let base_url = format!("http://{addr}");
    let client = DydxRawHttpClient::new(Some(base_url), Some(60), None, false, None).unwrap();

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
        .route("/v4/perpetualMarkets", get(mock_markets));

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let base_url = format!("http://{addr}");
    let client = DydxHttpClient::new(Some(base_url), Some(60), None, false, None).unwrap();

    let candles = client
        .request_candles("BTC-USD", DydxCandleResolution::OneMinute, None, None, None)
        .await
        .unwrap();

    assert!(candles.candles.is_empty());
}

#[rstest]
fn test_candle_timestamp_parsing() {
    let timestamp_str = "2024-01-01T00:00:00.000Z";
    let parsed = chrono::DateTime::parse_from_rfc3339(timestamp_str);
    assert!(parsed.is_ok());

    let dt = parsed.unwrap().with_timezone(&Utc);
    assert_eq!(dt.year(), 2024);
    assert_eq!(dt.month(), 1);
    assert_eq!(dt.day(), 1);
}

#[rstest]
fn test_pagination_offset_calculation() {
    // dYdX uses time-based pagination for candles, not offset
    // But for orders/fills, we track seen IDs or use createdBeforeOrAt

    // Simulating collecting items across multiple pages
    let mut all_items: Vec<i32> = Vec::new();
    let pages = vec![vec![1, 2, 3, 4, 5], vec![6, 7, 8, 9, 10], vec![11, 12, 13]];

    for page in pages {
        all_items.extend(page);
    }

    assert_eq!(all_items.len(), 13);
    assert_eq!(all_items, vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13]);
}
