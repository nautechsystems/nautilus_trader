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

//! Comprehensive pagination tests for Bybit adapter.
//!
//! This test suite covers pagination for:
//! 1. Market Data (bars/klines) - chronological ordering, multi-page fetching
//! 2. Execution Endpoints - orders, trade history, positions with cursor pagination

use std::{collections::HashMap, net::SocketAddr};

use axum::{Router, extract::Query, response::Json, routing::get};
use chrono::{DateTime, Duration, Utc};
use nautilus_bybit::{
    common::{enums::BybitProductType, parse::parse_linear_instrument},
    http::{
        client::BybitHttpClient,
        models::{
            BybitFeeRate, BybitOpenOrdersResponse, BybitOrderHistoryResponse,
            BybitPositionListResponse, BybitTradeHistoryResponse,
        },
        query::BybitInstrumentsInfoParamsBuilder,
    },
};
use nautilus_model::{
    data::{BarSpecification, BarType},
    enums::{AggregationSource, BarAggregation, PriceType},
    identifiers::{InstrumentId, Symbol, Venue},
};
use rstest::rstest;
use serde_json::{Value, json};
use tokio::{net::TcpListener, sync::OnceCell};

// Generate mock kline data with timestamps
fn generate_kline(timestamp_ms: i64, open: &str, high: &str, low: &str, close: &str) -> Value {
    json!([
        timestamp_ms.to_string(),
        open,
        high,
        low,
        close,
        "100.0",    // volume
        "100000.0"  // turnover
    ])
}

// Mock endpoint that simulates pagination
async fn mock_klines_paginated(Query(params): Query<HashMap<String, String>>) -> Json<Value> {
    let end_ms = params
        .get("end")
        .and_then(|s| s.parse::<i64>().ok())
        .unwrap_or_else(|| Utc::now().timestamp_millis());

    // Generate bars going backwards from end_ms
    // Each bar is 1 minute apart
    let mut klines = Vec::new();
    for i in 0..1000 {
        let bar_time = end_ms - (i * 60_000);
        klines.push(generate_kline(
            bar_time, "50000.0", "50100.0", "49900.0", "50050.0",
        ));
    }

    Json(json!({
        "retCode": 0,
        "retMsg": "OK",
        "result": {
            "category": "linear",
            "symbol": "ETHUSDT",
            "list": klines
        },
        "time": Utc::now().timestamp_millis()
    }))
}

// Mock instrument info endpoint
async fn mock_instruments_info(Query(_params): Query<HashMap<String, String>>) -> Json<Value> {
    Json(json!({
        "retCode": 0,
        "retMsg": "OK",
        "result": {
            "nextPageCursor": null,
            "list": [{
                "symbol": "ETHUSDT",
                "contractType": "LinearPerpetual",
                "status": "Trading",
                "baseCoin": "ETH",
                "quoteCoin": "USDT",
                "launchTime": "1699990000000",
                "deliveryTime": "1702592000000",
                "deliveryFeeRate": "0.0005",
                "priceScale": "2",
                "leverageFilter": {
                    "minLeverage": "1",
                    "maxLeverage": "100",
                    "leverageStep": "1"
                },
                "priceFilter": {
                    "minPrice": "0.1",
                    "maxPrice": "100000",
                    "tickSize": "0.05"
                },
                "lotSizeFilter": {
                    "maxOrderQty": "1000.0",
                    "minOrderQty": "0.01",
                    "qtyStep": "0.01",
                    "postOnlyMaxOrderQty": "1000.0",
                    "maxMktOrderQty": "500.0",
                    "minNotionalValue": "5"
                },
                "unifiedMarginTrade": true,
                "fundingInterval": 8,
                "settleCoin": "USDT"
            }]
        },
        "time": Utc::now().timestamp_millis()
    }))
}

async fn start_pagination_test_server() -> Result<SocketAddr, anyhow::Error> {
    let app = Router::new()
        .route("/v5/market/kline", get(mock_klines_paginated))
        .route("/v5/market/instruments-info", get(mock_instruments_info));

    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    // Give server time to start
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    Ok(addr)
}

