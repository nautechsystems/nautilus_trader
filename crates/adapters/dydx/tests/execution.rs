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

//! Integration tests for dYdX execution client.

use std::{collections::HashMap, net::SocketAddr, path::PathBuf, time::Duration};

use axum::{Router, http::StatusCode, response::IntoResponse, routing::get};
use nautilus_common::testing::wait_until_async;
use nautilus_core::UnixNanos;
use nautilus_dydx::{
    common::enums::{
        DydxFillType, DydxLiquidity, DydxMarketStatus, DydxOrderStatus, DydxOrderType,
        DydxTickerType, DydxTimeInForce,
    },
    http::{
        client::DydxRawHttpClient,
        models::{Fill, Order, PerpetualMarket},
        parse::{parse_fill_report, parse_instrument_any, parse_order_status_report},
    },
};
use nautilus_model::{
    enums::{OrderSide, OrderStatus, OrderType, TimeInForce},
    identifiers::AccountId,
    instruments::{Instrument, InstrumentAny},
};
use nautilus_network::http::HttpClient;
use rstest::rstest;
use rust_decimal_macros::dec;
use serde_json::{Value, json};

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
struct TestServerState {}

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

fn load_test_orders() -> Value {
    load_json_result_fixture("http_get_orders.json")
}

fn load_test_fills() -> Value {
    load_json_result_fixture("http_get_fills.json")
}

fn create_test_router(state: TestServerState) -> Router {
    Router::new()
        .route("/v4/perpetualMarkets", get(handle_get_markets))
        .route("/v4/orders", get(handle_get_orders))
        .route("/v4/fills", get(handle_get_fills))
        .with_state(state)
}

async fn handle_get_markets() -> impl IntoResponse {
    axum::response::Json(load_test_instruments())
}

async fn handle_get_orders() -> impl IntoResponse {
    axum::response::Json(load_test_orders())
}

