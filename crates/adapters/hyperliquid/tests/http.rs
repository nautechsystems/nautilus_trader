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

//! Integration tests for Hyperliquid HTTP client using a mock server.

use std::{
    collections::HashMap,
    net::SocketAddr,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use axum::{
    Router,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Json, Response},
    routing::post,
};
use nautilus_common::testing::wait_until_async;
use nautilus_hyperliquid::{
    HyperliquidHttpClient,
    common::enums::{HyperliquidEnvironment, HyperliquidInfoRequestType},
    http::{
        models::{
            Cloid, HyperliquidFills, HyperliquidL2Book, PerpMeta, PerpMetaAndCtxs, SpotMeta,
            SpotMetaAndCtxs,
        },
        query::{InfoRequest, InfoRequestParams},
    },
};
use nautilus_model::{
    enums::{OrderStatus, PositionSideSpecified},
    identifiers::{AccountId, ClientOrderId},
};
use nautilus_network::http::{HttpClient, Method};
use rstest::rstest;
use serde_json::{Value, json};

#[derive(Clone)]
struct TestServerState {
    request_count: Arc<tokio::sync::Mutex<usize>>,
    last_request_body: Arc<tokio::sync::Mutex<Option<Value>>>,
    rate_limit_after: Arc<AtomicUsize>,
    frontend_open_orders_response: Arc<tokio::sync::Mutex<Option<Value>>>,
    order_status_response: Arc<tokio::sync::Mutex<Option<Value>>>,
    clearinghouse_response: Arc<tokio::sync::Mutex<Option<Value>>>,
    spot_fails: Arc<std::sync::atomic::AtomicBool>,
}

impl Default for TestServerState {
    fn default() -> Self {
        Self {
            request_count: Arc::new(tokio::sync::Mutex::new(0)),
            last_request_body: Arc::new(tokio::sync::Mutex::new(None)),
            rate_limit_after: Arc::new(AtomicUsize::new(usize::MAX)),
            frontend_open_orders_response: Arc::new(tokio::sync::Mutex::new(None)),
            order_status_response: Arc::new(tokio::sync::Mutex::new(None)),
            clearinghouse_response: Arc::new(tokio::sync::Mutex::new(None)),
            spot_fails: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }
}

fn data_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test_data")
}

fn load_json(filename: &str) -> Value {
    let content = std::fs::read_to_string(data_path().join(filename))
        .unwrap_or_else(|_| panic!("failed to read {filename}"));
    serde_json::from_str(&content).expect("invalid json")
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

async fn handle_info(State(state): State<TestServerState>, body: axum::body::Bytes) -> Response {
    let mut count = state.request_count.lock().await;
    *count += 1;

    let limit_after = state.rate_limit_after.load(Ordering::Relaxed);
    if *count > limit_after {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            Json(json!({
                "error": "Rate limit exceeded"
            })),
        )
            .into_response();
    }

    let Ok(request_body): Result<Value, _> = serde_json::from_slice(&body) else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "Invalid JSON body"
            })),
        )
            .into_response();
    };

    *state.last_request_body.lock().await = Some(request_body.clone());

    let request_type = request_body
        .get("type")
        .and_then(|t| t.as_str())
        .unwrap_or("");

    match request_type {
        "meta" => {
            let meta = load_json("http_meta_perp_sample.json");
            Json(meta).into_response()
        }
        "allPerpMetas" => {
            let meta = load_json("http_meta_perp_sample.json");
            Json(json!([meta])).into_response()
        }
        "spotMeta" => Json(json!({
            "universe": [],
            "tokens": []
        }))
        .into_response(),
        "metaAndAssetCtxs" => {
            let meta = load_json("http_meta_perp_sample.json");
            Json(json!([meta, []])).into_response()
        }
        "spotMetaAndAssetCtxs" => Json(json!([
            {"universe": [], "tokens": []},
            []
        ]))
        .into_response(),
        "l2Book" => {
            let book = load_json("http_l2_book_btc.json");
            Json(book).into_response()
        }
        "userFills" => Json(json!([])).into_response(),
        "orderStatus" => {
            let custom = state.order_status_response.lock().await;
            Json(custom.clone().unwrap_or(json!({"statuses": []}))).into_response()
        }
        "openOrders" => Json(json!([])).into_response(),
        "frontendOpenOrders" => {
            let custom = state.frontend_open_orders_response.lock().await;
            Json(custom.clone().unwrap_or(json!([]))).into_response()
        }
        "clearinghouseState" => {
            let custom = state.clearinghouse_response.lock().await;
            let body = custom.clone().unwrap_or_else(|| {
                json!({
                    "marginSummary": {
                        "accountValue": "10000.0",
                        "totalMarginUsed": "0.0",
                        "totalNtlPos": "0.0",
                        "totalRawUsd": "10000.0"
                    },
                    "crossMarginSummary": {
                        "accountValue": "10000.0",
                        "totalMarginUsed": "0.0",
                        "totalNtlPos": "0.0",
                        "totalRawUsd": "10000.0"
                    },
                    "crossMaintenanceMarginUsed": "0.0",
                    "withdrawable": "10000.0",
                    "assetPositions": []
                })
            });
            Json(body).into_response()
        }
        "spotClearinghouseState" => {
            if state.spot_fails.load(Ordering::Relaxed) {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({"error": "spot endpoint unavailable"})),
                )
                    .into_response();
            }
            let spot = load_json("http_spot_clearinghouse_state.json");
            Json(spot).into_response()
        }
        "candleSnapshot" => Json(json!([
            {
                "t": 1703875200000u64,
                "T": 1703875260000u64,
                "s": "BTC",
                "i": "1m",
                "o": "98450.00",
                "c": "98460.00",
                "h": "98470.00",
                "l": "98440.00",
                "v": "100.5",
                "n": 50
            }
        ]))
        .into_response(),
        _ => (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": format!("Unknown request type: {}", request_type)
            })),
        )
            .into_response(),
    }
}

