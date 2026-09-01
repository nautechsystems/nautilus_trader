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

//! Integration tests for the Binance Futures HTTP client using a mock server.

use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use axum::{
    Router,
    extract::{RawQuery, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{delete, get, post},
};
use nautilus_binance::{
    common::enums::{
        BinanceEnvironment, BinanceFuturesOrderType, BinanceProductType, BinanceSide,
        BinanceTimeInForce,
    },
    config::BinanceInstrumentProviderConfig,
    futures::http::{
        client::{BinanceFuturesHttpClient, BinanceRawFuturesHttpClient},
        query::{BinanceNewOrderParamsBuilder, BinanceOpenInterestHistParams},
    },
};
use nautilus_common::cache::InstrumentLookupError;
use nautilus_core::time::get_atomic_clock_realtime;
use nautilus_model::{
    data::BarType,
    enums::{AssetClass, MarketStatusAction},
    identifiers::{AccountId, InstrumentId},
    instruments::{Instrument, InstrumentAny},
    types::Quantity,
};
use parking_lot::Mutex;
use rstest::rstest;
use rust_decimal_macros::dec;
use serde_json::json;
use ustr::Ustr;

#[derive(Debug, Clone, Copy)]
enum RequiredInstrumentCachePath {
    Trades,
    Bars,
}

#[derive(Clone)]
struct TestServerState {
    request_count: Arc<AtomicUsize>,
    rate_limit_threshold: usize,
    last_query: Arc<Mutex<Option<String>>>,
    exchange_info: Option<Arc<serde_json::Value>>,
}

impl Default for TestServerState {
    fn default() -> Self {
        Self {
            request_count: Arc::new(AtomicUsize::new(0)),
            rate_limit_threshold: usize::MAX,
            last_query: Arc::new(Mutex::new(None)),
            exchange_info: None,
        }
    }
}

impl TestServerState {
    fn with_rate_limit(mut self, limit: usize) -> Self {
        self.rate_limit_threshold = limit;
        self
    }

    fn with_exchange_info(mut self, exchange_info: serde_json::Value) -> Self {
        self.exchange_info = Some(Arc::new(exchange_info));
        self
    }

    fn increment_and_check(&self) -> bool {
        self.request_count.fetch_add(1, Ordering::Relaxed) >= self.rate_limit_threshold
    }
}

fn has_auth_headers(headers: &HeaderMap) -> bool {
    headers.contains_key("x-mbx-apikey")
}

fn json_response(body: &serde_json::Value) -> Response {
    (
        StatusCode::OK,
        [("content-type", "application/json")],
        body.to_string(),
    )
        .into_response()
}

fn unauthorized_response() -> Response {
    (
        StatusCode::UNAUTHORIZED,
        [("content-type", "application/json")],
        json!({"code": -2015, "msg": "Invalid API-key, IP, or permissions for action"}).to_string(),
    )
        .into_response()
}

fn rate_limit_response() -> Response {
    (
        StatusCode::TOO_MANY_REQUESTS,
        [("content-type", "application/json")],
        json!({"code": -1015, "msg": "Too many requests"}).to_string(),
    )
        .into_response()
}

fn load_fixture(name: &str) -> serde_json::Value {
    let path = format!(
        "{}/test_data/futures/http_json/{name}",
        env!("CARGO_MANIFEST_DIR")
    );
    let content = std::fs::read_to_string(&path).expect("Failed to read fixture");
    serde_json::from_str(&content).expect("Failed to parse fixture JSON")
}

async fn handle_ping() -> Response {
    json_response(&json!({}))
}

async fn handle_time() -> Response {
    json_response(&json!({"serverTime": 1700000000000_i64}))
}

async fn handle_exchange_info(State(state): State<TestServerState>) -> Response {
    if let Some(exchange_info) = state.exchange_info.as_deref() {
        json_response(exchange_info)
    } else {
        json_response(&load_fixture("exchange_info_usdm.json"))
    }
}

async fn handle_coinm_exchange_info(State(state): State<TestServerState>) -> Response {
    if let Some(exchange_info) = state.exchange_info.as_deref() {
        json_response(exchange_info)
    } else {
        json_response(&load_fixture("exchange_info_delivery_coinm.json"))
    }
}

async fn handle_depth() -> Response {
    json_response(&json!({
        "lastUpdateId": 1027024,
        "E": 1700000000000_i64,
        "T": 1700000000000_i64,
        "bids": [["50000.00", "1.000"], ["49999.00", "2.000"]],
        "asks": [["50001.00", "0.500"], ["50002.00", "1.500"]]
    }))
}

async fn handle_account(
    headers: HeaderMap,
    RawQuery(query): RawQuery,
    State(state): State<TestServerState>,
) -> Response {
    if !has_auth_headers(&headers) {
        return unauthorized_response();
    }

    if state.increment_and_check() {
        return rate_limit_response();
    }
    *state.last_query.lock() = query;
    json_response(&load_fixture("account_info_v2.json"))
}

async fn handle_commission_rate(
    headers: HeaderMap,
    State(state): State<TestServerState>,
) -> Response {
    if !has_auth_headers(&headers) {
        return unauthorized_response();
    }

    if state.increment_and_check() {
        return rate_limit_response();
    }
    json_response(&json!({
        "symbol": "BTCUSDT",
        "makerCommissionRate": "0.000123",
        "takerCommissionRate": "0.000456"
    }))
}

async fn handle_balance(headers: HeaderMap, State(state): State<TestServerState>) -> Response {
    if !has_auth_headers(&headers) {
        return unauthorized_response();
    }

    if state.increment_and_check() {
        return rate_limit_response();
    }
    json_response(&load_fixture("balance.json"))
}

async fn handle_position_risk(
    headers: HeaderMap,
    State(state): State<TestServerState>,
) -> Response {
    if !has_auth_headers(&headers) {
        return unauthorized_response();
    }

    if state.increment_and_check() {
        return rate_limit_response();
    }
    json_response(&load_fixture("position_risk.json"))
}

async fn handle_order_post(headers: HeaderMap, State(state): State<TestServerState>) -> Response {
    if !has_auth_headers(&headers) {
        return unauthorized_response();
    }

    if state.increment_and_check() {
        return rate_limit_response();
    }
    json_response(&load_fixture("order_response.json"))
}

async fn handle_order_get(headers: HeaderMap, State(state): State<TestServerState>) -> Response {
    if !has_auth_headers(&headers) {
        return unauthorized_response();
    }

    if state.increment_and_check() {
        return rate_limit_response();
    }
    json_response(&load_fixture("order_response.json"))
}

async fn handle_order_delete(headers: HeaderMap, State(state): State<TestServerState>) -> Response {
    if !has_auth_headers(&headers) {
        return unauthorized_response();
    }

    if state.increment_and_check() {
        return rate_limit_response();
    }
    json_response(&load_fixture("order_response.json"))
}

async fn handle_open_orders(headers: HeaderMap, State(state): State<TestServerState>) -> Response {
    if !has_auth_headers(&headers) {
        return unauthorized_response();
    }

    if state.increment_and_check() {
        return rate_limit_response();
    }
    let mut order = load_fixture("order_response.json");
    order["symbol"] = json!("BTCUSDT_260925");
    json_response(&json!([order]))
}

async fn handle_coinm_open_orders(
    headers: HeaderMap,
    State(state): State<TestServerState>,
) -> Response {
    if !has_auth_headers(&headers) {
        return unauthorized_response();
    }

    if state.increment_and_check() {
        return rate_limit_response();
    }
    let mut order = load_fixture("order_response.json");
    order["symbol"] = json!("BTCUSD_260925");
    json_response(&json!([order]))
}

async fn handle_cancel_all(headers: HeaderMap, State(state): State<TestServerState>) -> Response {
    if !has_auth_headers(&headers) {
        return unauthorized_response();
    }

    if state.increment_and_check() {
        return rate_limit_response();
    }
    json_response(&json!({"code": 200, "msg": "The operation of cancel all open order is done."}))
}

async fn handle_listen_key_post(
    headers: HeaderMap,
    State(state): State<TestServerState>,
) -> Response {
    if !has_auth_headers(&headers) {
        return unauthorized_response();
    }

    if state.increment_and_check() {
        return rate_limit_response();
    }
    json_response(&json!({"listenKey": "test-listen-key-12345"}))
}

async fn handle_listen_key_put(headers: HeaderMap) -> Response {
    if !has_auth_headers(&headers) {
        return unauthorized_response();
    }
    json_response(&json!({}))
}

async fn handle_hedge_mode(headers: HeaderMap) -> Response {
    if !has_auth_headers(&headers) {
        return unauthorized_response();
    }
    json_response(&json!({"dualSidePosition": false}))
}

async fn handle_user_trades(headers: HeaderMap, State(state): State<TestServerState>) -> Response {
    if !has_auth_headers(&headers) {
        return unauthorized_response();
    }

    if state.increment_and_check() {
        return rate_limit_response();
    }
    json_response(&json!([]))
}

async fn handle_all_orders(headers: HeaderMap, State(state): State<TestServerState>) -> Response {
    if !has_auth_headers(&headers) {
        return unauthorized_response();
    }

    if state.increment_and_check() {
        return rate_limit_response();
    }
    json_response(&json!([]))
}

async fn handle_open_interest_hist(raw_query: RawQuery) -> Response {
    let query = raw_query.0.unwrap_or_default();
    let params: HashMap<String, String> = serde_urlencoded::from_str(&query).unwrap_or_default();

    if params
        .get("symbol")
        .is_some_and(|symbol| symbol == "BTCUSDT")
        && params.get("period").is_some_and(|period| period == "5m")
    {
        return json_response(&json!([
            {
                "symbol": "BTCUSDT",
                "sumOpenInterest": "100.0",
                "sumOpenInterestValue": "1000.0",
                "timestamp": 1700000000000_i64
            }
        ]));
    }

    (
        StatusCode::BAD_REQUEST,
        [("content-type", "application/json")],
        json!({"code": -1102, "msg": "Unexpected params"}).to_string(),
    )
        .into_response()
}

fn create_router(state: TestServerState) -> Router {
    Router::new()
        .route("/fapi/v1/ping", get(handle_ping))
        .route("/fapi/v1/time", get(handle_time))
        .route("/fapi/v1/exchangeInfo", get(handle_exchange_info))
        .route("/dapi/v1/exchangeInfo", get(handle_coinm_exchange_info))
        .route("/fapi/v1/depth", get(handle_depth))
        .route(
            "/futures/data/openInterestHist",
            get(handle_open_interest_hist),
        )
        .route("/fapi/v2/account", get(handle_account))
        .route("/fapi/v1/commissionRate", get(handle_commission_rate))
        .route("/fapi/v2/balance", get(handle_balance))
        .route("/fapi/v2/positionRisk", get(handle_position_risk))
        .route(
            "/fapi/v1/order",
            post(handle_order_post)
                .get(handle_order_get)
                .delete(handle_order_delete)
                .put(handle_order_post),
        )
        .route("/fapi/v1/openOrders", get(handle_open_orders))
        .route("/dapi/v1/openOrders", get(handle_coinm_open_orders))
        .route("/fapi/v1/allOrders", get(handle_all_orders))
        .route("/fapi/v1/allOpenOrders", delete(handle_cancel_all))
        .route(
            "/fapi/v1/listenKey",
            post(handle_listen_key_post).put(handle_listen_key_put),
        )
        .route("/fapi/v1/positionSide/dual", get(handle_hedge_mode))
        .route("/fapi/v1/userTrades", get(handle_user_trades))
        .with_state(state)
}

async fn start_test_server(
    state: TestServerState,
) -> Result<SocketAddr, Box<dyn std::error::Error + Send + Sync>> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let router = create_router(state);

    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });

    tokio::time::sleep(Duration::from_millis(100)).await;
    Ok(addr)
}