static INSTRUMENT_CACHE: OnceCell<()> = OnceCell::const_new();

async fn init_instrument_cache(client: &BybitHttpClient) {
    INSTRUMENT_CACHE
        .get_or_init(|| async {
            // Load instruments and manually add to cache
            let mut params = BybitInstrumentsInfoParamsBuilder::default();
            params.category(BybitProductType::Linear);
            params.symbol("ETHUSDT".to_string());
            let params = params.build().unwrap();

            if let Ok(response) = client.get_instruments_linear(&params).await {
                use nautilus_core::time::get_atomic_clock_realtime;
                let ts_init = get_atomic_clock_realtime().get_time_ns();
                for definition in response.result.list {
                    // Create a default fee rate for testing
                    let fee_rate = BybitFeeRate {
                        symbol: definition.symbol,
                        taker_fee_rate: "0.00055".to_string(),
                        maker_fee_rate: "0.0001".to_string(),
                        base_coin: Some(definition.base_coin),
                    };

                    if let Ok(instrument) =
                        parse_linear_instrument(&definition, &fee_rate, ts_init, ts_init)
                    {
                        client.cache_instrument(instrument);
                    }
                }
            }
        })
        .await;
}

#[rstest]
#[tokio::test]
async fn test_bars_chronological_order_single_page() {
    let addr = start_pagination_test_server().await.unwrap();
    let base_url = format!("http://{}", addr);

    let client =
        BybitHttpClient::new(Some(base_url), Some(60), None, None, None, None, None).unwrap();
    init_instrument_cache(&client).await;

    let instrument_id = InstrumentId::new(Symbol::from("ETHUSDT-LINEAR"), Venue::from("BYBIT"));
    let bar_spec = BarSpecification {
        step: std::num::NonZero::new(1).unwrap(),
        aggregation: BarAggregation::Minute,
        price_type: PriceType::Last,
    };
    let bar_type = BarType::new(instrument_id, bar_spec, AggregationSource::External);

    let end = Utc::now();
    let start = end - Duration::hours(1);

    let bars = client
        .request_bars(
            BybitProductType::Linear,
            bar_type,
            Some(start),
            Some(end),
            Some(100),
            false,
        )
        .await
        .unwrap();

    // Verify we got bars
    assert!(!bars.is_empty());
    assert!(bars.len() <= 100);

    // Verify chronological order (each bar should be later than the previous)
    for i in 1..bars.len() {
        assert!(
            bars[i].ts_event >= bars[i - 1].ts_event,
            "Bars not in chronological order at index {}: {:?} should be >= {:?}",
            i,
            bars[i].ts_event,
            bars[i - 1].ts_event
        );
    }
}

#[rstest]
#[tokio::test]
async fn test_bars_chronological_order_multiple_pages() {
    let addr = start_pagination_test_server().await.unwrap();
    let base_url = format!("http://{}", addr);

    let client =
        BybitHttpClient::new(Some(base_url), Some(60), None, None, None, None, None).unwrap();
    init_instrument_cache(&client).await;

    let instrument_id = InstrumentId::new(Symbol::from("ETHUSDT-LINEAR"), Venue::from("BYBIT"));
    let bar_spec = BarSpecification {
        step: std::num::NonZero::new(1).unwrap(),
        aggregation: BarAggregation::Minute,
        price_type: PriceType::Last,
    };
    let bar_type = BarType::new(instrument_id, bar_spec, AggregationSource::External);

    let end = Utc::now();
    let start = end - Duration::days(2); // Request enough to trigger multiple pages

    let bars = client
        .request_bars(
            BybitProductType::Linear,
            bar_type,
            Some(start),
            Some(end),
            Some(1500), // More than one page (1000)
            false,
        )
        .await
        .unwrap();

    // Verify we got approximately the requested number of bars
    assert!(!bars.is_empty());
    // Should get around 1500 bars (might be slightly less due to time boundaries)
    assert!(bars.len() >= 1000, "Expected multiple pages of bars");

    // Verify strict chronological order across all pages
    for i in 1..bars.len() {
        assert!(
            bars[i].ts_event >= bars[i - 1].ts_event,
            "Bars not in chronological order at index {}: bar[{}].ts_event={:?} should be >= bar[{}].ts_event={:?}",
            i,
            i,
            bars[i].ts_event,
            i - 1,
            bars[i - 1].ts_event
        );
    }
}