async fn handle_exchange(
    State(state): State<TestServerState>,
    body: axum::body::Bytes,
) -> Response {
    let mut count = state.request_count.lock().await;
    *count += 1;

    let Ok(request_body): Result<Value, _> = serde_json::from_slice(&body) else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "status": "err",
                "response": {
                    "type": "error",
                    "data": "Invalid JSON body"
                }
            })),
        )
            .into_response();
    };

    *state.last_request_body.lock().await = Some(request_body.clone());

    // Validate signed request format
    if request_body.get("action").is_none()
        || request_body.get("nonce").is_none()
        || request_body.get("signature").is_none()
    {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "status": "err",
                "response": {
                    "type": "error",
                    "data": "Missing required fields"
                }
            })),
        )
            .into_response();
    }

    Json(json!({
        "status": "ok",
        "response": {
            "type": "order",
            "data": {
                "statuses": [{
                    "resting": {
                        "oid": 12345
                    }
                }]
            }
        }
    }))
    .into_response()
}

async fn handle_health() -> impl IntoResponse {
    StatusCode::OK
}

fn create_test_router(state: TestServerState) -> Router {
    Router::new()
        .route("/info", post(handle_info))
        .route("/exchange", post(handle_exchange))
        .route("/health", axum::routing::get(handle_health))
        .with_state(state)
}

async fn start_mock_server(state: TestServerState) -> SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let router = create_test_router(state);

    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });

    wait_for_server(addr, "/health").await;
    addr
}

#[rstest]
#[tokio::test]
async fn test_info_meta_returns_market_metadata() {
    let state = TestServerState::default();
    let addr = start_mock_server(state.clone()).await;

    let client = create_test_client(&addr);
    let result = client.info_meta().await;

    assert!(result.is_ok());
    let meta = result.unwrap();
    assert!(!meta.universe.is_empty());
    assert_eq!(meta.universe[0].name, "BTC");
}

#[rstest]
#[tokio::test]
async fn test_info_l2_book_returns_orderbook() {
    let state = TestServerState::default();
    let addr = start_mock_server(state.clone()).await;

    let client = create_test_client(&addr);
    let result = client.info_l2_book("BTC").await;

    assert!(result.is_ok());
    let book = result.unwrap();
    assert_eq!(book.coin, "BTC");
    assert_eq!(book.levels.len(), 2); // bids and asks
}

#[rstest]
#[tokio::test]
async fn test_spot_meta_returns_spot_metadata() {
    let state = TestServerState::default();
    let addr = start_mock_server(state.clone()).await;

    let client = create_test_client(&addr);
    let meta = client.get_spot_meta().await.unwrap();

    assert!(meta.tokens.is_empty());
    assert!(meta.universe.is_empty());
}

#[rstest]
#[tokio::test]
async fn test_perp_meta_and_ctxs_returns_metadata_with_contexts() {
    let state = TestServerState::default();
    let addr = start_mock_server(state.clone()).await;

    let client = create_test_client(&addr);
    let PerpMetaAndCtxs::Payload(data) = client.get_perp_meta_and_ctxs().await.unwrap();

    let (meta, ctxs) = *data;
    assert!(!meta.universe.is_empty());
    assert!(ctxs.is_empty());
}

#[rstest]
#[tokio::test]
async fn test_spot_meta_and_ctxs_returns_metadata_with_contexts() {
    let state = TestServerState::default();
    let addr = start_mock_server(state.clone()).await;

    let client = create_test_client(&addr);
    let SpotMetaAndCtxs::Payload(data) = client.get_spot_meta_and_ctxs().await.unwrap();

    let (meta, ctxs) = *data;
    assert!(meta.tokens.is_empty());
    assert!(meta.universe.is_empty());
    assert!(ctxs.is_empty());
}

#[rstest]
#[tokio::test]
async fn test_info_user_fills_returns_empty_for_new_user() {
    let state = TestServerState::default();
    let addr = start_mock_server(state.clone()).await;

    let client = create_test_client(&addr);
    let result = client
        .info_user_fills("0x1234567890123456789012345678901234567890")
        .await;

    assert!(result.is_ok());
    let fills = result.unwrap();
    assert!(fills.is_empty());
}