fn create_raw_client(
    addr: &SocketAddr,
    api_key: Option<String>,
    api_secret: Option<String>,
) -> BinanceRawFuturesHttpClient {
    let base_url = format!("http://{addr}");
    BinanceRawFuturesHttpClient::new(
        BinanceProductType::UsdM,
        BinanceEnvironment::Live,
        api_key,
        api_secret,
        Some(base_url),
        None,
        Some(60),
        None,
    )
    .unwrap()
}

fn create_domain_client(
    addr: &SocketAddr,
    product_type: BinanceProductType,
) -> BinanceFuturesHttpClient {
    BinanceFuturesHttpClient::new(
        product_type,
        BinanceEnvironment::Live,
        get_atomic_clock_realtime(),
        None,
        None,
        Some(format!("http://{addr}")),
        None,
        Some(60),
        None,
        false,
    )
    .unwrap()
}

#[rstest]
#[case::trades(RequiredInstrumentCachePath::Trades)]
#[case::bars(RequiredInstrumentCachePath::Bars)]
#[tokio::test]
async fn test_public_market_data_request_missing_cached_instrument_returns_lookup_error(
    #[case] path: RequiredInstrumentCachePath,
) {
    let client = BinanceFuturesHttpClient::new(
        BinanceProductType::UsdM,
        BinanceEnvironment::Live,
        get_atomic_clock_realtime(),
        None,
        None,
        Some("http://127.0.0.1:9".to_string()),
        None,
        Some(1),
        None,
        false,
    )
    .unwrap();
    let instrument_id = InstrumentId::from("BTCUSDT-PERP.BINANCE");

    let result = match path {
        RequiredInstrumentCachePath::Trades => {
            client.request_trades(instrument_id, None).await.map(|_| ())
        }
        RequiredInstrumentCachePath::Bars => {
            let bar_type = BarType::from("BTCUSDT-PERP.BINANCE-1-MINUTE-LAST-EXTERNAL");
            client
                .request_bars(bar_type, None, None, None)
                .await
                .map(|_| ())
        }
    };

    assert_eq!(
        result.unwrap_err().to_string(),
        InstrumentLookupError::not_found(instrument_id).to_string()
    );
}