#[rstest]
#[tokio::test]
async fn test_bars_limit_returns_most_recent() {
    let addr = start_pagination_test_server().await.unwrap();
    let base_url = format!("http://{}", addr);

    let client =
        BybitHttpClient::new(Some(base_url), Some(60), None, None, None, None, None).unwrap();
    init_instrument_cache(&client).await;

    let instrument_id = InstrumentId::new(Symbol::from("ETHUSDT-LINEAR"), Venue::from("BYBIT"));
    let bar_spec = BarSpecification {
        step: std::num::NonZero::new(1).unwrap(),
        aggregation: BarAggregation::Minute,
        price_type: PriceType::Last,
    };
    let bar_type = BarType::new(instrument_id, bar_spec, AggregationSource::External);

    let end = Utc::now();
    let start = end - Duration::days(3); // Request way more than limit

    let bars = client
        .request_bars(
            BybitProductType::Linear,
            bar_type,
            Some(start),
            Some(end),
            Some(500), // Limit to 500 bars
            false,
        )
        .await
        .unwrap();

    // Verify we got exactly the limit
    assert_eq!(bars.len(), 500);

    // Verify chronological order
    for i in 1..bars.len() {
        assert!(bars[i].ts_event >= bars[i - 1].ts_event);
    }

    // The last bar should be the most recent (close to end time)
    let last_bar_time = DateTime::from_timestamp_nanos(bars.last().unwrap().ts_event.as_i64());
    let time_diff = (end - last_bar_time).num_minutes().abs();
    assert!(
        time_diff < 100,
        "Last bar should be close to end time, but was {} minutes away",
        time_diff
    );
}

/// Test that BybitOpenOrdersResponse properly deserializes with cursor
#[rstest]
fn test_open_orders_response_with_cursor() {
    let json = r#"{
        "retCode": 0,
        "retMsg": "OK",
        "result": {
            "list": [
                {
                    "orderId": "order-1",
                    "orderLinkId": "client-1",
                    "blockTradeId": null,
                    "symbol": "BTCUSDT",
                    "price": "50000.00",
                    "qty": "0.100",
                    "side": "Buy",
                    "isLeverage": "0",
                    "positionIdx": 0,
                    "orderStatus": "New",
                    "cancelType": "",
                    "rejectReason": "",
                    "avgPrice": null,
                    "leavesQty": "0.100",
                    "leavesValue": "5000.00",
                    "cumExecQty": "0",
                    "cumExecValue": "0",
                    "cumExecFee": "0",
                    "timeInForce": "GTC",
                    "orderType": "Limit",
                    "stopOrderType": "",
                    "orderIv": null,
                    "triggerPrice": "0",
                    "takeProfit": "0",
                    "stopLoss": "0",
                    "tpTriggerBy": "LastPrice",
                    "slTriggerBy": "LastPrice",
                    "triggerDirection": 0,
                    "triggerBy": "LastPrice",
                    "lastPriceOnCreated": "50000.00",
                    "reduceOnly": false,
                    "closeOnTrigger": false,
                    "smpType": "None",
                    "smpGroup": 0,
                    "smpOrderId": "0",
                    "tpslMode": "Full",
                    "tpLimitPrice": "0",
                    "slLimitPrice": "0",
                    "placeType": "order",
                    "createdTime": "1672282722429",
                    "updatedTime": "1672282722429"
                }
            ],
            "nextPageCursor": "cursor-page-2"
        },
        "time": 1672282722429
    }"#;

    let response: BybitOpenOrdersResponse = serde_json::from_str(json).unwrap();
    assert_eq!(response.ret_code, 0);
    assert_eq!(response.result.list.len(), 1);
    assert_eq!(
        response.result.next_page_cursor,
        Some("cursor-page-2".to_string())
    );
}