#[rstest]
#[tokio::test]
async fn test_info_open_orders_returns_empty_array() {
    let state = TestServerState::default();
    let addr = start_mock_server(state.clone()).await;

    let client = create_test_client(&addr);
    let orders = client
        .info_open_orders("0x1234567890123456789012345678901234567890")
        .await
        .unwrap();

    assert!(orders.is_array());
    assert!(orders.as_array().unwrap().is_empty());
}

#[rstest]
#[tokio::test]
async fn test_info_clearinghouse_state_returns_account_state() {
    let state = TestServerState::default();
    let addr = start_mock_server(state.clone()).await;

    let client = create_test_client(&addr);
    let result = client
        .info_clearinghouse_state("0x1234567890123456789012345678901234567890")
        .await;

    assert!(result.is_ok());
    let state = result.unwrap();
    assert!(state.get("marginSummary").is_some());
}

#[rstest]
#[tokio::test]
async fn test_info_spot_clearinghouse_state_returns_balances() {
    let state = TestServerState::default();
    let addr = start_mock_server(state.clone()).await;

    let client = create_test_client(&addr);
    let result = client
        .info_spot_clearinghouse_state("0x1234567890123456789012345678901234567890")
        .await
        .unwrap();

    let balances = result.get("balances").and_then(|v| v.as_array()).unwrap();
    assert_eq!(balances.len(), 3);
    assert_eq!(balances[0].get("coin").unwrap().as_str().unwrap(), "USDC");

    let last_request = state.last_request_body.lock().await;
    let body = last_request.as_ref().unwrap();
    assert_eq!(
        body.get("type").unwrap().as_str().unwrap(),
        "spotClearinghouseState"
    );
    assert_eq!(
        body.get("user").unwrap().as_str().unwrap(),
        "0x1234567890123456789012345678901234567890"
    );
}

#[rstest]
#[tokio::test]
async fn test_request_account_state_dedupes_usdc_with_perp_summary() {
    let state = TestServerState::default();
    let addr = start_mock_server(state).await;

    let client = create_domain_client(&addr);
    let account_state = client
        .request_account_state("0x1234567890123456789012345678901234567890")
        .await
        .unwrap();

    let usdc_balances: Vec<_> = account_state
        .balances
        .iter()
        .filter(|b| b.currency.code.as_str() == "USDC")
        .collect();
    assert_eq!(usdc_balances.len(), 1, "USDC must not be duplicated");
    assert_eq!(usdc_balances[0].total.as_f64(), 10000.0);

    let non_usdc: Vec<_> = account_state
        .balances
        .iter()
        .filter(|b| b.currency.code.as_str() != "USDC")
        .map(|b| b.currency.code.as_str())
        .collect();
    assert!(non_usdc.contains(&"PURR"));
    assert!(non_usdc.contains(&"HYPE"));
}

#[rstest]
#[tokio::test]
async fn test_request_spot_balances_emits_one_per_non_zero_token() {
    let state = TestServerState::default();
    let addr = start_mock_server(state).await;

    let client = create_domain_client(&addr);
    let balances = client
        .request_spot_balances("0x1234567890123456789012345678901234567890")
        .await
        .unwrap();

    assert_eq!(balances.len(), 3);

    let usdc = balances
        .iter()
        .find(|b| b.currency.code.as_str() == "USDC")
        .expect("USDC balance");
    assert_eq!(usdc.total.as_f64(), 14.625485);
    assert_eq!(usdc.free.as_f64(), 14.625485);

    let purr = balances
        .iter()
        .find(|b| b.currency.code.as_str() == "PURR")
        .expect("PURR balance");
    assert_eq!(purr.total.as_f64(), 2000.0);
    assert_eq!(purr.locked.as_f64(), 100.0);
    assert_eq!(purr.free.as_f64(), 1900.0);
}

#[rstest]
#[tokio::test]
async fn test_request_position_status_reports_skips_spot_fetch_for_perp_filter() {
    let state = TestServerState::default();
    let addr = start_mock_server(state.clone()).await;

    let client = create_domain_client(&addr);
    cache_btc_instrument(&client);

    // Perp filter must not trigger a spotClearinghouseState request
    let reports = client
        .request_position_status_reports(
            "0x1234567890123456789012345678901234567890",
            Some("BTC-USD-PERP.HYPERLIQUID".into()),
        )
        .await
        .unwrap();

    assert!(reports.is_empty());

    let last = state.last_request_body.lock().await;
    let body = last.as_ref().unwrap();
    assert_eq!(
        body.get("type").unwrap().as_str().unwrap(),
        "clearinghouseState",
        "filtered perp query must not reach spotClearinghouseState"
    );
}