#[rstest]
fn test_raw_client_accepts_demo_environment() {
    let result = BinanceRawFuturesHttpClient::new(
        BinanceProductType::UsdM,
        BinanceEnvironment::Demo,
        None,
        None,
        Some("http://127.0.0.1:1".to_string()),
        None,
        Some(60),
        None,
    );

    assert!(result.is_ok());
}

#[rstest]
#[tokio::test]
async fn test_ping() {
    let addr = start_test_server(TestServerState::default()).await.unwrap();
    let client = create_raw_client(&addr, None, None);

    let result: serde_json::Value = client.get("ping", None::<&()>, false, false).await.unwrap();
    assert_eq!(result, json!({}));
}

#[rstest]
#[tokio::test]
async fn test_server_time() {
    let addr = start_test_server(TestServerState::default()).await.unwrap();
    let client = create_raw_client(&addr, None, None);

    let result: serde_json::Value = client.get("time", None::<&()>, false, false).await.unwrap();
    assert_eq!(result["serverTime"], 1700000000000_i64);
}

#[rstest]
#[tokio::test]
async fn test_exchange_info() {
    let addr = start_test_server(TestServerState::default()).await.unwrap();
    let client = create_raw_client(&addr, None, None);

    let result: serde_json::Value = client
        .get("exchangeInfo", None::<&()>, false, false)
        .await
        .unwrap();
    let symbols = result["symbols"].as_array().unwrap();
    assert!(!symbols.is_empty());
    assert_eq!(symbols[0]["symbol"], "BTCUSDT");
}

