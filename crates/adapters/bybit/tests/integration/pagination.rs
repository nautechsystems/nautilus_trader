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

//! Pagination tests for Bybit adapter.
//!
//! This test suite covers pagination for:
//! 1. Market Data (bars/klines) - chronological ordering, multi-page fetching
//! 2. Execution Endpoints - orders, trade history, positions with cursor pagination

use std::{collections::HashMap, net::SocketAddr};

use axum::{
    Router,
    extract::{Query, State},
    response::Json,
    routing::get,
};
use jiff::{SignedDuration, Timestamp};
use nautilus_bybit::{
    common::{consts::BYBIT_VENUE, enums::BybitProductType, parse::parse_linear_instrument},
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
    identifiers::{AccountId, InstrumentId, Symbol},
    instruments::Instrument,
};
use rstest::rstest;
use serde_json::{Value, json};
use tokio::net::TcpListener;

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
        .unwrap_or_else(|| Timestamp::now().as_millisecond());

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
        "time": Timestamp::now().as_millisecond()
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
        "time": Timestamp::now().as_millisecond()
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

async fn init_instrument_cache(client: &BybitHttpClient) {
    let mut params = BybitInstrumentsInfoParamsBuilder::default();
    params.category(BybitProductType::Linear);
    params.symbol("ETHUSDT".to_string());
    let params = params.build().unwrap();

    let response = client.get_instruments_linear(&params).await.unwrap();
    let ts_init = nautilus_core::time::get_atomic_clock_realtime().get_time_ns();

    for definition in response.result.list {
        let fee_rate = BybitFeeRate {
            symbol: definition.symbol,
            taker_fee_rate: "0.00055".to_string(),
            maker_fee_rate: "0.0001".to_string(),
            base_coin: Some(definition.base_coin),
        };

        let instrument = parse_linear_instrument(&definition, &fee_rate, ts_init, ts_init).unwrap();
        client.cache_instrument(instrument);
    }
}