#[rstest]
#[tokio::test]
async fn test_request_spot_position_status_reports_skips_when_instrument_missing() {
    let state = TestServerState::default();
    let addr = start_mock_server(state).await;

    let client = create_domain_client(&addr);
    // No spot instruments are cached, so reports are skipped (non-fatal)
    let reports = client
        .request_spot_position_status_reports("0x1234567890123456789012345678901234567890", None)
        .await
        .unwrap();

    assert!(reports.is_empty());
}

#[rstest]
#[tokio::test]
async fn test_request_spot_position_status_reports_emits_for_cached_instrument() {
    use nautilus_model::{
        enums::CurrencyType,
        identifiers::{InstrumentId, Symbol},
        instruments::{CurrencyPair, InstrumentAny},
        types::{Currency, Price, Quantity},
    };

    let state = TestServerState::default();
    let addr = start_mock_server(state).await;

    let client = create_domain_client(&addr);

    let purr = Currency::new("PURR", 8, 0, "PURR", CurrencyType::Crypto);
    let usdc = Currency::new("USDC", 6, 0, "USDC", CurrencyType::Crypto);
    let ts = nautilus_core::time::get_atomic_clock_realtime().get_time_ns();

    let instrument = CurrencyPair::new(
        InstrumentId::from("PURR-USDC-SPOT.HYPERLIQUID"),
        Symbol::new("PURR/USDC"),
        purr,
        usdc,
        5,
        0,
        Price::from("0.00001"),
        Quantity::from("1"),
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        ts,
        ts,
    );
    client.cache_instrument(&InstrumentAny::CurrencyPair(instrument));

    let reports = client
        .request_spot_position_status_reports("0x1234567890123456789012345678901234567890", None)
        .await
        .unwrap();

    assert_eq!(reports.len(), 1);
    let report = &reports[0];
    assert_eq!(
        report.instrument_id,
        InstrumentId::from("PURR-USDC-SPOT.HYPERLIQUID")
    );
    assert_eq!(report.quantity.as_f64(), 2000.0);
    // Spot holdings are always Long on Hyperliquid (no spot shorting)
    assert_eq!(report.position_side, PositionSideSpecified::Long);
    // entryNtl=1234.56 / total=2000 = 0.61728
    assert_eq!(
        report.avg_px_open.unwrap(),
        rust_decimal_macros::dec!(0.61728),
    );
}

#[rstest]
#[tokio::test]
async fn test_request_spot_position_status_reports_skips_usdc() {
    // USDC is the universal spot quote and has no `USDC-*-SPOT` instrument,
    // so the loop must skip it to avoid a misleading cache-miss WARN. Cache a
    // PURR/USDC instrument so the test observes the skip (USDC continues past
    // the early return) while PURR still resolves normally.
    use nautilus_model::{
        enums::CurrencyType,
        identifiers::{InstrumentId, Symbol},
        instruments::{CurrencyPair, InstrumentAny},
        types::{Currency, Price, Quantity},
    };

    let state = TestServerState::default();
    let addr = start_mock_server(state).await;

    let client = create_domain_client(&addr);
    let ts = nautilus_core::time::get_atomic_clock_realtime().get_time_ns();

    let purr = Currency::new("PURR", 8, 0, "PURR", CurrencyType::Crypto);
    let usdc = Currency::new("USDC", 6, 0, "USDC", CurrencyType::Crypto);
    let instrument = CurrencyPair::new(
        InstrumentId::from("PURR-USDC-SPOT.HYPERLIQUID"),
        Symbol::new("PURR/USDC"),
        purr,
        usdc,
        5,
        0,
        Price::from("0.00001"),
        Quantity::from("1"),
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        ts,
        ts,
    );
    client.cache_instrument(&InstrumentAny::CurrencyPair(instrument));

    let reports = client
        .request_spot_position_status_reports("0x1234567890123456789012345678901234567890", None)
        .await
        .unwrap();

    assert_eq!(reports.len(), 1, "only PURR should emit a position report");
    assert!(
        reports[0]
            .instrument_id
            .symbol
            .as_str()
            .starts_with("PURR-")
    );
}