#[rstest]
#[case::usdm(
    BinanceProductType::UsdM,
    "BTCUSDT_260925",
    "BTCUSDT_260925.BINANCE",
    "USDT",
    false,
    Quantity::from(1)
)]
#[case::coinm(
    BinanceProductType::CoinM,
    "BTCUSD_260925",
    "BTCUSD_260925.BINANCE",
    "BTC",
    true,
    Quantity::from(100)
)]
#[tokio::test]
async fn test_request_delivery_instrument_populates_cache_and_status(
    #[case] product_type: BinanceProductType,
    #[case] raw_symbol: &str,
    #[case] expected_id: &str,
    #[case] settlement_currency: &str,
    #[case] is_inverse: bool,
    #[case] multiplier: Quantity,
) {
    let addr = start_test_server(TestServerState::default()).await.unwrap();
    let client = create_domain_client(&addr, product_type);
    let raw_symbol = ustr::Ustr::from(raw_symbol);

    let instruments = client.request_instruments().await.unwrap();
    let statuses = client.request_symbol_statuses().await.unwrap();
    let instrument = instruments
        .iter()
        .find(|instrument| instrument.raw_symbol().inner() == raw_symbol)
        .unwrap();
    let InstrumentAny::CryptoFuture(future) = instrument else {
        panic!("Expected CryptoFuture, was {instrument:?}");
    };
    let cache = client.instruments_cache();
    let cached = cache
        .get(&raw_symbol)
        .expect("delivery instrument missing from HTTP cache");

    assert_eq!(future.id.to_string(), expected_id);
    assert_eq!(
        future.settlement_currency.code.as_str(),
        settlement_currency
    );
    assert_eq!(future.is_inverse, is_inverse);
    assert_eq!(future.multiplier, multiplier);
    assert_eq!(future.maker_fee, dec!(0.0002));
    assert_eq!(future.taker_fee, dec!(0.0005));
    assert_eq!(cached.id().to_string(), expected_id);
    assert_eq!(
        statuses.get(&raw_symbol),
        Some(&MarketStatusAction::Trading),
    );
}