#[rstest]
#[tokio::test]
async fn test_bars_chronological_order_single_page() {
    let addr = start_pagination_test_server().await.unwrap();
    let base_url = format!("http://{addr}");

    let client = BybitHttpClient::new(Some(base_url), 60, 3, 1000, 10_000, 5_000, None).unwrap();
    init_instrument_cache(&client).await;

    let instrument_id = InstrumentId::new(Symbol::from("ETHUSDT-LINEAR"), *BYBIT_VENUE);
    let bar_spec = BarSpecification {
        step: std::num::NonZero::new(1).unwrap(),
        aggregation: BarAggregation::Minute,
        price_type: PriceType::Last,
    };
    let bar_type = BarType::new(instrument_id, bar_spec, AggregationSource::External);

    let end = Timestamp::now();
    let start = end - SignedDuration::from_hours(1);

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
    let base_url = format!("http://{addr}");

    let client = BybitHttpClient::new(Some(base_url), 60, 3, 1000, 10_000, 5_000, None).unwrap();
    init_instrument_cache(&client).await;

    let instrument_id = InstrumentId::new(Symbol::from("ETHUSDT-LINEAR"), *BYBIT_VENUE);
    let bar_spec = BarSpecification {
        step: std::num::NonZero::new(1).unwrap(),
        aggregation: BarAggregation::Minute,
        price_type: PriceType::Last,
    };
    let bar_type = BarType::new(instrument_id, bar_spec, AggregationSource::External);

    let end = Timestamp::now();
    let start = end - SignedDuration::from_hours(2 * 24); // Request enough to trigger multiple pages

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
    let base_url = format!("http://{addr}");

    let client = BybitHttpClient::new(Some(base_url), 60, 3, 1000, 10_000, 5_000, None).unwrap();
    init_instrument_cache(&client).await;

    let instrument_id = InstrumentId::new(Symbol::from("ETHUSDT-LINEAR"), *BYBIT_VENUE);
    let bar_spec = BarSpecification {
        step: std::num::NonZero::new(1).unwrap(),
        aggregation: BarAggregation::Minute,
        price_type: PriceType::Last,
    };
    let bar_type = BarType::new(instrument_id, bar_spec, AggregationSource::External);

    let end = Timestamp::now();
    let start = end - SignedDuration::from_hours(3 * 24); // Request way more than limit

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
    let last_bar_time = bars.last().unwrap().ts_event.to_datetime_utc();
    let time_diff = (end - last_bar_time).get_minutes().abs();
    assert!(
        time_diff < 100,
        "Last bar should be close to end time, but was {time_diff} minutes away"
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
    assert_eq!(response.result.next_page_cursor, Some(String::new()));
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

    for response_json in &responses {
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
    let cursor: Option<String> = Some(String::new());

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

// ---------------------------------------------------------------------------------------------
// Execution endpoints: a `nextPageCursor` the venue stops advancing
// ---------------------------------------------------------------------------------------------

/// The cursor served with a repeating order page. Bybit composes it from the first and last row
/// of the page, so a page that keeps being re-served keeps producing the same value.
const REPEATED_ORDER_CURSOR: &str = "ORD-3:1789221005855,ORD-5:1789221005855";

/// The cursor served with a repeating execution page. This is the value in
/// `test_data/http_get_executions.json`, whose two halves already address the same row.
const REPEATED_EXECUTION_CURSOR: &str = "132766%3A2%2C132766%3A2";

/// Which endpoint stops advancing its cursor. The others serve one empty page and finish.
#[derive(Clone, Copy, PartialEq, Eq)]
enum StallingEndpoint {
    OrderRealtime,
    OrderHistory,
    ExecutionList,
}

#[derive(Clone)]
struct CursorStallState {
    stalling: StallingEndpoint,
    cursors: std::sync::Arc<tokio::sync::Mutex<Vec<Option<String>>>>,
}

impl CursorStallState {
    fn new(stalling: StallingEndpoint) -> Self {
        Self {
            stalling,
            cursors: std::sync::Arc::new(tokio::sync::Mutex::new(Vec::new())),
        }
    }
}

/// Field set taken from `test_data/http_get_orders_history_with_duplicate.json`, so the mock
/// deserializes through the same model as a live response.
fn stall_order_row(order_id: &str) -> Value {
    json!({
        "orderId": order_id,
        "orderLinkId": format!("client-{order_id}"),
        "blockTradeId": null,
        "symbol": "ETHUSDT",
        "price": "3930.41",
        "qty": "0.010",
        "side": "Buy",
        "isLeverage": "0",
        "positionIdx": 0,
        "orderStatus": "Cancelled",
        "cancelType": "CancelByUser",
        "rejectReason": "",
        "avgPrice": null,
        "leavesQty": "0",
        "leavesValue": "0",
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
        "lastPriceOnCreated": "3936.41",
        "reduceOnly": false,
        "closeOnTrigger": false,
        "smpType": "None",
        "smpGroup": 0,
        "smpOrderId": "0",
        "tpslMode": "Full",
        "tpLimitPrice": "0",
        "slLimitPrice": "0",
        "placeType": "order",
        "createdTime": "1789221005855",
        "updatedTime": "1789221005855"
    })
}

/// Field set taken from `test_data/http_get_executions.json`.
fn stall_execution_row(exec_id: &str) -> Value {
    json!({
        "symbol": "ETHUSDT",
        "orderId": "8c065341-7b52-4ca9-ac2c-37e31ac55c94",
        "orderLinkId": format!("client-{exec_id}"),
        "side": "Buy",
        "orderPrice": "3000.00",
        "orderQty": "0.100",
        "leavesQty": "0.000",
        "createType": "CreateByUser",
        "orderType": "Limit",
        "stopOrderType": "",
        "execFee": "0.0150",
        "execId": exec_id,
        "execPrice": "3000.00",
        "execQty": "0.010",
        "execType": "Trade",
        "execValue": "30.00",
        "execTime": "1789221005855",
        "feeCurrency": "USDT",
        "isMaker": true,
        "feeRate": "0.0003",
        "tradeIv": "",
        "markIv": "",
        "markPrice": "3000.50",
        "indexPrice": "3000.25",
        "underlyingPrice": "",
        "blockTradeId": "",
        "closedSize": "0.000",
        "seq": 4688002127i64
    })
}

fn stall_page(rows: &[Value], next_cursor: &str) -> Json<Value> {
    Json(json!({
        "retCode": 0,
        "retMsg": "OK",
        "result": {
            "category": "linear",
            "list": rows,
            "nextPageCursor": next_cursor
        },
        "retExtInfo": {},
        "time": 1789221005855i64
    }))
}

fn empty_page() -> Json<Value> {
    stall_page(&[], "")
}

/// Mirrors the page sequence observed on a live account: a page that advances normally, then a
/// page whose cursor addresses itself and is served unchanged for every request after it.
async fn serve_stalling_page(
    state: &CursorStallState,
    cursor: Option<String>,
    endpoint: StallingEndpoint,
) -> Json<Value> {
    if state.stalling != endpoint {
        return empty_page();
    }

    state.cursors.lock().await.push(cursor.clone());

    let (rows, repeated_cursor): (Vec<Value>, &str) = if endpoint == StallingEndpoint::ExecutionList
    {
        (
            vec![stall_execution_row("EXEC-3")],
            REPEATED_EXECUTION_CURSOR,
        )
    } else {
        (
            ["ORD-3", "ORD-4", "ORD-5"]
                .iter()
                .map(|id| stall_order_row(id))
                .collect(),
            REPEATED_ORDER_CURSOR,
        )
    };

    match cursor {
        None => {
            let first = if endpoint == StallingEndpoint::ExecutionList {
                vec![stall_execution_row("EXEC-1"), stall_execution_row("EXEC-2")]
            } else {
                vec![stall_order_row("ORD-1"), stall_order_row("ORD-2")]
            };
            stall_page(&first, "page-2")
        }
        Some(_) => stall_page(&rows, repeated_cursor),
    }
}

async fn mock_stalling_order_realtime(
    Query(params): Query<HashMap<String, String>>,
    State(state): State<CursorStallState>,
) -> Json<Value> {
    serve_stalling_page(
        &state,
        params.get("cursor").cloned(),
        StallingEndpoint::OrderRealtime,
    )
    .await
}

async fn mock_stalling_order_history(
    Query(params): Query<HashMap<String, String>>,
    State(state): State<CursorStallState>,
) -> Json<Value> {
    serve_stalling_page(
        &state,
        params.get("cursor").cloned(),
        StallingEndpoint::OrderHistory,
    )
    .await
}

async fn mock_stalling_execution_list(
    Query(params): Query<HashMap<String, String>>,
    State(state): State<CursorStallState>,
) -> Json<Value> {
    serve_stalling_page(
        &state,
        params.get("cursor").cloned(),
        StallingEndpoint::ExecutionList,
    )
    .await
}

async fn mock_instruments_info_with_state(
    query: Query<HashMap<String, String>>,
    State(_state): State<CursorStallState>,
) -> Json<Value> {
    mock_instruments_info(query).await
}

async fn start_cursor_stall_server(
    stalling: StallingEndpoint,
) -> Result<(SocketAddr, CursorStallState), anyhow::Error> {
    let state = CursorStallState::new(stalling);
    let app = Router::new()
        .route(
            "/v5/market/instruments-info",
            get(mock_instruments_info_with_state),
        )
        .route("/v5/order/realtime", get(mock_stalling_order_realtime))
        .route("/v5/order/history", get(mock_stalling_order_history))
        .route("/v5/execution/list", get(mock_stalling_execution_list))
        .with_state(state.clone());

    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    Ok((addr, state))
}

async fn cursor_stall_client(addr: SocketAddr) -> BybitHttpClient {
    let client = BybitHttpClient::with_credentials(
        "test_api_key".to_string(),
        "test_api_secret".to_string(),
        Some(format!("http://{addr}")),
        60,
        3,
        1000,
        10_000,
        5_000,
        None,
    )
    .unwrap();
    init_instrument_cache(&client).await;
    client
}

/// Asserts the walk reached the repeating page, then that it ended rather than following the
/// cursor forever. The cursor sequence identifies the path: only a walk that follows
/// `nextPageCursor` on this endpoint produces it, and the third entry is the request made with
/// the cursor the second page returned about itself.
///
/// The page observed on a live account addresses itself, so the walk reports the cursor as
/// stalled rather than as a longer cycle.
async fn assert_walk_reports_the_repeat<T>(
    outcome: Result<anyhow::Result<T>, tokio::time::error::Elapsed>,
    state: &CursorStallState,
    endpoint: &str,
    repeated_cursor: &str,
    call: &str,
) {
    let cursors = state.cursors.lock().await.clone();
    let repeats = cursors
        .iter()
        .filter(|cursor| cursor.as_deref() == Some(repeated_cursor))
        .count();

    assert_eq!(
        cursors.get(..3).map(<[Option<String>]>::to_vec),
        Some(vec![
            None,
            Some("page-2".to_string()),
            Some(repeated_cursor.to_string()),
        ]),
        "unexpected pagination path for {endpoint}, first four of {} requests: {:?}",
        cursors.len(),
        cursors.get(..4).unwrap_or(&cursors)
    );

    let result = outcome.unwrap_or_else(|_| {
        panic!(
            "{call} never returned; {endpoint} was requested {} times, {repeats} of them with \
             the cursor the venue repeated",
            cursors.len(),
        )
    });

    // Stopping with the rows gathered so far would hand reconciliation a truncated history it
    // cannot tell from a complete one, so the walk reports what the venue did.
    let error = result
        .err()
        .expect("a repeated cursor should be reported, not silently truncated")
        .to_string();
    assert_eq!(
        error,
        format!("{endpoint} pagination cursor did not advance from {repeated_cursor:?}")
    );
}

#[rstest]
#[tokio::test]
async fn test_order_status_reports_terminate_on_repeated_history_cursor() {
    // `generate_order_status_reports` passes `None` for the limit at both of its call sites, so
    // the `remaining == 0` exit is dead on the live reconciliation path and an empty cursor is
    // the only exit left.
    let (addr, state) = start_cursor_stall_server(StallingEndpoint::OrderHistory)
        .await
        .unwrap();
    let client = cursor_stall_client(addr).await;

    let outcome = tokio::time::timeout(
        tokio::time::Duration::from_secs(5),
        client.request_order_status_reports(
            AccountId::from("BYBIT-UNIFIED"),
            BybitProductType::Linear,
            None,  // no instrument: `generate_mass_status` never sets one
            false, // open_only=false: the history endpoint is walked
            None,  // start
            None,  // end
            None,  // limit
        ),
    )
    .await;

    assert_walk_reports_the_repeat(
        outcome,
        &state,
        "/v5/order/history",
        REPEATED_ORDER_CURSOR,
        "request_order_status_reports",
    )
    .await;
}

#[rstest]
#[tokio::test]
async fn test_order_status_reports_terminate_on_repeated_open_order_cursor() {
    // The open-order pass of the same call. Its budget is computed from the deduplicated order
    // count, so a page that repeats rows it has already seen adds nothing to it and the
    // `remaining == 0` exit stays out of reach even when a limit is given.
    let (addr, state) = start_cursor_stall_server(StallingEndpoint::OrderRealtime)
        .await
        .unwrap();
    let client = cursor_stall_client(addr).await;

    let outcome = tokio::time::timeout(
        tokio::time::Duration::from_secs(5),
        client.request_order_status_reports(
            AccountId::from("BYBIT-UNIFIED"),
            BybitProductType::Linear,
            None,
            false,
            None,
            None,
            Some(100), // a limit does not bound this loop
        ),
    )
    .await;

    assert_walk_reports_the_repeat(
        outcome,
        &state,
        "/v5/order/realtime",
        REPEATED_ORDER_CURSOR,
        "request_order_status_reports",
    )
    .await;
}

#[rstest]
#[tokio::test]
async fn test_open_only_order_reports_terminate_on_repeated_cursor() {
    // The `open_only` branch is a third loop over the same endpoint, reached by the periodic
    // open-order check rather than by startup reconciliation. Its budget is also computed from a
    // deduplicated count, so a limit does not bound it either.
    let (addr, state) = start_cursor_stall_server(StallingEndpoint::OrderRealtime)
        .await
        .unwrap();
    let client = cursor_stall_client(addr).await;

    let outcome = tokio::time::timeout(
        tokio::time::Duration::from_secs(5),
        client.request_order_status_reports(
            AccountId::from("BYBIT-UNIFIED"),
            BybitProductType::Linear,
            None,
            true, // open_only
            None,
            None,
            Some(100),
        ),
    )
    .await;

    assert_walk_reports_the_repeat(
        outcome,
        &state,
        "/v5/order/realtime",
        REPEATED_ORDER_CURSOR,
        "request_order_status_reports",
    )
    .await;
}

#[rstest]
#[tokio::test]
async fn test_fill_reports_terminate_on_repeated_cursor() {
    let (addr, state) = start_cursor_stall_server(StallingEndpoint::ExecutionList)
        .await
        .unwrap();
    let client = cursor_stall_client(addr).await;

    let outcome = tokio::time::timeout(
        tokio::time::Duration::from_secs(5),
        client.request_fill_reports(
            AccountId::from("BYBIT-UNIFIED"),
            BybitProductType::Linear,
            None, // no instrument
            None, // start
            None, // end
            None, // limit
        ),
    )
    .await;

    assert_walk_reports_the_repeat(
        outcome,
        &state,
        "/v5/execution/list",
        REPEATED_EXECUTION_CURSOR,
        "request_fill_reports",
    )
    .await;
}

// ---------------------------------------------------------------------------------------------
// Instrument pagination: a cursor cycle rather than a stall
// ---------------------------------------------------------------------------------------------

/// Records the `cursor` query parameter of every instruments-info request, in order.
#[derive(Clone, Default)]
struct CursorLog {
    cursors: std::sync::Arc<tokio::sync::Mutex<Vec<Option<String>>>>,
}

fn linear_instrument_definition(symbol: &str, base_coin: &str) -> Value {
    json!({
        "symbol": symbol,
        "contractType": "LinearPerpetual",
        "status": "Trading",
        "baseCoin": base_coin,
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
    })
}

/// Serves three pages whose cursors cycle `a -> b -> a`. A comparison against the previous cursor
/// alone never fires here, because no cursor equals the one used immediately before it.
async fn mock_instruments_info_cycling(
    Query(params): Query<HashMap<String, String>>,
    State(log): State<CursorLog>,
) -> Json<Value> {
    let cursor = params.get("cursor").cloned();
    log.cursors.lock().await.push(cursor.clone());

    let (symbol, base_coin, next_cursor) = match cursor.as_deref() {
        None => ("ETHUSDT", "ETH", "cursor-a"),
        Some("cursor-a") => ("BTCUSDT", "BTC", "cursor-b"),
        _ => ("SOLUSDT", "SOL", "cursor-a"),
    };

    Json(json!({
        "retCode": 0,
        "retMsg": "OK",
        "result": {
            "nextPageCursor": next_cursor,
            "list": [linear_instrument_definition(symbol, base_coin)]
        },
        "retExtInfo": {},
        "time": 1789221005855i64
    }))
}

async fn mock_fee_rate(
    Query(_params): Query<HashMap<String, String>>,
    State(_log): State<CursorLog>,
) -> Json<Value> {
    Json(json!({
        "retCode": 0,
        "retMsg": "OK",
        "result": {
            "list": [
                {
                    "symbol": "ETHUSDT",
                    "takerFeeRate": "0.0006",
                    "makerFeeRate": "0.0001",
                    "baseCoin": ""
                },
                {
                    "symbol": "BTCUSDT",
                    "takerFeeRate": "0.0006",
                    "makerFeeRate": "0.0001",
                    "baseCoin": ""
                },
                {
                    "symbol": "SOLUSDT",
                    "takerFeeRate": "0.00075",
                    "makerFeeRate": "0.00025",
                    "baseCoin": ""
                }
            ]
        },
        "retExtInfo": {},
        "time": 1789221005855i64
    }))
}

async fn start_cycling_instruments_server() -> Result<(SocketAddr, CursorLog), anyhow::Error> {
    let log = CursorLog::default();
    let app = Router::new()
        .route(
            "/v5/market/instruments-info",
            get(mock_instruments_info_cycling),
        )
        .route("/v5/account/fee-rate", get(mock_fee_rate))
        .with_state(log.clone());

    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    Ok((addr, log))
}

#[rstest]
#[tokio::test]
async fn test_instrument_pagination_ends_on_a_cursor_cycle() {
    // Unlike the execution endpoints, this walk keeps what it has read and logs, because a short
    // instrument list is a visible gap rather than a silent one. What it must not do is keep
    // asking: the cursors here cycle `a -> b -> a`, which a comparison against the previous
    // cursor does not catch.
    let (addr, log) = start_cycling_instruments_server().await.unwrap();
    let client = BybitHttpClient::with_credentials(
        "test_api_key".to_string(),
        "test_api_secret".to_string(),
        Some(format!("http://{addr}")),
        60,
        3,
        1000,
        10_000,
        5_000,
        None,
    )
    .unwrap();

    let outcome = tokio::time::timeout(
        tokio::time::Duration::from_secs(5),
        client.request_instruments(BybitProductType::Linear, None, None),
    )
    .await;

    // Identifies the path: only a walk that follows `nextPageCursor` produces this prefix, and
    // the request after it would have reopened the cycle at `cursor-a`.
    let cursors = log.cursors.lock().await.clone();
    assert_eq!(
        cursors.get(..3).map(<[Option<String>]>::to_vec),
        Some(vec![
            None,
            Some("cursor-a".to_string()),
            Some("cursor-b".to_string()),
        ]),
        "unexpected instrument pagination path, first four of {} requests: {:?}",
        cursors.len(),
        cursors.get(..4).unwrap_or(&cursors)
    );

    let instruments = outcome
        .unwrap_or_else(|_| {
            panic!(
                "request_instruments never returned; instruments-info was requested {} times, \
                 cycling between cursor-a and cursor-b",
                cursors.len()
            )
        })
        .expect("a cursor cycle leaves the instruments already read usable");

    assert_eq!(
        cursors.len(),
        3,
        "the walk reopened the cycle instead of stopping at it"
    );

    let mut symbols: Vec<String> = instruments
        .iter()
        .map(|instrument| instrument.id().symbol.to_string())
        .collect();
    symbols.sort();
    assert_eq!(
        symbols,
        vec!["BTCUSDT-LINEAR", "ETHUSDT-LINEAR", "SOLUSDT-LINEAR"],
        "the pages read before the cycle should still be returned"
    );
}