#[rstest]
#[tokio::test]
async fn test_request_spot_position_status_reports_filters_by_instrument_id() {
    use nautilus_model::{
        enums::CurrencyType,
        identifiers::{InstrumentId, Symbol},
        instruments::{CurrencyPair, InstrumentAny},
        types::{Currency, Price, Quantity},
    };

    let state = TestServerState::default();
    let addr = start_mock_server(state).await;

    let client = create_domain_client(&addr);
    let ts = nautilus_core::time::get_atomic_clock_realtime().get_time_ns();
    let usdc = Currency::new("USDC", 6, 0, "USDC", CurrencyType::Crypto);

    // Cache PURR/USDC (fixture contains total=2000)
    let purr = Currency::new("PURR", 8, 0, "PURR", CurrencyType::Crypto);
    let purr_inst = CurrencyPair::new(
        InstrumentId::from("PURR-USDC-SPOT.HYPERLIQUID"),
        Symbol::new("PURR/USDC"),
        purr,
        usdc,
        5,
        0,
        Price::from("0.00001"),
        Quantity::from("1"),
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        ts,
        ts,
    );
    client.cache_instrument(&InstrumentAny::CurrencyPair(purr_inst));

    // Cache HYPE/USDC (fixture contains total=5.2)
    let hype = Currency::new("HYPE", 8, 0, "HYPE", CurrencyType::Crypto);
    let hype_inst = CurrencyPair::new(
        InstrumentId::from("HYPE-USDC-SPOT.HYPERLIQUID"),
        Symbol::new("@150"),
        hype,
        usdc,
        5,
        2,
        Price::from("0.00001"),
        Quantity::from("0.01"),
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        ts,
        ts,
    );
    client.cache_instrument(&InstrumentAny::CurrencyPair(hype_inst));

    let reports = client
        .request_spot_position_status_reports(
            "0x1234567890123456789012345678901234567890",
            Some("PURR-USDC-SPOT.HYPERLIQUID".into()),
        )
        .await
        .unwrap();

    assert_eq!(reports.len(), 1);
    assert_eq!(
        reports[0].instrument_id,
        InstrumentId::from("PURR-USDC-SPOT.HYPERLIQUID")
    );
}

#[rstest]
#[tokio::test]
async fn test_request_account_state_propagates_spot_endpoint_failure() {
    let state = TestServerState::default();
    state
        .spot_fails
        .store(true, std::sync::atomic::Ordering::Relaxed);
    let addr = start_mock_server(state).await;

    let client = create_domain_client(&addr);
    let result = client
        .request_account_state("0x1234567890123456789012345678901234567890")
        .await;

    let err = result.expect_err("spot endpoint failure must propagate");
    let msg = err.to_string().to_lowercase();
    assert!(
        msg.contains("spot") || msg.contains("clearinghouse") || msg.contains("http"),
        "error must reference the failing spot fetch; got: {err}",
    );
}

#[rstest]
#[tokio::test]
async fn test_request_position_status_reports_skips_perp_fetch_for_spot_filter() {
    use nautilus_model::{
        enums::CurrencyType,
        identifiers::{InstrumentId, Symbol},
        instruments::{CurrencyPair, InstrumentAny},
        types::{Currency, Price, Quantity},
    };

    let state = TestServerState::default();
    let addr = start_mock_server(state.clone()).await;

    let client = create_domain_client(&addr);
    let ts = nautilus_core::time::get_atomic_clock_realtime().get_time_ns();

    let purr = Currency::new("PURR", 8, 0, "PURR", CurrencyType::Crypto);
    let usdc = Currency::new("USDC", 6, 0, "USDC", CurrencyType::Crypto);
    let instrument = CurrencyPair::new(
        InstrumentId::from("PURR-USDC-SPOT.HYPERLIQUID"),
        Symbol::new("PURR/USDC"),
        purr,
        usdc,
        5,
        0,
        Price::from("0.00001"),
        Quantity::from("1"),
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        ts,
        ts,
    );
    client.cache_instrument(&InstrumentAny::CurrencyPair(instrument));

    let reports = client
        .request_position_status_reports(
            "0x1234567890123456789012345678901234567890",
            Some("PURR-USDC-SPOT.HYPERLIQUID".into()),
        )
        .await
        .unwrap();

    // PURR is cached and fixture has total=2000, so the spot report emerges
    assert_eq!(reports.len(), 1);
    assert_eq!(
        reports[0].instrument_id,
        InstrumentId::from("PURR-USDC-SPOT.HYPERLIQUID")
    );

    // Last request must be the spot endpoint; perp clearinghouseState
    // must not be called for spot-filtered queries
    let last = state.last_request_body.lock().await;
    let body = last.as_ref().unwrap();
    assert_eq!(
        body.get("type").unwrap().as_str().unwrap(),
        "spotClearinghouseState",
        "spot-filtered query must not reach clearinghouseState"
    );
}

#[rstest]
#[tokio::test]
async fn test_rate_limit_triggers_429_response() {
    let state = TestServerState::default();
    state.rate_limit_after.store(2, Ordering::Relaxed);
    let addr = start_mock_server(state.clone()).await;

    let client = create_test_client(&addr);

    assert!(client.info_meta().await.is_ok());
    assert!(client.info_meta().await.is_ok());

    // Third triggers rate limit
    let result = client.info_meta().await;
    assert!(result.is_err());
}

#[rstest]
#[tokio::test]
async fn test_invalid_request_type_returns_error() {
    let state = TestServerState::default();
    let addr = start_mock_server(state.clone()).await;

    let client = create_test_client(&addr);

    let request = InfoRequest {
        request_type: HyperliquidInfoRequestType::Meta,
        params: InfoRequestParams::None,
    };

    let result = client.send_info_request_raw(&request).await;
    assert!(result.is_ok());
}