#[rstest]
#[tokio::test]
async fn test_request_instruments_applies_filters_and_retains_raw_metadata() {
    let addr = start_test_server(TestServerState::default()).await.unwrap();
    let client = BinanceFuturesHttpClient::new(
        BinanceProductType::UsdM,
        BinanceEnvironment::Live,
        get_atomic_clock_realtime(),
        Some("test-key".to_string()),
        Some("test-secret".to_string()),
        Some(format!("http://{addr}")),
        None,
        Some(60),
        None,
        false,
    )
    .unwrap();
    let perpetual_only = BinanceInstrumentProviderConfig {
        filters: HashMap::from([("contract_types".to_string(), json!(["PERPETUAL"]))]),
        query_commission_rates: true,
        ..Default::default()
    };

    let instruments = client
        .request_instruments_with_config(&perpetual_only)
        .await
        .unwrap();

    assert_eq!(instruments.len(), 1);
    assert_eq!(
        instruments[0].id(),
        InstrumentId::from("BTCUSDT-PERP.BINANCE")
    );
    assert_eq!(instruments[0].maker_fee(), dec!(0.000123));
    assert_eq!(instruments[0].taker_fee(), dec!(0.000456));
    assert!(
        client
            .instruments_cache()
            .contains_key(&Ustr::from("BTCUSDT"))
    );
    assert!(
        client
            .instruments_cache()
            .contains_key(&Ustr::from("BTCUSDT_260925"))
    );

    let delivery_only = BinanceInstrumentProviderConfig {
        load_all: false,
        load_ids: Some(vec!["BTCUSDT_260925.BINANCE".to_string()]),
        ..Default::default()
    };
    let refreshed = client
        .request_instruments_with_config(&delivery_only)
        .await
        .unwrap();

    assert_eq!(refreshed.len(), 1);
    assert_eq!(
        refreshed[0].id(),
        InstrumentId::from("BTCUSDT_260925.BINANCE")
    );
    assert!(
        client
            .instruments_cache()
            .contains_key(&Ustr::from("BTCUSDT"))
    );
    assert!(
        client
            .instruments_cache()
            .contains_key(&Ustr::from("BTCUSDT_260925"))
    );
}