/// Test that empty cursor properly deserializes
#[rstest]
fn test_open_orders_response_empty_cursor() {
    let json = r#"{
        "retCode": 0,
        "retMsg": "OK",
        "result": {
            "list": [],
            "nextPageCursor": ""
        },
        "time": 1672282722429
    }"#;

    let response: BybitOpenOrdersResponse = serde_json::from_str(json).unwrap();
    assert_eq!(response.ret_code, 0);
    assert!(response.result.list.is_empty());
    // Empty string should deserialize to Some("")
    assert_eq!(response.result.next_page_cursor, Some("".to_string()));
}

/// Test that order history response supports cursor pagination
#[rstest]
fn test_order_history_response_with_cursor() {
    let json = r#"{
        "retCode": 0,
        "retMsg": "OK",
        "result": {
            "list": [],
            "nextPageCursor": "next-page"
        },
        "time": 1672282722429
    }"#;

    let response: BybitOrderHistoryResponse = serde_json::from_str(json).unwrap();
    assert_eq!(
        response.result.next_page_cursor,
        Some("next-page".to_string())
    );
}

/// Test that trade history response supports cursor pagination
#[rstest]
fn test_trade_history_response_with_cursor() {
    let json = r#"{
        "retCode": 0,
        "retMsg": "OK",
        "result": {
            "list": [],
            "nextPageCursor": "execution-cursor"
        },
        "time": 1672282722429
    }"#;

    let response: BybitTradeHistoryResponse = serde_json::from_str(json).unwrap();
    assert_eq!(
        response.result.next_page_cursor,
        Some("execution-cursor".to_string())
    );
}

/// Test that position list response supports cursor pagination
#[rstest]
fn test_position_list_response_with_cursor() {
    let json = r#"{
        "retCode": 0,
        "retMsg": "OK",
        "result": {
            "list": [],
            "nextPageCursor": "position-cursor"
        },
        "time": 1672282722429
    }"#;

    let response: BybitPositionListResponse = serde_json::from_str(json).unwrap();
    assert_eq!(
        response.result.next_page_cursor,
        Some("position-cursor".to_string())
    );
}

/// Test the pagination loop pattern that's used in the implementation
#[rstest]
fn test_pagination_loop_pattern() {
    // Simulate pagination responses
    let responses = [
        r#"{"retCode": 0, "retMsg": "OK", "result": {"list": ["item1", "item2"], "nextPageCursor": "page2"}, "time": 123}"#,
        r#"{"retCode": 0, "retMsg": "OK", "result": {"list": ["item3", "item4"], "nextPageCursor": "page3"}, "time": 123}"#,
        r#"{"retCode": 0, "retMsg": "OK", "result": {"list": ["item5"], "nextPageCursor": ""}, "time": 123}"#,
    ];

    // Simulate the pagination loop
    let mut all_items: Vec<String> = Vec::new();
    let mut page_count = 0;

    for response_json in responses.iter() {
        #[derive(serde::Deserialize)]
        struct MockResponse {
            result: MockResult,
        }
        #[derive(serde::Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct MockResult {
            list: Vec<String>,
            next_page_cursor: Option<String>,
        }

        let response: MockResponse = serde_json::from_str(response_json).unwrap();
        all_items.extend(response.result.list);
        page_count += 1;

        let cursor = response.result.next_page_cursor;
        if cursor.is_none() || cursor.as_ref().is_none_or(|c| c.is_empty()) {
            break;
        }
    }

    assert_eq!(page_count, 3, "Should have processed 3 pages");
    assert_eq!(all_items.len(), 5, "Should have collected 5 total items");
    assert_eq!(all_items, vec!["item1", "item2", "item3", "item4", "item5"]);
}