#[rstest]
#[tokio::test]
async fn test_l2_book_request_includes_coin_parameter() {
    let state = TestServerState::default();
    let addr = start_mock_server(state.clone()).await;

    let client = create_test_client(&addr);
    let _ = client.info_l2_book("ETH").await;

    let last_request = state.last_request_body.lock().await;
    let request_body = last_request.as_ref().unwrap();

    assert_eq!(
        request_body.get("type").unwrap().as_str().unwrap(),
        "l2Book"
    );
    assert_eq!(request_body.get("coin").unwrap().as_str().unwrap(), "ETH");
}

#[rstest]
#[tokio::test]
async fn test_user_fills_request_includes_user_parameter() {
    let state = TestServerState::default();
    let addr = start_mock_server(state.clone()).await;

    let client = create_test_client(&addr);
    let user = "0xabcdef1234567890abcdef1234567890abcdef12";
    let _ = client.info_user_fills(user).await;

    let last_request = state.last_request_body.lock().await;
    let request_body = last_request.as_ref().unwrap();

    assert_eq!(
        request_body.get("type").unwrap().as_str().unwrap(),
        "userFills"
    );
    assert_eq!(request_body.get("user").unwrap().as_str().unwrap(), user);
}

#[rstest]
#[tokio::test]
async fn test_request_account_state_preserves_parsed_margins() {
    // Regression: the HTTP client previously discarded parsed margins by passing
    // `vec![]` into `AccountState::new`. Verify that a non-zero `totalMarginUsed`
    // surfaces as a USDC account-wide margin on the returned `AccountState`.
    let state = TestServerState::default();
    *state.clearinghouse_response.lock().await = Some(json!({
        "marginSummary": {
            "accountValue": "10000.0",
            "totalMarginUsed": "1250.0",
            "totalNtlPos": "0.0",
            "totalRawUsd": "10000.0"
        },
        "crossMarginSummary": {
            "accountValue": "10000.0",
            "totalMarginUsed": "1250.0",
            "totalNtlPos": "0.0",
            "totalRawUsd": "10000.0"
        },
        "crossMaintenanceMarginUsed": "0.0",
        "withdrawable": "8750.0",
        "assetPositions": []
    }));
    let addr = start_mock_server(state.clone()).await;

    let mut client = HyperliquidHttpClient::new(HyperliquidEnvironment::Mainnet, 60, None)
        .expect("failed to create Hyperliquid HTTP client");
    client.set_base_info_url(format!("http://{addr}/info"));
    client.set_base_exchange_url(format!("http://{addr}/exchange"));
    client.set_account_id(AccountId::new("HYPERLIQUID-001"));

    let account_state = client
        .request_account_state("0x1234567890123456789012345678901234567890")
        .await
        .expect("request_account_state should succeed");

    assert_eq!(
        account_state.margins.len(),
        1,
        "parsed margins must not be discarded by the HTTP client",
    );
    let margin = &account_state.margins[0];
    assert!(
        margin.instrument_id.is_none(),
        "Hyperliquid emits account-wide (cross margin) entries, not per-instrument",
    );
    assert_eq!(margin.currency.code.as_str(), "USDC");
    assert_eq!(margin.initial.as_f64(), 1250.0);
    assert_eq!(margin.maintenance.as_f64(), 1250.0);
}

fn create_test_client(addr: &SocketAddr) -> TestHttpClient {
    TestHttpClient::new(format!("http://{addr}"))
}

struct TestHttpClient {
    client: HttpClient,
    base_url: String,
}

impl TestHttpClient {
    fn new(base_url: String) -> Self {
        let client = HttpClient::new(
            HashMap::from([("Content-Type".to_string(), "application/json".to_string())]),
            vec![],
            vec![],
            None,
            None,
            None,
        )
        .unwrap();

        Self { client, base_url }
    }

    async fn send_info_request(&self, request: &InfoRequest) -> Result<Value, String> {
        let url = format!("{}/info", self.base_url);
        let body = serde_json::to_vec(request).map_err(|e| e.to_string())?;

        let response = self
            .client
            .request(Method::POST, url, None, None, Some(body), None, None)
            .await
            .map_err(|e| e.to_string())?;

        if !response.status.is_success() {
            return Err(format!("HTTP error: {:?}", response.status));
        }

        serde_json::from_slice(&response.body).map_err(|e| e.to_string())
    }

    async fn info_meta(&self) -> Result<PerpMeta, String> {
        let request = InfoRequest::meta();
        let value = self.send_info_request(&request).await?;
        serde_json::from_value(value).map_err(|e| e.to_string())
    }

    async fn get_spot_meta(&self) -> Result<SpotMeta, String> {
        let request = InfoRequest::spot_meta();
        let value = self.send_info_request(&request).await?;
        serde_json::from_value(value).map_err(|e| e.to_string())
    }

    async fn get_perp_meta_and_ctxs(&self) -> Result<PerpMetaAndCtxs, String> {
        let request = InfoRequest::meta_and_asset_ctxs();
        let value = self.send_info_request(&request).await?;
        serde_json::from_value(value).map_err(|e| e.to_string())
    }