#[rstest]
#[case::usdm(
    BinanceProductType::UsdM,
    "exchange_info_usdm.json",
    "status",
    "BTCUSDT",
    "BTCUSDT-PERP.BINANCE",
    2,
    3
)]
#[case::coinm(
    BinanceProductType::CoinM,
    "exchange_info_delivery_coinm.json",
    "contractStatus",
    "BTCUSD_260925",
    "BTCUSD_260925.BINANCE",
    1,
    0
)]
#[tokio::test]
async fn test_non_trading_instrument_retains_raw_precision_metadata(
    #[case] product_type: BinanceProductType,
    #[case] fixture: &str,
    #[case] status_field: &str,
    #[case] raw_symbol: &str,
    #[case] instrument_id: &str,
    #[case] price_precision: u8,
    #[case] size_precision: u8,
) {
    let mut exchange_info = load_fixture(fixture);
    exchange_info["symbols"][0][status_field] = json!("PENDING_TRADING");
    let state = TestServerState::default().with_exchange_info(exchange_info);
    let addr = start_test_server(state).await.unwrap();
    let client = create_domain_client(&addr, product_type);

    let instruments = client.request_instruments().await.unwrap();
    let cache = client.instruments_cache();
    let cached = cache.get(&Ustr::from(raw_symbol)).unwrap();

    assert!(
        instruments
            .iter()
            .all(|instrument| instrument.id() != InstrumentId::from(instrument_id))
    );
    assert_eq!(cached.id(), InstrumentId::from(instrument_id));
    assert_eq!(
        cached.precisions().unwrap(),
        (price_precision, size_precision)
    );
}

#[rstest]
#[tokio::test]
async fn test_request_instruments_parses_tradifi_perpetual_exchange_info() {
    let addr = start_test_server(TestServerState::default()).await.unwrap();
    let client = BinanceFuturesHttpClient::new(
        BinanceProductType::UsdM,
        BinanceEnvironment::Live,
        get_atomic_clock_realtime(),
        None,
        None,
        Some(format!("http://{addr}")),
        None,
        Some(60),
        None,
        false,
    )
    .unwrap();

    let instruments = client.request_instruments().await.unwrap();
    let btc_id = InstrumentId::from("BTCUSDT-PERP.BINANCE");
    let btc = instruments
        .iter()
        .find(|instrument| instrument.id() == btc_id)
        .expect("Missing crypto perpetual instrument");
    assert!(matches!(btc, InstrumentAny::CryptoPerpetual(_)));

    let xau_id = InstrumentId::from("XAUUSDT-PERP.BINANCE");
    let xau = instruments
        .iter()
        .find(|instrument| instrument.id() == xau_id)
        .expect("Missing TradFi perpetual instrument");
    let InstrumentAny::PerpetualContract(tradifi) = xau else {
        panic!("Expected XAU to parse as a PerpetualContract, was {xau:?}");
    };

    assert_eq!(tradifi.id.to_string(), "XAUUSDT-PERP.BINANCE");
    assert_eq!(tradifi.raw_symbol.as_str(), "XAUUSDT");
    assert_eq!(tradifi.underlying.as_str(), "XAU");
    assert_eq!(tradifi.asset_class, AssetClass::Commodity);
    assert_eq!(tradifi.base_currency, None);
    assert_eq!(tradifi.maker_fee, dec!(0.0002));
    assert_eq!(tradifi.taker_fee, dec!(0.0005));
}

#[rstest]
#[tokio::test]
async fn test_depth() {
    let addr = start_test_server(TestServerState::default()).await.unwrap();
    let client = create_raw_client(&addr, None, None);

    let result: serde_json::Value = client
        .get("depth", None::<&()>, false, false)
        .await
        .unwrap();
    assert!(!result["bids"].as_array().unwrap().is_empty());
    assert!(!result["asks"].as_array().unwrap().is_empty());
}

#[rstest]
#[tokio::test]
async fn test_open_interest_hist_public_path() {
    let addr = start_test_server(TestServerState::default()).await.unwrap();
    let client = create_raw_client(&addr, None, None);
    let params = BinanceOpenInterestHistParams {
        symbol: Some("BTCUSDT".to_string()),
        pair: None,
        contract_type: None,
        period: "5m".to_string(),
        start_time: None,
        end_time: None,
        limit: Some(1),
    };

    let result: serde_json::Value = client
        .get(
            "/futures/data/openInterestHist",
            Some(&params),
            false,
            false,
        )
        .await
        .unwrap();

    assert_eq!(result[0]["symbol"], "BTCUSDT");
    assert_eq!(result[0]["sumOpenInterest"], "100.0");
}