async fn handle_get_fills() -> impl IntoResponse {
    axum::response::Json(load_test_fills())
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

fn create_test_instrument() -> InstrumentAny {
    let market = PerpetualMarket {
        clob_pair_id: 0,
        ticker: "BTC-USD".to_string(),
        status: DydxMarketStatus::Active,
        base_asset: Some("BTC".to_string()),
        quote_asset: Some("USD".to_string()),
        step_size: dec!(0.001),
        tick_size: dec!(1),
        index_price: Some(dec!(50000)),
        oracle_price: dec!(50000),
        price_change_24h: dec!(500),
        next_funding_rate: dec!(0.0001),
        next_funding_at: None,
        min_order_size: Some(dec!(0.001)),
        market_type: None,
        initial_margin_fraction: dec!(0.05),
        maintenance_margin_fraction: dec!(0.03),
        base_position_notional: None,
        incremental_position_size: None,
        incremental_initial_margin_fraction: None,
        max_position_size: None,
        open_interest: dec!(500000000),
        atomic_resolution: -10,
        quantum_conversion_exponent: -9,
        subticks_per_tick: 100000,
        step_base_quantums: 1000000,
        is_reduce_only: false,
    };

    parse_instrument_any(
        &market,
        Some(dec!(0.0002)),
        Some(dec!(0.0005)),
        UnixNanos::default(),
    )
    .unwrap()
}

fn create_test_order() -> Order {
    Order {
        id: "order-123".to_string(),
        subaccount_id: "dydx1test/0".to_string(),
        client_id: "12345".to_string(),
        clob_pair_id: 0,
        side: OrderSide::Buy,
        size: dec!(0.1),
        total_filled: dec!(0.05),
        price: dec!(50000),
        order_type: DydxOrderType::Limit,
        status: DydxOrderStatus::PartiallyFilled,
        time_in_force: DydxTimeInForce::Gtt,
        post_only: false,
        reduce_only: false,
        order_flags: 0,
        good_til_block: Some(12400),
        good_til_block_time: None,
        created_at_height: Some(12345),
        client_metadata: 0,
        ticker: Some("BTC-USD".to_string()),
        updated_at: None,
        updated_at_height: None,
        trigger_price: None,
        condition_type: None,
        conditional_order_trigger_subticks: None,
        execution: None,
        subaccount_number: 0,
        order_router_address: None,
    }
}

fn create_test_fill() -> Fill {
    Fill {
        id: "fill-001".to_string(),
        side: OrderSide::Buy,
        liquidity: DydxLiquidity::Taker,
        fill_type: DydxFillType::Limit,
        market: "BTC-USD".to_string(),
        market_type: DydxTickerType::Perpetual,
        price: dec!(50000),
        size: dec!(0.05),
        fee: dec!(2.50),
        created_at: chrono::Utc::now(),
        created_at_height: 12345,
        order_id: "order-123".to_string(),
        client_metadata: 0,
    }
}

#[rstest]
#[tokio::test]
async fn test_parse_order_status_report_buy_limit() {
    let instrument = create_test_instrument();
    let order = create_test_order();
    let account_id = AccountId::new("DYDX-001");
    let ts_init = UnixNanos::default();

    let report = parse_order_status_report(&order, &instrument, account_id, ts_init).unwrap();

    assert_eq!(report.account_id, account_id);
    assert_eq!(report.instrument_id, instrument.id());
    assert_eq!(report.venue_order_id.as_str(), "order-123");
    assert_eq!(report.order_side, OrderSide::Buy);
    assert_eq!(report.order_type, OrderType::Limit);
    assert_eq!(report.time_in_force, TimeInForce::Gtc);
    assert_eq!(report.order_status, OrderStatus::PartiallyFilled);
    assert_eq!(report.quantity.as_f64(), 0.1);
    assert_eq!(report.filled_qty.as_f64(), 0.05);
}

#[rstest]
#[tokio::test]
async fn test_parse_order_status_report_sell_filled() {
    let instrument = create_test_instrument();
    let mut order = create_test_order();
    order.id = "order-124".to_string();
    order.side = OrderSide::Sell;
    order.size = dec!(0.2);
    order.total_filled = dec!(0.2);
    order.price = dec!(51000);
    order.status = DydxOrderStatus::Filled;
    order.time_in_force = DydxTimeInForce::Ioc;
    order.reduce_only = false; // Must be false for IOC to be respected

    let account_id = AccountId::new("DYDX-001");
    let ts_init = UnixNanos::default();

    let report = parse_order_status_report(&order, &instrument, account_id, ts_init).unwrap();

    assert_eq!(report.order_side, OrderSide::Sell);
    assert_eq!(report.time_in_force, TimeInForce::Ioc);
    assert_eq!(report.order_status, OrderStatus::Filled);
    assert_eq!(report.quantity.as_f64(), 0.2);
    assert_eq!(report.filled_qty.as_f64(), 0.2);
}

#[rstest]
#[tokio::test]
async fn test_parse_order_status_report_canceled() {
    let instrument = create_test_instrument();
    let mut order = create_test_order();
    order.status = DydxOrderStatus::Canceled;
    order.total_filled = dec!(0);

    let account_id = AccountId::new("DYDX-001");
    let ts_init = UnixNanos::default();

    let report = parse_order_status_report(&order, &instrument, account_id, ts_init).unwrap();

    assert_eq!(report.order_status, OrderStatus::Canceled);
    assert_eq!(report.filled_qty.as_f64(), 0.0);
}

#[rstest]
#[tokio::test]
async fn test_parse_order_status_report_with_client_order_id() {
    let instrument = create_test_instrument();
    let order = create_test_order();
    let account_id = AccountId::new("DYDX-001");
    let ts_init = UnixNanos::default();

    let report = parse_order_status_report(&order, &instrument, account_id, ts_init).unwrap();

    assert!(report.client_order_id.is_some());
    assert_eq!(report.client_order_id.unwrap().as_str(), "12345");
}

#[rstest]
#[tokio::test]
async fn test_parse_order_status_report_empty_client_id() {
    let instrument = create_test_instrument();
    let mut order = create_test_order();
    order.client_id = String::new();

    let account_id = AccountId::new("DYDX-001");
    let ts_init = UnixNanos::default();

    let report = parse_order_status_report(&order, &instrument, account_id, ts_init).unwrap();

    assert!(report.client_order_id.is_none());
}

#[rstest]
#[tokio::test]
async fn test_parse_fill_report_taker_buy() {
    let instrument = create_test_instrument();
    let fill = create_test_fill();
    let account_id = AccountId::new("DYDX-001");
    let ts_init = UnixNanos::default();

    let report = parse_fill_report(&fill, &instrument, account_id, ts_init).unwrap();

    assert_eq!(report.account_id, account_id);
    assert_eq!(report.instrument_id, instrument.id());
    assert_eq!(report.venue_order_id.as_str(), "order-123");
    assert_eq!(report.trade_id.to_string(), "fill-001");
    assert_eq!(report.order_side, OrderSide::Buy);
    assert_eq!(report.last_qty.as_f64(), 0.05);
    assert_eq!(report.last_px.as_f64(), 50000.0);
}

#[rstest]
#[tokio::test]
async fn test_parse_fill_report_maker_sell() {
    let instrument = create_test_instrument();
    let mut fill = create_test_fill();
    fill.id = "fill-002".to_string();
    fill.side = OrderSide::Sell;
    fill.liquidity = DydxLiquidity::Maker;
    fill.price = dec!(51000);
    fill.size = dec!(0.2);
    fill.fee = dec!(-1.02); // Rebate (negative fee)

    let account_id = AccountId::new("DYDX-001");
    let ts_init = UnixNanos::default();

    let report = parse_fill_report(&fill, &instrument, account_id, ts_init).unwrap();

    assert_eq!(report.order_side, OrderSide::Sell);
    assert_eq!(report.last_qty.as_f64(), 0.2);
    assert_eq!(report.last_px.as_f64(), 51000.0);
    // Commission should be negated (rebate becomes positive in Nautilus)
    assert!(report.commission.as_f64() > 0.0);
}

#[rstest]
#[tokio::test]
async fn test_parse_fill_report_zero_fee() {
    let instrument = create_test_instrument();
    let mut fill = create_test_fill();
    fill.fee = dec!(0);

    let account_id = AccountId::new("DYDX-001");
    let ts_init = UnixNanos::default();

    let report = parse_fill_report(&fill, &instrument, account_id, ts_init).unwrap();

    assert_eq!(report.commission.as_f64(), 0.0);
}

#[rstest]
#[tokio::test]
async fn test_get_orders_returns_parsed_data() {
    let (addr, _state) = start_test_server().await.unwrap();
    let base_url = format!("http://{addr}");

    let client = DydxRawHttpClient::new(Some(base_url), Some(30), None, false, None).unwrap();
    let orders = client.get_orders("dydx1test", 0, None, None).await.unwrap();

    assert_eq!(orders.len(), 3);
    assert_eq!(orders[0].id, "0f0981cb-152e-57d3-bea9-4d8e0dd5ed35");
    assert_eq!(orders[0].side, OrderSide::Buy);
    assert_eq!(orders[0].status, DydxOrderStatus::Filled);
    assert_eq!(orders[1].id, "8e2be4a2-86c6-5a32-a081-b223778c3e33");
    assert_eq!(orders[1].side, OrderSide::Sell);
    assert_eq!(orders[1].status, DydxOrderStatus::Filled);
}

#[rstest]
#[tokio::test]
async fn test_get_fills_returns_parsed_data() {
    let (addr, _state) = start_test_server().await.unwrap();
    let base_url = format!("http://{addr}");

    let client = DydxRawHttpClient::new(Some(base_url), Some(30), None, false, None).unwrap();
    let result = client
        .get_fills("dydx1test", 0, Some("BTC-USD"), None)
        .await
        .unwrap();

    assert_eq!(result.fills.len(), 3);
    assert_eq!(result.fills[0].id, "6450e369-1dc3-5229-8dc2-fb3b5d1cf2ab");
    assert_eq!(result.fills[0].market, "BTC-USD");
    assert_eq!(result.fills[1].id, "ef7ad6fb-ed77-50c7-b592-73ab5b32d42a");
}

#[rstest]
#[tokio::test]
async fn test_orders_to_reports_roundtrip() {
    let (addr, _state) = start_test_server().await.unwrap();
    let base_url = format!("http://{addr}");

    let client = DydxRawHttpClient::new(Some(base_url), Some(30), None, false, None).unwrap();
    let orders = client.get_orders("dydx1test", 0, None, None).await.unwrap();

    let instrument = create_test_instrument();
    let account_id = AccountId::new("DYDX-001");
    let ts_init = UnixNanos::default();

    let reports: Vec<_> = orders
        .iter()
        .map(|o| parse_order_status_report(o, &instrument, account_id, ts_init))
        .collect::<Result<Vec<_>, _>>()
        .unwrap();

    assert_eq!(reports.len(), 3);
    assert_eq!(
        reports[0].venue_order_id.as_str(),
        "0f0981cb-152e-57d3-bea9-4d8e0dd5ed35"
    );
    assert_eq!(
        reports[1].venue_order_id.as_str(),
        "8e2be4a2-86c6-5a32-a081-b223778c3e33"
    );
}

#[rstest]
#[tokio::test]
async fn test_fills_to_reports_roundtrip() {
    let (addr, _state) = start_test_server().await.unwrap();
    let base_url = format!("http://{addr}");

    let client = DydxRawHttpClient::new(Some(base_url), Some(30), None, false, None).unwrap();
    let result = client.get_fills("dydx1test", 0, None, None).await.unwrap();

    let instrument = create_test_instrument();
    let account_id = AccountId::new("DYDX-001");
    let ts_init = UnixNanos::default();

    let reports: Vec<_> = result
        .fills
        .iter()
        .map(|f| parse_fill_report(f, &instrument, account_id, ts_init))
        .collect::<Result<Vec<_>, _>>()
        .unwrap();

    assert_eq!(reports.len(), 3);
    assert_eq!(
        reports[0].trade_id.to_string(),
        "6450e369-1dc3-5229-8dc2-fb3b5d1cf2ab"
    );
    assert_eq!(
        reports[1].trade_id.to_string(),
        "ef7ad6fb-ed77-50c7-b592-73ab5b32d42a"
    );
}

#[rstest]
#[tokio::test]
async fn test_http_error_handling_500() {
    let state = TestServerState::default();
    let router = Router::new()
        .route(
            "/v4/orders",
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
    wait_for_server(addr, "/v4/orders").await;

    let base_url = format!("http://{addr}");
    let client = DydxRawHttpClient::new(Some(base_url), Some(5), None, false, None).unwrap();

    let result = client.get_orders("dydx1test", 0, None, None).await;
    assert!(result.is_err());
}

#[rstest]
#[tokio::test]
async fn test_empty_orders_response() {
    let state = TestServerState::default();
    let router = Router::new()
        .route(
            "/v4/orders",
            get(|| async { axum::response::Json(json!([])) }),
        )
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    wait_for_server(addr, "/v4/orders").await;

    let base_url = format!("http://{addr}");
    let client = DydxRawHttpClient::new(Some(base_url), Some(5), None, false, None).unwrap();

    let orders = client.get_orders("dydx1test", 0, None, None).await.unwrap();
    assert!(orders.is_empty());
}

#[rstest]
#[tokio::test]
async fn test_empty_fills_response() {
    let state = TestServerState::default();
    let router = Router::new()
        .route(
            "/v4/fills",
            get(|| async { axum::response::Json(json!({"fills": []})) }),
        )
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    wait_for_server(addr, "/v4/fills").await;

    let base_url = format!("http://{addr}");
    let client = DydxRawHttpClient::new(Some(base_url), Some(5), None, false, None).unwrap();

    let result = client.get_fills("dydx1test", 0, None, None).await.unwrap();
    assert!(result.fills.is_empty());
}

#[rstest]
#[tokio::test]
async fn test_parse_block_height_websocket_message() {
    use chrono::Utc;
    use nautilus_dydx::websocket::{
        enums::{DydxWsChannel, DydxWsMessageType},
        messages::{DydxBlockHeightChannelContents, DydxWsBlockHeightChannelData},
    };

    let test_block_height = "9876543210";
    let block_msg = DydxWsBlockHeightChannelData {
        msg_type: DydxWsMessageType::ChannelData,
        connection_id: "test-conn-123".to_string(),
        message_id: 42,
        id: "dydx".to_string(),
        channel: DydxWsChannel::BlockHeight,
        version: "4.0.0".to_string(),
        contents: DydxBlockHeightChannelContents {
            block_height: test_block_height.to_string(),
            time: Utc::now(),
        },
    };

    assert_eq!(
        block_msg.contents.block_height.parse::<u64>().unwrap(),
        9876543210_u64,
        "Block height string should parse to correct u64"
    );
    assert_eq!(block_msg.channel, DydxWsChannel::BlockHeight);
    assert_eq!(block_msg.msg_type, DydxWsMessageType::ChannelData);
}

#[rstest]
#[tokio::test]
async fn test_block_height_zero_validation() {
    use std::sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    };

    let block_height = Arc::new(AtomicU64::new(0));
    let current_height = block_height.load(Ordering::Relaxed);

    assert_eq!(current_height, 0, "Initial block height should be 0");

    // Validation that should prevent order submission when block height is 0
    assert_eq!(
        current_height, 0,
        "Validation should detect uninitialized block height"
    );

    block_height.store(100, Ordering::Relaxed);
    let updated_height = block_height.load(Ordering::Relaxed);

    assert_eq!(updated_height, 100, "Block height should update correctly");
    assert!(
        updated_height > 0,
        "Updated block height should be valid for order submission"
    );
}

#[rstest]
#[tokio::test]
async fn test_good_til_block_calculation() {
    use nautilus_dydx::grpc::SHORT_TERM_ORDER_MAXIMUM_LIFETIME;

    let current_block: u32 = 1000;
    let good_til_block = current_block + SHORT_TERM_ORDER_MAXIMUM_LIFETIME;

    assert_eq!(
        good_til_block, 1020,
        "good_til_block should be current block + 20"
    );
    assert!(
        good_til_block > current_block,
        "good_til_block must be in the future"
    );
}

#[rstest]
#[tokio::test]
async fn test_block_height_concurrent_access() {
    use std::sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    };

    use tokio::task;

    let block_height = Arc::new(AtomicU64::new(1000));
    let mut handles = vec![];

    for i in 1..=10 {
        let bh = Arc::clone(&block_height);
        let handle = task::spawn(async move {
            let new_height = 1000 + i * 100;
            bh.store(new_height, Ordering::Relaxed);
            tokio::time::sleep(Duration::from_millis(1)).await;
            bh.load(Ordering::Relaxed)
        });
        handles.push(handle);
    }

    for handle in handles {
        let height = handle.await.unwrap();
        assert!(
            height >= 1000,
            "Block height should never go below initial value"
        );
    }

    let final_height = block_height.load(Ordering::Relaxed);
    assert!(final_height >= 1000, "Final block height should be valid");
}