    async fn get_spot_meta_and_ctxs(&self) -> Result<SpotMetaAndCtxs, String> {
        let request = InfoRequest::spot_meta_and_asset_ctxs();
        let value = self.send_info_request(&request).await?;
        serde_json::from_value(value).map_err(|e| e.to_string())
    }

    async fn info_l2_book(&self, coin: &str) -> Result<HyperliquidL2Book, String> {
        let request = InfoRequest::l2_book(coin);
        let value = self.send_info_request(&request).await?;
        serde_json::from_value(value).map_err(|e| e.to_string())
    }

    async fn info_user_fills(&self, user: &str) -> Result<HyperliquidFills, String> {
        let request = InfoRequest::user_fills(user);
        let value = self.send_info_request(&request).await?;
        serde_json::from_value(value).map_err(|e| e.to_string())
    }

    async fn info_open_orders(&self, user: &str) -> Result<Value, String> {
        let request = InfoRequest::open_orders(user);
        self.send_info_request(&request).await
    }

    async fn info_clearinghouse_state(&self, user: &str) -> Result<Value, String> {
        let request = InfoRequest::clearinghouse_state(user);
        self.send_info_request(&request).await
    }

    async fn info_spot_clearinghouse_state(&self, user: &str) -> Result<Value, String> {
        let request = InfoRequest::spot_clearinghouse_state(user);
        self.send_info_request(&request).await
    }

    async fn send_info_request_raw(&self, request: &InfoRequest) -> Result<Value, String> {
        self.send_info_request(request).await
    }
}

fn create_domain_client(addr: &SocketAddr) -> HyperliquidHttpClient {
    let mut client = HyperliquidHttpClient::new(HyperliquidEnvironment::Mainnet, 60, None).unwrap();
    client.set_base_info_url(format!("http://{addr}/info"));
    client.set_base_exchange_url(format!("http://{addr}/exchange"));
    client.set_account_id(AccountId::new("HYPERLIQUID-master"));
    client
}

fn cache_btc_instrument(client: &HyperliquidHttpClient) {
    use nautilus_model::{
        enums::CurrencyType,
        identifiers::{InstrumentId, Symbol},
        instruments::{CryptoPerpetual, InstrumentAny},
        types::{Currency, Money, Price, Quantity},
    };

    let btc = Currency::new("BTC", 8, 0, "BTC", CurrencyType::Crypto);
    let usd = Currency::new("USD", 2, 0, "USD", CurrencyType::Fiat);
    let usdc = Currency::new("USDC", 6, 0, "USDC", CurrencyType::Crypto);
    let ts = nautilus_core::time::get_atomic_clock_realtime().get_time_ns();

    let instrument = CryptoPerpetual::new_checked(
        InstrumentId::from("BTC-USD-PERP.HYPERLIQUID"),
        Symbol::new("BTC"),
        btc,
        usd,
        usdc,
        false,
        1,
        5,
        Price::from("0.1"),
        Quantity::from("0.00001"),
        None,
        None,
        None,
        None,
        None,
        Some(Money::from("0.1 USDC")),
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        ts,
        ts,
    )
    .unwrap();

    client.cache_instrument(&InstrumentAny::CryptoPerpetual(instrument));
}

#[rstest]
#[tokio::test]
async fn test_request_order_status_report_open_order() {
    let state = TestServerState::default();
    *state.frontend_open_orders_response.lock().await = Some(json!([{
        "coin": "BTC",
        "side": "B",
        "limitPx": "95000.0",
        "sz": "0.05",
        "oid": 12345,
        "timestamp": 1700000000000u64,
        "origSz": "0.1",
        "cloid": "0xaabbccdd00112233aabbccdd00112233"
    }]));

    let addr = start_mock_server(state).await;
    let client = create_domain_client(&addr);
    cache_btc_instrument(&client);

    let report = client
        .request_order_status_report("0xuser", 12345)
        .await
        .unwrap()
        .expect("should find open order");

    // Status stays Accepted (matching bulk path) because we lack avg_px
    // for safe PartiallyFilled reconciliation. Real fills arrive via WebSocket.
    assert_eq!(report.order_status, OrderStatus::Accepted);
    assert!(report.price.is_some(), "open order retains limit price");
    assert_eq!(report.filled_qty.as_f64(), 0.05);
    assert_eq!(report.quantity.as_f64(), 0.1);
}

#[rstest]
#[tokio::test]
async fn test_request_order_status_report_triggered_order() {
    let state = TestServerState::default();
    *state.frontend_open_orders_response.lock().await = Some(json!([{
        "coin": "BTC",
        "side": "A",
        "limitPx": "90000.0",
        "sz": "0.1",
        "oid": 99999,
        "timestamp": 1700000000000u64,
        "origSz": "0.1",
        "triggerPx": "91000.0",
        "isMarket": true,
        "tpsl": "sl",
        "triggerActivated": true
    }]));

    let addr = start_mock_server(state).await;
    let client = create_domain_client(&addr);
    cache_btc_instrument(&client);

    let report = client
        .request_order_status_report("0xuser", 99999)
        .await
        .unwrap()
        .expect("should find triggered order");

    assert_eq!(report.order_status, OrderStatus::Triggered);
}