#[rstest]
#[tokio::test]
async fn test_account_requires_credentials() {
    let addr = start_test_server(TestServerState::default()).await.unwrap();
    let client = create_raw_client(&addr, None, None);

    let result: Result<serde_json::Value, _> = client
        .get("/fapi/v2/account", None::<&()>, true, false)
        .await;
    result.unwrap_err();
}

#[rstest]
#[tokio::test]
async fn test_account_with_credentials() {
    let addr = start_test_server(TestServerState::default()).await.unwrap();
    let client = create_raw_client(
        &addr,
        Some("test-key".to_string()),
        Some("test-secret".to_string()),
    );

    let result: serde_json::Value = client
        .get("/fapi/v2/account", None::<&()>, true, false)
        .await
        .unwrap();
    assert_eq!(result["canTrade"], true);
}

#[rstest]
#[tokio::test]
async fn test_signed_request_includes_configured_recv_window() {
    let state = TestServerState::default();
    let addr = start_test_server(state.clone()).await.unwrap();
    let client = BinanceRawFuturesHttpClient::new(
        BinanceProductType::UsdM,
        BinanceEnvironment::Live,
        Some("test-key".to_string()),
        Some("test-secret".to_string()),
        Some(format!("http://{addr}")),
        Some(42_000),
        Some(60),
        None,
    )
    .unwrap();

    let _: serde_json::Value = client
        .get("/fapi/v2/account", None::<&()>, true, false)
        .await
        .unwrap();
    let query = state.last_query.lock().clone().unwrap();
    let params = serde_urlencoded::from_str::<HashMap<String, String>>(&query).unwrap();

    assert_eq!(params.get("recvWindow"), Some(&"42000".to_string()));
    assert!(params.contains_key("timestamp"));
    assert!(params.contains_key("signature"));
}

#[rstest]
#[tokio::test]
async fn test_position_risk_with_credentials() {
    let addr = start_test_server(TestServerState::default()).await.unwrap();
    let client = create_raw_client(
        &addr,
        Some("test-key".to_string()),
        Some("test-secret".to_string()),
    );

    let result: serde_json::Value = client
        .get("/fapi/v2/positionRisk", None::<&()>, true, false)
        .await
        .unwrap();
    assert!(result.as_array().is_some());
}

#[rstest]
#[tokio::test]
async fn test_open_orders_with_credentials() {
    let addr = start_test_server(TestServerState::default()).await.unwrap();
    let client = create_raw_client(
        &addr,
        Some("test-key".to_string()),
        Some("test-secret".to_string()),
    );

    let result: serde_json::Value = client
        .get("openOrders", None::<&()>, true, false)
        .await
        .unwrap();
    assert!(result.as_array().is_some());
}

#[rstest]
#[case::usdm(BinanceProductType::UsdM, "BTCUSDT_260925.BINANCE")]
#[case::coinm(BinanceProductType::CoinM, "BTCUSD_260925.BINANCE")]
#[tokio::test]
async fn test_request_delivery_order_status_reports_without_instrument_id(
    #[case] product_type: BinanceProductType,
    #[case] expected_id: &str,
) {
    let addr = start_test_server(TestServerState::default()).await.unwrap();
    let client = BinanceFuturesHttpClient::new(
        product_type,
        BinanceEnvironment::Live,
        get_atomic_clock_realtime(),
        Some("test-key".to_string()),
        Some("test-secret".to_string()),
        Some(format!("http://{addr}")),
        None,
        Some(60),
        None,
        false,
    )
    .unwrap();
    client.request_instruments().await.unwrap();

    let reports = client
        .request_order_status_reports(AccountId::from("BINANCE-001"), None, true)
        .await
        .unwrap();

    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].instrument_id, InstrumentId::from(expected_id));
}

#[rstest]
#[tokio::test]
async fn test_listen_key_creation() {
    let addr = start_test_server(TestServerState::default()).await.unwrap();
    let client = create_raw_client(
        &addr,
        Some("test-key".to_string()),
        Some("test-secret".to_string()),
    );

    let result: serde_json::Value = client
        .post("listenKey", None::<&()>, None, false, false)
        .await
        .unwrap();
    assert_eq!(result["listenKey"], "test-listen-key-12345");
}