/// Test that pagination stops on empty cursor
#[rstest]
fn test_pagination_stops_on_empty_cursor() {
    let cursor: Option<String> = Some("".to_string());

    // This is the termination condition used in the pagination loops
    let should_stop = cursor.is_none() || cursor.as_ref().is_none_or(|c| c.is_empty());

    assert!(should_stop, "Empty cursor should terminate pagination");
}

/// Test that pagination continues with valid cursor
#[rstest]
fn test_pagination_continues_with_valid_cursor() {
    let cursor: Option<String> = Some("next-page".to_string());

    // This is the termination condition used in the pagination loops
    let should_stop = cursor.is_none() || cursor.as_ref().is_none_or(|c| c.is_empty());

    assert!(!should_stop, "Valid cursor should continue pagination");
}

/// Test that limit calculation respects remaining items correctly
#[rstest]
fn test_limit_calculation() {
    // Test case 1: limit=10, total=0, should request min(10, 50) = 10
    let limit = 10u32;
    let total = 0;
    let remaining = (limit as usize).saturating_sub(total);
    let page_limit = std::cmp::min(remaining, 50);
    assert_eq!(page_limit, 10, "Should request exactly 10 items");

    // Test case 2: limit=10, total=5, should request min(5, 50) = 5
    let total = 5;
    let remaining = (limit as usize).saturating_sub(total);
    let page_limit = std::cmp::min(remaining, 50);
    assert_eq!(page_limit, 5, "Should request exactly 5 remaining items");

    // Test case 3: limit=10, total=10, should request 0
    let total = 10;
    let remaining = (limit as usize).saturating_sub(total);
    assert_eq!(remaining, 0, "Should have no remaining items to request");

    // Test case 4: limit=10, total=15, should request 0 (saturating)
    let total = 15;
    let remaining = (limit as usize).saturating_sub(total);
    assert_eq!(remaining, 0, "Should saturate at 0 when over limit");

    // Test case 5: limit=100, total=0, should request min(100, 50) = 50 (API max)
    let limit = 100u32;
    let total = 0;
    let remaining = (limit as usize).saturating_sub(total);
    let page_limit = std::cmp::min(remaining, 50);
    assert_eq!(page_limit, 50, "Should respect API maximum of 50");

    // Test case 6: limit=100, total=75, should request min(25, 50) = 25
    let total = 75;
    let remaining = (limit as usize).saturating_sub(total);
    let page_limit = std::cmp::min(remaining, 50);
    assert_eq!(page_limit, 25, "Should request exactly 25 remaining items");

    // Test case 7: no limit (None), should use usize::MAX
    let limit: Option<u32> = None;
    let total = 1000;
    let remaining = if let Some(l) = limit {
        (l as usize).saturating_sub(total)
    } else {
        usize::MAX
    };
    assert_eq!(
        remaining,
        usize::MAX,
        "Should have unlimited remaining when no limit"
    );
}

/// Test execution limit calculation with API max of 100
#[rstest]
fn test_execution_limit_calculation() {
    // Test case 1: limit=50, total=0, should request min(50, 100) = 50
    let limit = 50u32;
    let total = 0;
    let remaining = (limit as usize).saturating_sub(total);
    let page_limit = std::cmp::min(remaining, 100);
    assert_eq!(page_limit, 50, "Should request exactly 50 executions");

    // Test case 2: limit=150, total=0, should request min(150, 100) = 100 (API max)
    let limit = 150u32;
    let total = 0;
    let remaining = (limit as usize).saturating_sub(total);
    let page_limit = std::cmp::min(remaining, 100);
    assert_eq!(page_limit, 100, "Should respect API maximum of 100");

    // Test case 3: limit=150, total=100, should request min(50, 100) = 50
    let total = 100;
    let remaining = (limit as usize).saturating_sub(total);
    let page_limit = std::cmp::min(remaining, 100);
    assert_eq!(
        page_limit, 50,
        "Should request exactly 50 remaining executions"
    );
}