#[rstest]
#[tokio::test]
async fn test_request_order_status_report_closed_order_fallback() {
    let state = TestServerState::default();
    // frontendOpenOrders returns empty (order no longer open)
    *state.order_status_response.lock().await = Some(json!({
        "status": "order",
        "order": {
            "order": {
                "coin": "BTC",
                "side": "B",
                "limitPx": "95000.0",
                "sz": "0.0",
                "oid": 55555,
                "timestamp": 1700000000000u64,
                "origSz": "0.1"
            },
            "status": "filled",
            "statusTimestamp": 1700001000000u64
        }
    }));

    let addr = start_mock_server(state).await;
    let client = create_domain_client(&addr);
    cache_btc_instrument(&client);

    let report = client
        .request_order_status_report("0xuser", 55555)
        .await
        .unwrap()
        .expect("should find closed order via fallback");

    assert_eq!(report.order_status, OrderStatus::Filled);
    assert_eq!(
        report.ts_last.as_u64(),
        1700001000000u64 * 1_000_000,
        "ts_last should use statusTimestamp"
    );
}

#[rstest]
#[tokio::test]
async fn test_request_order_status_report_closed_order_fallback_propagates_cloid() {
    let coid = ClientOrderId::new("O-20240101-000042");
    let cloid_hex = Cloid::from_client_order_id(coid).to_hex();

    let state = TestServerState::default();
    *state.order_status_response.lock().await = Some(json!({
        "status": "order",
        "order": {
            "order": {
                "coin": "BTC",
                "side": "B",
                "limitPx": "95000.0",
                "sz": "0.0",
                "oid": 55556,
                "timestamp": 1700000000000u64,
                "origSz": "0.1",
                "cloid": cloid_hex,
            },
            "status": "canceled",
            "statusTimestamp": 1700001000000u64
        }
    }));

    let addr = start_mock_server(state).await;
    let client = create_domain_client(&addr);
    cache_btc_instrument(&client);

    let report = client
        .request_order_status_report("0xuser", 55556)
        .await
        .unwrap()
        .expect("should find closed order via fallback");

    assert_eq!(report.order_status, OrderStatus::Canceled);
    assert_eq!(
        report.client_order_id,
        Some(ClientOrderId::new(cloid_hex.as_str())),
        "closed order report should carry the cloid hex from the API response",
    );
}

#[rstest]
#[tokio::test]
async fn test_request_order_status_report_not_found() {
    let state = TestServerState::default();
    // Both endpoints return empty
    *state.order_status_response.lock().await = Some(json!({"status": "unknownOid"}));

    let addr = start_mock_server(state).await;
    let client = create_domain_client(&addr);
    cache_btc_instrument(&client);

    let report = client
        .request_order_status_report("0xuser", 99999)
        .await
        .unwrap();

    assert!(report.is_none());
}

#[rstest]
#[tokio::test]
async fn test_request_order_status_report_by_client_order_id_matches_cloid() {
    let coid = ClientOrderId::new("O-20240101-000001");
    let cloid_hex = Cloid::from_client_order_id(coid).to_hex();

    let state = TestServerState::default();
    *state.frontend_open_orders_response.lock().await = Some(json!([{
        "coin": "BTC",
        "side": "B",
        "limitPx": "95000.0",
        "sz": "0.1",
        "oid": 77777,
        "timestamp": 1700000000000u64,
        "origSz": "0.1",
        "cloid": cloid_hex
    }]));

    let addr = start_mock_server(state).await;
    let client = create_domain_client(&addr);
    cache_btc_instrument(&client);

    let report = client
        .request_order_status_report_by_client_order_id("0xuser", &coid)
        .await
        .unwrap()
        .expect("should match by cloid hash");

    assert_eq!(
        report.client_order_id,
        Some(coid),
        "should return original client_order_id, not cloid hash"
    );
    assert_eq!(report.order_status, OrderStatus::Accepted);
}

#[rstest]
#[tokio::test]
async fn test_request_order_status_report_by_client_order_id_no_match() {
    let state = TestServerState::default();
    // frontendOpenOrders returns an order with a different cloid
    *state.frontend_open_orders_response.lock().await = Some(json!([{
        "coin": "BTC",
        "side": "B",
        "limitPx": "95000.0",
        "sz": "0.1",
        "oid": 88888,
        "timestamp": 1700000000000u64,
        "origSz": "0.1",
        "cloid": "0x0000000000000000000000000000dead"
    }]));

    let addr = start_mock_server(state).await;
    let client = create_domain_client(&addr);
    cache_btc_instrument(&client);

    let coid = ClientOrderId::new("O-20240101-999999");
    let report = client
        .request_order_status_report_by_client_order_id("0xuser", &coid)
        .await
        .unwrap();

    assert!(report.is_none(), "should not match different cloid");
}