#[rstest]
#[tokio::test]
async fn test_hedge_mode_query() {
    let addr = start_test_server(TestServerState::default()).await.unwrap();
    let client = create_raw_client(
        &addr,
        Some("test-key".to_string()),
        Some("test-secret".to_string()),
    );

    let result: serde_json::Value = client
        .get("positionSide/dual", None::<&()>, true, false)
        .await
        .unwrap();
    assert_eq!(result["dualSidePosition"], false);
}

#[rstest]
#[tokio::test]
async fn test_order_submission() {
    let addr = start_test_server(TestServerState::default()).await.unwrap();
    let client = create_raw_client(
        &addr,
        Some("test-key".to_string()),
        Some("test-secret".to_string()),
    );

    let params = BinanceNewOrderParamsBuilder::default()
        .symbol("BTCUSDT")
        .side(BinanceSide::Buy)
        .order_type(BinanceFuturesOrderType::Limit)
        .time_in_force(BinanceTimeInForce::Gtc)
        .quantity("0.001")
        .price("50000.00")
        .build()
        .unwrap();

    let result: serde_json::Value = client
        .post("order", Some(&params), None, true, true)
        .await
        .unwrap();
    assert!(result["orderId"].as_i64().is_some());
}

#[rstest]
#[tokio::test]
async fn test_order_query() {
    let addr = start_test_server(TestServerState::default()).await.unwrap();
    let client = create_raw_client(
        &addr,
        Some("test-key".to_string()),
        Some("test-secret".to_string()),
    );

    let result: serde_json::Value = client.get("order", None::<&()>, true, false).await.unwrap();
    assert!(result["orderId"].as_i64().is_some());
}

#[rstest]
#[tokio::test]
async fn test_order_cancellation() {
    let addr = start_test_server(TestServerState::default()).await.unwrap();
    let client = create_raw_client(
        &addr,
        Some("test-key".to_string()),
        Some("test-secret".to_string()),
    );

    let result: serde_json::Value = client
        .request_delete("order", None::<&()>, true, true)
        .await
        .unwrap();
    assert!(result["orderId"].as_i64().is_some());
}

#[rstest]
#[tokio::test]
async fn test_cancel_all_orders() {
    let addr = start_test_server(TestServerState::default()).await.unwrap();
    let client = create_raw_client(
        &addr,
        Some("test-key".to_string()),
        Some("test-secret".to_string()),
    );

    let result: serde_json::Value = client
        .request_delete("allOpenOrders", None::<&()>, true, false)
        .await
        .unwrap();
    assert_eq!(result["code"], 200);
}

#[rstest]
#[tokio::test]
async fn test_all_orders_history() {
    let addr = start_test_server(TestServerState::default()).await.unwrap();
    let client = create_raw_client(
        &addr,
        Some("test-key".to_string()),
        Some("test-secret".to_string()),
    );

    let result: serde_json::Value = client
        .get("allOrders", None::<&()>, true, false)
        .await
        .unwrap();
    assert!(result.as_array().is_some());
}

#[rstest]
#[tokio::test]
async fn test_user_trades() {
    let addr = start_test_server(TestServerState::default()).await.unwrap();
    let client = create_raw_client(
        &addr,
        Some("test-key".to_string()),
        Some("test-secret".to_string()),
    );

    let result: serde_json::Value = client
        .get("userTrades", None::<&()>, true, false)
        .await
        .unwrap();
    assert!(result.as_array().is_some());
}

#[rstest]
#[tokio::test]
async fn test_rate_limit_triggers() {
    let state = TestServerState::default().with_rate_limit(0);
    let addr = start_test_server(state).await.unwrap();
    let client = create_raw_client(
        &addr,
        Some("test-key".to_string()),
        Some("test-secret".to_string()),
    );

    let result: Result<serde_json::Value, _> = client
        .get("/fapi/v2/account", None::<&()>, true, false)
        .await;
    result.unwrap_err();
}
