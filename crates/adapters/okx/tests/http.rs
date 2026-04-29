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

//! Integration tests for the OKX HTTP client using a mock Axum server.

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
    extract::Query,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Json},
    routing::{get, post},
};
use chrono::{Duration as ChronoDuration, Utc};
use nautilus_common::testing::wait_until_async;
use nautilus_core::UnixNanos;
use nautilus_model::{
    enums::{OrderSide, OrderType, TriggerType},
    identifiers::{AccountId, ClientOrderId, InstrumentId},
    instruments::{Instrument, InstrumentAny},
    types::{Price, Quantity},
};
use nautilus_network::http::HttpClient;
use nautilus_okx::{
    common::{
        enums::{
            OKXEnvironment, OKXInstrumentType, OKXOrderStatus, OKXPositionMode, OKXTradeMode,
            OKXTriggerType,
        },
        models::OKXInstrument,
    },
    http::{
        client::{OKXHttpClient, OKXRawHttpClient, OKXResponse},
        error::OKXHttpError,
        models::OKXAttachAlgoOrdRequest,
        query::{
            GetAlgoOrdersParamsBuilder, GetInstrumentsParamsBuilder, GetOptionSummaryParamsBuilder,
            GetOrderHistoryParams, GetOrderListParams, GetOrderParamsBuilder,
            GetPositionTiersParamsBuilder, GetPositionsParamsBuilder, GetTradeFeeParamsBuilder,
            GetTransactionDetailsParamsBuilder, SetPositionModeParamsBuilder,
        },
    },
};
use rstest::rstest;
use serde_json::{Value, json};
use ustr::Ustr;

#[derive(Clone, Default)]
struct TestServerState {
    request_count: Arc<tokio::sync::Mutex<usize>>,
    last_history_trades_query: Arc<tokio::sync::Mutex<Option<HashMap<String, String>>>>,
    last_pending_orders_query: Arc<tokio::sync::Mutex<Option<HashMap<String, String>>>>,
    last_order_history_query: Arc<tokio::sync::Mutex<Option<HashMap<String, String>>>>,
    last_order_detail_query: Arc<tokio::sync::Mutex<Option<HashMap<String, String>>>>,
    option_summary_queries: Arc<tokio::sync::Mutex<Vec<HashMap<String, String>>>>,
    option_summary_response: Arc<tokio::sync::Mutex<Option<Value>>>,
    algo_pending_queries: Arc<tokio::sync::Mutex<Vec<HashMap<String, String>>>>,
    algo_history_queries: Arc<tokio::sync::Mutex<Vec<HashMap<String, String>>>>,
    last_order_body: Arc<tokio::sync::Mutex<Option<Value>>>,
    last_algo_order_body: Arc<tokio::sync::Mutex<Option<Value>>>,
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

fn manifest_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn load_test_data(filename: &str) -> Value {
    let path = manifest_path().join("test_data").join(filename);
    let content = std::fs::read_to_string(path).unwrap();
    serde_json::from_str(&content).unwrap()
}

fn has_auth_headers(headers: &HeaderMap) -> bool {
    headers.contains_key("ok-access-key")
        && headers.contains_key("ok-access-passphrase")
        && headers.contains_key("ok-access-timestamp")
        && headers.contains_key("ok-access-sign")
}

fn load_instruments_any() -> Vec<InstrumentAny> {
    load_instruments_from("http_get_instruments_spot.json")
}

fn load_swap_instruments_any() -> Vec<InstrumentAny> {
    load_instruments_from("http_get_instruments_swap.json")
}

fn load_option_instruments_any() -> Vec<InstrumentAny> {
    load_instruments_from("http_get_instruments_option.json")
}

fn load_instruments_from(filename: &str) -> Vec<InstrumentAny> {
    let payload = load_test_data(filename);
    let response: OKXResponse<OKXInstrument> = serde_json::from_value(payload).unwrap();
    let ts_init = UnixNanos::default();
    response
        .data
        .iter()
        .filter_map(|raw| {
            nautilus_okx::common::parse::parse_instrument_any(raw, None, None, None, None, ts_init)
                .ok()
                .flatten()
        })
        .collect()
}

fn create_router(state: Arc<TestServerState>) -> Router {
    let instruments_state = state.clone();
    let history_state = state.clone();
    let option_summary_state = state.clone();
    let pending_state = state.clone();
    let order_history_state = state.clone();
    let order_detail_state = state.clone();
    let order_place_state = state.clone();
    let algo_pending_state = state.clone();
    let algo_history_state = state.clone();
    let algo_order_state = state;
    Router::new()
        .route(
            "/api/v5/public/instruments",
            get(move || {
                let state = instruments_state.clone();
                async move {
                    let mut count = state.request_count.lock().await;
                    *count += 1;

                    if *count > 3 {
                        return (
                            StatusCode::TOO_MANY_REQUESTS,
                            Json(json!({
                                "code": "50116",
                                "msg": "Rate limit reached",
                                "data": [],
                            })),
                        )
                            .into_response();
                    }

                    Json(load_test_data("http_get_instruments_spot.json")).into_response()
                }
            }),
        )
        .route(
            "/api/v5/public/mark-price",
            get(|| async { Json(load_test_data("http_get_mark_price.json")) }),
        )
        .route(
            "/api/v5/public/opt-summary",
            get(move |Query(params): Query<HashMap<String, String>>| {
                let state = option_summary_state.clone();
                async move {
                    state.option_summary_queries.lock().await.push(params);
                    let override_resp = state.option_summary_response.lock().await.clone();
                    let data = override_resp
                        .unwrap_or_else(|| load_test_data("http_get_option_summary.json"));
                    Json(data).into_response()
                }
            }),
        )
        .route(
            "/api/v5/market/history-trades",
            get(move |Query(params): Query<HashMap<String, String>>| {
                let state = history_state.clone();
                async move {
                    *state.last_history_trades_query.lock().await = Some(params);
                    Json(json!({
                        "code": "0",
                        "msg": "",
                        "data": [],
                    }))
                }
            }),
        )
        .route(
            "/api/v5/account/balance",
            get(|headers: HeaderMap| async move {
                if !has_auth_headers(&headers) {
                    return (
                        StatusCode::UNAUTHORIZED,
                        Json(json!({
                            "code": "401",
                            "msg": "Missing authentication headers",
                            "data": [],
                        })),
                    )
                        .into_response();
                }

                Json(load_test_data("http_get_account_balance.json")).into_response()
            }),
        )
        .route(
            "/api/v5/trade/orders-pending",
            get(
                move |headers: HeaderMap, Query(params): Query<HashMap<String, String>>| {
                    let state = pending_state.clone();
                    async move {
                        if !has_auth_headers(&headers) {
                            return (
                                StatusCode::UNAUTHORIZED,
                                Json(json!({
                                    "code": "401",
                                    "msg": "Missing authentication headers",
                                    "data": [],
                                })),
                            )
                                .into_response();
                        }

                        let fixture =
                            if params.get("instId").map(String::as_str) == Some("ETH-USDT-SWAP") {
                                "http_get_orders_pending_with_attached_tp_sl.json"
                            } else {
                                "http_get_orders_pending.json"
                            };

                        *state.last_pending_orders_query.lock().await = Some(params);
                        Json(load_test_data(fixture)).into_response()
                    }
                },
            ),
        )
        .route(
            "/api/v5/trade/orders-history",
            get(
                move |headers: HeaderMap, Query(params): Query<HashMap<String, String>>| {
                    let state = order_history_state.clone();
                    async move {
                        if !has_auth_headers(&headers) {
                            return (
                                StatusCode::UNAUTHORIZED,
                                Json(json!({
                                    "code": "401",
                                    "msg": "Missing authentication headers",
                                    "data": [],
                                })),
                            )
                                .into_response();
                        }

                        *state.last_order_history_query.lock().await = Some(params);
                        Json(load_test_data("http_get_orders_history.json")).into_response()
                    }
                },
            ),
        )
        .route(
            "/api/v5/trade/order",
            get(
                move |headers: HeaderMap, Query(params): Query<HashMap<String, String>>| {
                    let state = order_detail_state.clone();
                    async move {
                        if !has_auth_headers(&headers) {
                            return (
                                StatusCode::UNAUTHORIZED,
                                Json(json!({
                                    "code": "401",
                                    "msg": "Missing authentication headers",
                                    "data": [],
                                })),
                            )
                                .into_response();
                        }

                        *state.last_order_detail_query.lock().await = Some(params);
                        Json(load_test_data("http_get_orders_history.json")).into_response()
                    }
                },
            )
            .post(move |headers: HeaderMap, Json(payload): Json<Value>| {
                let state = order_place_state.clone();
                async move {
                    if !has_auth_headers(&headers) {
                        return (
                            StatusCode::UNAUTHORIZED,
                            Json(json!({
                                "code": "401",
                                "msg": "Missing authentication headers",
                                "data": [],
                            })),
                        )
                            .into_response();
                    }

                    *state.last_order_body.lock().await = Some(payload);
                    Json(json!({
                        "code": "0",
                        "msg": "",
                        "data": [
                            {
                                "ordId": "12345",
                                "clOrdId": "O-bracket-entry",
                                "sCode": "0",
                                "sMsg": "Order placed",
                            }
                        ],
                    }))
                    .into_response()
                }
            }),
        )
        .route(
            "/api/v5/trade/orders-algo-pending",
            get(
                move |headers: HeaderMap, Query(params): Query<HashMap<String, String>>| {
                    let state = algo_pending_state.clone();
                    async move {
                        if !has_auth_headers(&headers) {
                            return (
                                StatusCode::UNAUTHORIZED,
                                Json(json!({
                                    "code": "401",
                                    "msg": "Missing authentication headers",
                                    "data": [],
                                })),
                            )
                                .into_response();
                        }

                        state.algo_pending_queries.lock().await.push(params.clone());

                        if params.get("algoClOrdId").map(String::as_str) == Some("O-attached-oco") {
                            if params.get("ordType").map(String::as_str) != Some("oco") {
                                return (
                                    StatusCode::BAD_REQUEST,
                                    Json(json!({
                                        "code": "51000",
                                        "msg": "Parameter ordType error",
                                        "data": [],
                                    })),
                                )
                                    .into_response();
                            }

                            return Json(load_test_data(
                                "http_get_orders_algo_pending_attached_oco.json",
                            ))
                            .into_response();
                        }

                        let fixture = if params.get("algoClOrdId").map(String::as_str)
                            == Some("O-close-frac-status")
                        {
                            "http_get_orders_algo_pending_close_fraction.json"
                        } else {
                            "http_get_orders_algo_pending.json"
                        };

                        Json(load_test_data(fixture)).into_response()
                    }
                },
            ),
        )
        .route(
            "/api/v5/trade/orders-algo-history",
            get(
                move |headers: HeaderMap, Query(params): Query<HashMap<String, String>>| {
                    let state = algo_history_state.clone();
                    async move {
                        if !has_auth_headers(&headers) {
                            return (
                                StatusCode::UNAUTHORIZED,
                                Json(json!({
                                    "code": "401",
                                    "msg": "Missing authentication headers",
                                    "data": [],
                                })),
                            )
                                .into_response();
                        }

                        state.algo_history_queries.lock().await.push(params.clone());

                        if params.get("algoClOrdId").map(String::as_str) == Some("O-attached-oco") {
                            if params.get("ordType").map(String::as_str) != Some("oco") {
                                return (
                                    StatusCode::BAD_REQUEST,
                                    Json(json!({
                                        "code": "51000",
                                        "msg": "Parameter ordType error",
                                        "data": [],
                                    })),
                                )
                                    .into_response();
                            }

                            return Json(json!({
                                "code": "0",
                                "msg": "",
                                "data": [],
                            }))
                            .into_response();
                        }

                        if params.get("algoClOrdId").map(String::as_str)
                            == Some("O-close-frac-status")
                        {
                            return Json(json!({
                                "code": "0",
                                "msg": "",
                                "data": [],
                            }))
                            .into_response();
                        }

                        Json(load_test_data("http_get_orders_algo_history.json")).into_response()
                    }
                },
            ),
        )
        .route(
            "/api/v5/trade/order-algo",
            post(move |headers: HeaderMap, Json(payload): Json<Value>| {
                let state = algo_order_state.clone();
                async move {
                    if !has_auth_headers(&headers) {
                        return (
                            StatusCode::UNAUTHORIZED,
                            Json(json!({
                                "code": "401",
                                "msg": "Missing authentication headers",
                                "data": [],
                            })),
                        )
                            .into_response();
                    }

                    *state.last_algo_order_body.lock().await = Some(payload);
                    Json(load_test_data("http_place_algo_order_response.json")).into_response()
                }
            }),
        )
        .route(
            "/api/v5/trade/cancel-algos",
            post(|headers: HeaderMap| async move {
                if !has_auth_headers(&headers) {
                    return (
                        StatusCode::UNAUTHORIZED,
                        Json(json!({
                            "code": "401",
                            "msg": "Missing authentication headers",
                            "data": [],
                        })),
                    )
                        .into_response();
                }

                Json(load_test_data("http_cancel_algo_order_response.json")).into_response()
            }),
        )
        .route(
            "/api/v5/account/set-position-mode",
            post(|headers: HeaderMap| async move {
                if !has_auth_headers(&headers) {
                    return (
                        StatusCode::UNAUTHORIZED,
                        Json(json!({
                            "code": "401",
                            "msg": "Missing authentication headers",
                            "data": [],
                        })),
                    )
                        .into_response();
                }

                Json(load_test_data("http_set_position_mode_response.json")).into_response()
            }),
        )
        .route(
            "/api/v5/account/trade-fee",
            get(|headers: HeaderMap| async move {
                if !has_auth_headers(&headers) {
                    return (
                        StatusCode::UNAUTHORIZED,
                        Json(json!({
                            "code": "401",
                            "msg": "Missing authentication headers",
                            "data": [],
                        })),
                    )
                        .into_response();
                }

                Json(load_test_data("http_get_trade_fee_response.json")).into_response()
            }),
        )
        .route(
            "/api/v5/account/positions",
            get(|headers: HeaderMap| async move {
                if !has_auth_headers(&headers) {
                    return (
                        StatusCode::UNAUTHORIZED,
                        Json(json!({
                            "code": "401",
                            "msg": "Missing authentication headers",
                            "data": [],
                        })),
                    )
                        .into_response();
                }

                Json(load_test_data("http_get_positions.json")).into_response()
            }),
        )
        .route(
            "/api/v5/trade/fills",
            get(|headers: HeaderMap| async move {
                if !has_auth_headers(&headers) {
                    return (
                        StatusCode::UNAUTHORIZED,
                        Json(json!({
                            "code": "401",
                            "msg": "Missing authentication headers",
                            "data": [],
                        })),
                    )
                        .into_response();
                }

                Json(json!({
                    "code": "0",
                    "msg": "",
                    "data": [load_test_data("http_transaction_detail.json")],
                }))
                .into_response()
            }),
        )
        .route(
            "/api/v5/public/position-tiers",
            get(|| async { Json(load_test_data("http_get_position_tiers.json")) }),
        )
}

async fn start_test_server(state: Arc<TestServerState>) -> SocketAddr {
    let router = create_router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        axum::serve(listener, router.into_make_service())
            .await
            .unwrap();
    });

    wait_for_server(addr, "/api/v5/public/instruments").await;
    addr
}

#[rstest]
#[tokio::test]
async fn test_http_get_instruments_returns_data() {
    let addr = start_test_server(Arc::new(TestServerState::default())).await;
    let base_url = format!("http://{addr}");

    let params = GetInstrumentsParamsBuilder::default()
        .inst_type(OKXInstrumentType::Spot)
        .build()
        .unwrap();
    let client = OKXRawHttpClient::new(
        Some(base_url.clone()),
        60,
        3,
        1000,
        10_000,
        OKXEnvironment::Live,
        None,
    )
    .unwrap();

    let instruments = client.get_instruments(params).await.unwrap();

    assert!(!instruments.is_empty());
    assert_eq!(instruments[0].inst_type, OKXInstrumentType::Spot);
}

#[rstest]
#[tokio::test]
async fn test_http_get_balance_requires_credentials() {
    let addr = start_test_server(Arc::new(TestServerState::default())).await;
    let base_url = format!("http://{addr}");

    let client = OKXRawHttpClient::new(
        Some(base_url),
        60,
        3,
        1000,
        10_000,
        OKXEnvironment::Live,
        None,
    )
    .unwrap();

    let result = client.get_balance().await;

    match result {
        Err(OKXHttpError::MissingCredentials) => {}
        other => panic!("expected MissingCredentials error: {other:?}"),
    }
}

#[rstest]
#[tokio::test]
async fn test_http_get_balance_with_credentials_succeeds() {
    let addr = start_test_server(Arc::new(TestServerState::default())).await;
    let base_url = format!("http://{addr}");

    let client = OKXRawHttpClient::with_credentials(
        "test_key".to_string(),
        "test_secret".to_string(),
        "passphrase".to_string(),
        base_url.clone(),
        60,
        3,
        1000,
        10_000,
        OKXEnvironment::Live,
        None,
    )
    .unwrap();

    let accounts = client.get_balance().await.unwrap();

    assert!(!accounts.is_empty());
}

#[rstest]
#[tokio::test]
async fn test_http_get_instruments_handles_rate_limit_error() {
    let state = Arc::new(TestServerState::default());
    let addr = start_test_server(state.clone()).await;
    let base_url = format!("http://{addr}");

    let params = GetInstrumentsParamsBuilder::default()
        .inst_type(OKXInstrumentType::Spot)
        .build()
        .unwrap();
    let client = OKXRawHttpClient::new(
        Some(base_url.clone()),
        60,
        0,
        1000,
        10_000,
        OKXEnvironment::Live,
        None,
    )
    .unwrap();

    let mut last_error = None;

    for _ in 0..5 {
        match client.get_instruments(params.clone()).await {
            Ok(_) => {}
            Err(e) => {
                last_error = Some(e);
                break;
            }
        }
    }

    match last_error.unwrap() {
        OKXHttpError::OkxError { error_code, .. } => assert_eq!(error_code, "50116"),
        other => panic!("expected OkxError: {other:?}"),
    }
}

#[rstest]
#[tokio::test]
async fn test_http_get_pending_orders_requires_credentials() {
    let addr = start_test_server(Arc::new(TestServerState::default())).await;
    let base_url = format!("http://{addr}");

    let client = OKXRawHttpClient::new(
        Some(base_url),
        60,
        3,
        1000,
        10_000,
        OKXEnvironment::Live,
        None,
    )
    .unwrap();

    let params = GetOrderListParams {
        inst_type: Some(OKXInstrumentType::Swap),
        inst_id: Some("BTC-USDT-SWAP".to_string()),
        inst_family: None,
        state: None,
        after: None,
        before: None,
        limit: None,
    };

    match client.get_orders_pending(params).await {
        Err(OKXHttpError::MissingCredentials) => {}
        other => panic!("expected MissingCredentials error: {other:?}"),
    }
}

#[rstest]
#[tokio::test]
async fn test_http_get_pending_orders_returns_live_orders() {
    let state = Arc::new(TestServerState::default());
    let addr = start_test_server(state.clone()).await;
    let base_url = format!("http://{addr}");

    let client = OKXRawHttpClient::with_credentials(
        "key".to_string(),
        "secret".to_string(),
        "pass".to_string(),
        base_url.clone(),
        60,
        3,
        1000,
        10_000,
        OKXEnvironment::Live,
        None,
    )
    .unwrap();

    let params = GetOrderListParams {
        inst_type: Some(OKXInstrumentType::Swap),
        inst_id: Some("BTC-USDT-SWAP".to_string()),
        inst_family: None,
        state: None,
        after: None,
        before: None,
        limit: None,
    };

    let orders = client.get_orders_pending(params).await.unwrap();

    assert_eq!(orders.len(), 1);
    assert_eq!(orders[0].state, OKXOrderStatus::Live);
    assert_eq!(orders[0].inst_id.as_str(), "BTC-USDT-SWAP");

    let query = state
        .last_pending_orders_query
        .lock()
        .await
        .clone()
        .unwrap();
    assert_eq!(query.get("instType"), Some(&"SWAP".to_string()));
    assert_eq!(query.get("instId"), Some(&"BTC-USDT-SWAP".to_string()));
}

#[rstest]
#[tokio::test]
async fn test_http_get_order_history_applies_filters() {
    let state = Arc::new(TestServerState::default());
    let addr = start_test_server(state.clone()).await;
    let base_url = format!("http://{addr}");

    let client = OKXRawHttpClient::with_credentials(
        "key".to_string(),
        "secret".to_string(),
        "pass".to_string(),
        base_url.clone(),
        60,
        3,
        1000,
        10_000,
        OKXEnvironment::Live,
        None,
    )
    .unwrap();

    let params = GetOrderHistoryParams {
        inst_type: OKXInstrumentType::Swap,
        uly: None,
        inst_family: None,
        inst_id: Some("BTC-USDT-SWAP".to_string()),
        ord_type: None,
        state: Some("filled".to_string()),
        after: None,
        before: None,
        limit: Some(50),
    };

    let orders = client.get_orders_history(params).await.unwrap();
    assert!(!orders.is_empty());

    let query = state.last_order_history_query.lock().await.clone().unwrap();
    assert_eq!(query.get("instType"), Some(&"SWAP".to_string()));
    assert_eq!(query.get("instId"), Some(&"BTC-USDT-SWAP".to_string()));
    assert_eq!(query.get("state"), Some(&"filled".to_string()));
    assert_eq!(query.get("limit"), Some(&"50".to_string()));
}

#[rstest]
#[tokio::test]
async fn test_http_get_order_by_client_and_exchange_ids() {
    let state = Arc::new(TestServerState::default());
    let addr = start_test_server(state.clone()).await;
    let base_url = format!("http://{addr}");

    let client = OKXRawHttpClient::with_credentials(
        "key".to_string(),
        "secret".to_string(),
        "pass".to_string(),
        base_url.clone(),
        60,
        3,
        1000,
        10_000,
        OKXEnvironment::Live,
        None,
    )
    .unwrap();

    let params = GetOrderParamsBuilder::default()
        .inst_type(OKXInstrumentType::Swap)
        .inst_id("BTC-USDT-SWAP")
        .ord_id("1234567890123456789")
        .cl_ord_id("client-order-1")
        .build()
        .unwrap();

    let orders = client.get_order(params).await.unwrap();
    assert_eq!(orders.len(), 1);

    let query = state.last_order_detail_query.lock().await.clone().unwrap();
    assert_eq!(query.get("instType"), Some(&"SWAP".to_string()));
    assert_eq!(query.get("instId"), Some(&"BTC-USDT-SWAP".to_string()));
    assert_eq!(query.get("ordId"), Some(&"1234567890123456789".to_string()));
    assert_eq!(query.get("clOrdId"), Some(&"client-order-1".to_string()));
}

#[tokio::test]
async fn test_request_trades_pagination_parameters() {
    let state = Arc::new(TestServerState::default());
    let addr = start_test_server(state.clone()).await;
    let base_url = format!("http://{addr}");

    let client = OKXHttpClient::new(
        Some(base_url),
        60,
        3,
        1000,
        10_000,
        OKXEnvironment::Live,
        None,
    )
    .unwrap();

    for instrument in load_instruments_any() {
        client.cache_instrument(instrument);
    }

    let start = Utc::now() - ChronoDuration::minutes(5);
    let end = Utc::now();

    let trades = client
        .request_trades(
            InstrumentId::from("BTC-USD.OKX"),
            Some(start),
            Some(end),
            Some(150),
        )
        .await
        .unwrap();
    assert!(trades.is_empty());

    let query = state
        .last_history_trades_query
        .lock()
        .await
        .clone()
        .unwrap();

    assert_eq!(query.get("instId"), Some(&"BTC-USD".to_string()));
    assert!(
        !query.contains_key("before"),
        "First request should fetch latest trades (no before parameter)"
    );
    assert_eq!(query.get("limit"), Some(&"100".to_string()));
    assert_eq!(
        query.get("type"),
        Some(&"1".to_string()),
        "Should use trade ID pagination"
    );
}

#[tokio::test]
async fn test_request_trades_latest_mode() {
    let router = Router::new()
        .route(
            "/api/v5/public/instruments",
            get({
                move || async move {
                    Json(load_test_data("http_get_instruments_spot.json")).into_response()
                }
            }),
        )
        .route(
            "/api/v5/market/history-trades",
            get({
                move |Query(_params): Query<HashMap<String, String>>| async move {
                    let data = vec![
                        json!({
                            "instId": "BTC-USD",
                            "side": "buy",
                            "sz": "0.01",
                            "px": "100000.0",
                            "tradeId": "999999",
                            "ts": "1747087163557",
                        }),
                        json!({
                            "instId": "BTC-USD",
                            "side": "sell",
                            "sz": "0.01",
                            "px": "100000.0",
                            "tradeId": "999998",
                            "ts": "1747087163556",
                        }),
                    ];

                    Json(json!({
                        "code": "0",
                        "msg": "",
                        "data": data,
                    }))
                }
            }),
        );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        axum::serve(listener, router.into_make_service())
            .await
            .unwrap();
    });

    wait_for_server(addr, "/api/v5/public/instruments").await;

    let base_url = format!("http://{addr}");
    let client = OKXHttpClient::new(
        Some(base_url),
        60,
        3,
        1000,
        10_000,
        OKXEnvironment::Live,
        None,
    )
    .unwrap();

    for instrument in load_instruments_any() {
        client.cache_instrument(instrument);
    }

    let trades = client
        .request_trades(InstrumentId::from("BTC-USD.OKX"), None, None, None)
        .await
        .unwrap();

    assert!(!trades.is_empty(), "Should retrieve latest trades");
    assert_eq!(trades.len(), 2, "Should return all trades from API");

    for i in 1..trades.len() {
        assert!(
            trades[i].ts_event >= trades[i - 1].ts_event,
            "Trades should be in chronological order"
        );
    }
}

#[tokio::test]
async fn test_request_trades_chronological_order() {
    let router = Router::new()
        .route(
            "/api/v5/public/instruments",
            get({
                move || async move {
                    Json(load_test_data("http_get_instruments_spot.json")).into_response()
                }
            }),
        )
        .route(
            "/api/v5/market/history-trades",
            get({
                move |Query(_params): Query<HashMap<String, String>>| async move {
                    let data = vec![
                        json!({
                            "instId": "BTC-USD",
                            "side": "buy",
                            "sz": "0.01",
                            "px": "100000.0",
                            "tradeId": "1005",
                            "ts": "1747087165000",
                        }),
                        json!({
                            "instId": "BTC-USD",
                            "side": "sell",
                            "sz": "0.01",
                            "px": "100000.0",
                            "tradeId": "1004",
                            "ts": "1747087164000",
                        }),
                        json!({
                            "instId": "BTC-USD",
                            "side": "buy",
                            "sz": "0.01",
                            "px": "100000.0",
                            "tradeId": "1003",
                            "ts": "1747087163000",
                        }),
                        json!({
                            "instId": "BTC-USD",
                            "side": "sell",
                            "sz": "0.01",
                            "px": "100000.0",
                            "tradeId": "1002",
                            "ts": "1747087162000",
                        }),
                        json!({
                            "instId": "BTC-USD",
                            "side": "buy",
                            "sz": "0.01",
                            "px": "100000.0",
                            "tradeId": "1001",
                            "ts": "1747087161000",
                        }),
                    ];

                    Json(json!({
                        "code": "0",
                        "msg": "",
                        "data": data,
                    }))
                }
            }),
        );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        axum::serve(listener, router.into_make_service())
            .await
            .unwrap();
    });

    wait_for_server(addr, "/api/v5/public/instruments").await;

    let base_url = format!("http://{addr}");
    let client = OKXHttpClient::new(
        Some(base_url),
        60,
        3,
        1000,
        10_000,
        OKXEnvironment::Live,
        None,
    )
    .unwrap();

    for instrument in load_instruments_any() {
        client.cache_instrument(instrument);
    }

    let trades = client
        .request_trades(InstrumentId::from("BTC-USD.OKX"), None, None, None)
        .await
        .unwrap();

    assert_eq!(trades.len(), 5, "Should return all 5 trades");

    // Verify trades are in ASCENDING chronological order (oldest first)
    assert!(
        trades[0].ts_event < trades[1].ts_event,
        "First trade should be older than second. Got: {} < {}",
        trades[0].ts_event,
        trades[1].ts_event
    );
    assert!(
        trades[1].ts_event < trades[2].ts_event,
        "Second trade should be older than third"
    );
    assert!(
        trades[2].ts_event < trades[3].ts_event,
        "Third trade should be older than fourth"
    );
    assert!(
        trades[3].ts_event < trades[4].ts_event,
        "Fourth trade should be older than fifth"
    );

    let oldest_ts = trades.iter().map(|t| t.ts_event).min().unwrap();
    assert_eq!(
        trades[0].ts_event, oldest_ts,
        "First trade should be the oldest"
    );

    let newest_ts = trades.iter().map(|t| t.ts_event).max().unwrap();
    assert_eq!(
        trades[4].ts_event, newest_ts,
        "Last trade should be the newest"
    );
}

#[tokio::test]
async fn test_request_trades_range_mode_pagination() {
    let router = Router::new()
        .route(
            "/api/v5/public/instruments",
            get({
                move || async move {
                    Json(load_test_data("http_get_instruments_spot.json")).into_response()
                }
            }),
        )
        .route(
            "/api/v5/market/history-trades",
            get({
                move |Query(params): Query<HashMap<String, String>>| async move {
                    let now_ms = Utc::now().timestamp_millis();
                    // OKX backwards semantics: 'after' is used for backward pagination (get older trades)
                    let after_trade_id = params.get("after").and_then(|s| s.parse::<i64>().ok());

                    let data = if let Some(after_id) = after_trade_id {
                        let mut trades = Vec::new();

                        for i in 0..100 {
                            let trade_id = after_id - i - 1;
                            if trade_id <= 0 {
                                break;
                            }

                            // Calculate timestamp: trade IDs > 999900 are recent (< 1 hour ago)
                            // trade IDs <= 999900 are historical (90+ minutes ago, within 1-2 hour range)
                            let ts_ms = if trade_id > 999900 {
                                // Recent: 1-10 seconds ago (will be filtered out)
                                now_ms - ((999999 - trade_id) * 100)
                            } else {
                                // Historical: 90-92 minutes ago (within 1-2 hour range)
                                let offset_from_boundary = 999900 - trade_id;
                                now_ms - (90 * 60 * 1000) - (offset_from_boundary * 1000)
                            };

                            trades.push(json!({
                                "instId": "BTC-USD",
                                "side": if i % 2 == 0 { "buy" } else { "sell" },
                                "sz": "0.01",
                                "px": "100000.0",
                                "tradeId": trade_id.to_string(),
                                "ts": ts_ms.to_string(),
                            }));
                        }
                        trades
                    } else {
                        // First request with no 'after' - return latest trades
                        vec![
                            json!({
                                "instId": "BTC-USD",
                                "side": "buy",
                                "sz": "0.01",
                                "px": "100000.0",
                                "tradeId": "999999",
                                "ts": (now_ms - 1000).to_string(),
                            }),
                            json!({
                                "instId": "BTC-USD",
                                "side": "sell",
                                "sz": "0.01",
                                "px": "100000.0",
                                "tradeId": "999998",
                                "ts": (now_ms - 2000).to_string(),
                            }),
                        ]
                    };

                    Json(json!({
                        "code": "0",
                        "msg": "",
                        "data": data,
                    }))
                }
            }),
        );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        axum::serve(listener, router.into_make_service())
            .await
            .unwrap();
    });

    wait_for_server(addr, "/api/v5/public/instruments").await;

    let base_url = format!("http://{addr}");
    let client = OKXHttpClient::new(
        Some(base_url),
        60,
        3,
        1000,
        10_000,
        OKXEnvironment::Live,
        None,
    )
    .unwrap();

    for instrument in load_instruments_any() {
        client.cache_instrument(instrument);
    }

    // Regression test for issue #2997 where Range mode pagination could get stuck
    // when all trades on a page are filtered out
    let start = Utc::now() - ChronoDuration::hours(2);
    let end = Utc::now() - ChronoDuration::hours(1);

    let trades = client
        .request_trades(
            InstrumentId::from("BTC-USD.OKX"),
            Some(start),
            Some(end),
            Some(100),
        )
        .await
        .unwrap();

    assert!(!trades.is_empty(), "Should retrieve trades in Range mode");

    for trade in &trades {
        let trade_ts = trade.ts_event.as_i64();
        let start_ns = start.timestamp_nanos_opt().unwrap();
        let end_ns = end.timestamp_nanos_opt().unwrap();
        assert!(
            trade_ts >= start_ns && trade_ts <= end_ns,
            "Trade timestamp should be within requested range"
        );
    }

    for i in 1..trades.len() {
        assert!(
            trades[i].ts_event >= trades[i - 1].ts_event,
            "Trades should be in chronological order"
        );
    }
}

#[tokio::test]
async fn test_request_bars_range_mode_pagination() {
    use nautilus_model::{
        data::{BarSpecification, BarType},
        enums::{AggregationSource, BarAggregation, PriceType},
        identifiers::InstrumentId,
    };

    let router = Router::new()
        .route(
            "/api/v5/public/instruments",
            get({
                move || async move {
                    Json(load_test_data("http_get_instruments_swap.json")).into_response()
                }
            }),
        )
        .route(
            "/api/v5/market/candles",
            get({
                move |Query(params): Query<HashMap<String, String>>| async move {
                    // OKX backwards semantics: after=upper bound, before=lower bound
                    let after = params.get("after").and_then(|s| s.parse::<i64>().ok());
                    let before = params.get("before").and_then(|s| s.parse::<i64>().ok());

                    let data = if let Some(a) = after {
                        let mut bars = Vec::new();

                        for i in 0..10 {
                            let ts = a - ((i + 1) * 60_000);

                            if let Some(b) = before
                                && ts <= b
                            {
                                break;
                            }
                            bars.push(json!([
                                ts.to_string(),
                                "100000.0",
                                "100100.0",
                                "99900.0",
                                "100050.0",
                                "10.5",
                                "0",
                                "0",
                                "0"
                            ]));
                        }
                        bars
                    } else {
                        vec![]
                    };

                    Json(json!({
                        "code": "0",
                        "msg": "",
                        "data": data,
                    }))
                }
            }),
        )
        .route(
            "/api/v5/market/history-candles",
            get({
                move |Query(params): Query<HashMap<String, String>>| async move {
                    // OKX backwards semantics: after=upper bound, before=lower bound
                    let after = params.get("after").and_then(|s| s.parse::<i64>().ok());
                    let before = params.get("before").and_then(|s| s.parse::<i64>().ok());

                    let data = if let Some(a) = after {
                        let mut bars = Vec::new();

                        for i in 0..50 {
                            let ts = a - ((i + 1) * 60_000);

                            if let Some(b) = before
                                && ts <= b
                            {
                                break;
                            }
                            bars.push(json!([
                                ts.to_string(),
                                "100000.0",
                                "100100.0",
                                "99900.0",
                                "100050.0",
                                "10.5",
                                "0",
                                "0",
                                "0"
                            ]));
                        }
                        bars
                    } else {
                        vec![]
                    };

                    Json(json!({
                        "code": "0",
                        "msg": "",
                        "data": data,
                    }))
                }
            }),
        );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        axum::serve(listener, router.into_make_service())
            .await
            .unwrap();
    });

    wait_for_server(addr, "/api/v5/public/instruments").await;

    let base_url = format!("http://{addr}");
    let client = OKXHttpClient::new(
        Some(base_url),
        60,
        3,
        1000,
        10_000,
        OKXEnvironment::Live,
        None,
    )
    .unwrap();

    for instrument in load_instruments_any() {
        client.cache_instrument(instrument);
    }

    let bar_type = BarType::new(
        InstrumentId::from("BTC-USD.OKX"),
        BarSpecification::new(1, BarAggregation::Minute, PriceType::Last),
        AggregationSource::External,
    );

    // Regression test for issue #3145 where Range mode pagination could get stuck
    // when all bars on a page are filtered out
    let start = Utc::now() - ChronoDuration::hours(2);
    let end = Utc::now() - ChronoDuration::hours(1);

    let bars = client
        .request_bars(bar_type, Some(start), Some(end), Some(100))
        .await
        .unwrap();

    assert!(!bars.is_empty(), "Should retrieve bars in Range mode");

    for bar in &bars {
        let bar_ts = bar.ts_event.as_i64();
        let start_ns = start.timestamp_nanos_opt().unwrap();
        let end_ns = end.timestamp_nanos_opt().unwrap();
        assert!(
            bar_ts >= start_ns && bar_ts <= end_ns,
            "Bar timestamp should be within requested range"
        );
    }

    for i in 1..bars.len() {
        assert!(
            bars[i].ts_event >= bars[i - 1].ts_event,
            "Bars should be in chronological order"
        );
    }
}

#[tokio::test]
async fn test_request_trades_multi_page_chronological_order() {
    // Regression test: verify chronological order is maintained when pagination
    // fetches multiple pages (each page contains older trades than the previous)
    let state = Arc::new(TestServerState::default());
    let addr = start_test_server(state.clone()).await;
    let base_url = format!("http://{addr}");

    let client = OKXHttpClient::new(
        Some(base_url),
        60,
        3,
        1000,
        10_000,
        OKXEnvironment::Live,
        None,
    )
    .unwrap();

    for instrument in load_instruments_any() {
        client.cache_instrument(instrument);
    }

    // Request range that spans multiple pages (typical page = 100 trades)
    let start = Utc::now() - ChronoDuration::minutes(10);
    let end = Utc::now();

    let trades = client
        .request_trades(
            InstrumentId::from("BTC-USD.OKX"),
            Some(start),
            Some(end),
            Some(250), // Request more than one page
        )
        .await
        .unwrap();

    if trades.len() > 100 {
        // Verify strict monotonic order across page boundary
        for i in 1..trades.len() {
            assert!(
                trades[i].ts_event >= trades[i - 1].ts_event,
                "Trade timestamps must be monotonically increasing. \
                 Found ts[{}]={} < ts[{}]={} (likely page boundary issue)",
                i,
                trades[i].ts_event,
                i - 1,
                trades[i - 1].ts_event
            );
        }
    }
}

#[tokio::test]
async fn test_request_trades_overlapping_pages_chronological_order() {
    // Regression test: verify that overlapping trades across pages are handled correctly
    // This simulates the real OKX API behavior where pages might have overlapping trades
    // when the pagination cursor points to the middle of a timestamp cluster
    let call_count = Arc::new(AtomicUsize::new(0));
    let call_count_clone = call_count.clone();

    let router = Router::new()
        .route(
            "/api/v5/public/instruments",
            get({
                move || async move {
                    Json(load_test_data("http_get_instruments_spot.json")).into_response()
                }
            }),
        )
        .route(
            "/api/v5/market/history-trades",
            get({
                move |Query(_params): Query<HashMap<String, String>>| {
                    let count = call_count_clone.clone();
                    async move {
                        let call_num = count.fetch_add(1, Ordering::SeqCst);

                        let data = match call_num {
                            0 => {
                                // Page 1: Latest trades (newest first as OKX returns)
                                vec![
                                    json!({"instId": "BTC-USD", "side": "buy", "sz": "0.01", "px": "100000.0", "tradeId": "1010", "ts": "1747087170000"}),
                                    json!({"instId": "BTC-USD", "side": "sell", "sz": "0.01", "px": "100000.0", "tradeId": "1009", "ts": "1747087169000"}),
                                    json!({"instId": "BTC-USD", "side": "buy", "sz": "0.01", "px": "100000.0", "tradeId": "1008", "ts": "1747087168000"}),
                                ]
                            },
                            1 => {
                                // Page 2: Older trades, BUT with one overlapping trade ID
                                vec![
                                    json!({"instId": "BTC-USD", "side": "buy", "sz": "0.01", "px": "100000.0", "tradeId": "1008", "ts": "1747087168000"}), // Same as page 1 last trade!
                                    json!({"instId": "BTC-USD", "side": "sell", "sz": "0.01", "px": "100000.0", "tradeId": "1007", "ts": "1747087167000"}),
                                    json!({"instId": "BTC-USD", "side": "buy", "sz": "0.01", "px": "100000.0", "tradeId": "1006", "ts": "1747087166000"}),
                                ]
                            },
                            _ => vec![],
                        };

                        Json(json!({
                            "code": "0",
                            "msg": "",
                            "data": data,
                        }))
                    }
                }
            }),
        );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        axum::serve(listener, router.into_make_service())
            .await
            .unwrap();
    });

    wait_for_server(addr, "/api/v5/public/instruments").await;

    let base_url = format!("http://{addr}");
    let client = OKXHttpClient::new(
        Some(base_url),
        60,
        3,
        1000,
        10_000,
        OKXEnvironment::Live,
        None,
    )
    .unwrap();

    for instrument in load_instruments_any() {
        client.cache_instrument(instrument);
    }

    // Use Range mode with end timestamp to trigger backward pagination
    let end = Utc::now();
    let trades = client
        .request_trades(InstrumentId::from("BTC-USD.OKX"), None, Some(end), Some(10))
        .await
        .unwrap();

    // Should have 5 unique trades after deduplication: 1006, 1007, 1008, 1009, 1010
    // (trade 1008 appears in both pages but should only appear once)
    assert_eq!(
        trades.len(),
        5,
        "Expected 5 unique trades after deduplication"
    );

    // Verify strict chronological order
    for i in 1..trades.len() {
        assert!(
            trades[i].ts_event >= trades[i - 1].ts_event,
            "Trade timestamps must be monotonically increasing. \
             Found ts[{}]={} < ts[{}]={}",
            i,
            trades[i].ts_event,
            i - 1,
            trades[i - 1].ts_event
        );
    }

    // Verify no duplicate trade IDs (should deduplicate overlapping trade ID 1008)
    let mut seen_ids = std::collections::HashSet::new();

    for trade in &trades {
        assert!(
            seen_ids.insert(trade.trade_id),
            "Duplicate trade ID: {:?}",
            trade.trade_id
        );
    }
}

#[tokio::test]
async fn test_request_trades_default_limit_with_end_only() {
    // Regression test: verify that limit=None defaults to 100 trades
    // and doesn't paginate forever when only end timestamp is provided
    let call_count = Arc::new(AtomicUsize::new(0));
    let call_count_clone = call_count.clone();

    let router = Router::new()
        .route(
            "/api/v5/public/instruments",
            get({
                move || async move {
                    Json(load_test_data("http_get_instruments_spot.json")).into_response()
                }
            }),
        )
        .route(
            "/api/v5/market/history-trades",
            get({
                move |Query(_params): Query<HashMap<String, String>>| {
                    let count = call_count_clone.clone();
                    async move {
                        let call_num = count.fetch_add(1, Ordering::SeqCst);

                        // Mock returns 100 trades per page
                        let mut data = Vec::new();
                        let base_id = 2000 - (call_num * 100);
                        let base_ts = 1747087170000i64 - (call_num as i64 * 10000);

                        for i in 0..100 {
                            data.push(json!({
                                "instId": "BTC-USD",
                                "side": if i % 2 == 0 { "buy" } else { "sell" },
                                "sz": "0.01",
                                "px": "100000.0",
                                "tradeId": (base_id - i).to_string(),
                                "ts": (base_ts - (i as i64 * 100)).to_string(),
                            }));
                        }

                        Json(json!({
                            "code": "0",
                            "msg": "",
                            "data": data,
                        }))
                    }
                }
            }),
        );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        axum::serve(listener, router.into_make_service())
            .await
            .unwrap();
    });

    wait_for_server(addr, "/api/v5/public/instruments").await;

    let base_url = format!("http://{addr}");
    let client = OKXHttpClient::new(
        Some(base_url),
        60,
        3,
        1000,
        10_000,
        OKXEnvironment::Live,
        None,
    )
    .unwrap();

    for instrument in load_instruments_any() {
        client.cache_instrument(instrument);
    }

    // Request with end timestamp but no limit (should default to 100)
    let end = Utc::now();
    let trades = client
        .request_trades(
            InstrumentId::from("BTC-USD.OKX"),
            None,
            Some(end),
            None, // No explicit limit
        )
        .await
        .unwrap();

    // Should stop at default limit of 100, not paginate forever
    assert_eq!(
        trades.len(),
        100,
        "Expected exactly 100 trades with limit=None (default)"
    );

    // Should only make 1 API call (not multiple pages)
    assert_eq!(
        call_count.load(Ordering::SeqCst),
        1,
        "Should only fetch one page with default limit"
    );
}

#[tokio::test]
async fn test_request_trades_historical_with_filtered_pages() {
    // Regression test: historical queries must paginate through pages where all
    // trades fall outside the requested range until reaching valid trades,
    // rather than stopping early based on zero contribution
    let router = Router::new()
        .route(
            "/api/v5/public/instruments",
            get({
                move || async move {
                    Json(load_test_data("http_get_instruments_spot.json")).into_response()
                }
            }),
        )
        .route(
            "/api/v5/market/history-trades",
            get({
                move |Query(params): Query<HashMap<String, String>>| async move {
                    let now_ms = Utc::now().timestamp_millis();
                    // OKX backwards semantics: 'after' is used for backward pagination
                    let after_trade_id = params.get("after").and_then(|s| s.parse::<i64>().ok());

                    let data = if let Some(after_id) = after_trade_id {
                        if after_id == 3102 {
                            // Return 2 trades within 1.5-2.5 hour historical range
                            let historical_ms = now_ms - (2 * 3600 * 1000) - (10 * 60 * 1000);
                            vec![
                                json!({
                                    "instId": "BTC-USD",
                                    "side": "buy",
                                    "sz": "0.01",
                                    "px": "100000.0",
                                    "tradeId": "3000",
                                    "ts": (historical_ms + 1000).to_string(),
                                }),
                                json!({
                                    "instId": "BTC-USD",
                                    "side": "sell",
                                    "sz": "0.01",
                                    "px": "100000.0",
                                    "tradeId": "2999",
                                    "ts": historical_ms.to_string(),
                                }),
                            ]
                        } else if after_id < 3102 {
                            vec![]
                        } else {
                            let mut trades = Vec::new();

                            for i in 0..100 {
                                let trade_id = after_id - i - 1;
                                if trade_id < 3102 {
                                    break;
                                }
                                trades.push(json!({
                                    "instId": "BTC-USD",
                                    "side": if i % 2 == 0 { "buy" } else { "sell" },
                                    "sz": "0.01",
                                    "px": "100000.0",
                                    "tradeId": trade_id.to_string(),
                                    "ts": (now_ms - ((trade_id - 3100) * 10)).to_string(),
                                }));
                            }
                            trades
                        }
                    } else {
                        vec![
                            json!({
                                "instId": "BTC-USD",
                                "side": "buy",
                                "sz": "0.01",
                                "px": "100000.0",
                                "tradeId": "3203",
                                "ts": (now_ms - 1000).to_string(),
                            }),
                            json!({
                                "instId": "BTC-USD",
                                "side": "sell",
                                "sz": "0.01",
                                "px": "100000.0",
                                "tradeId": "3202",
                                "ts": (now_ms - 2000).to_string(),
                            }),
                        ]
                    };

                    Json(json!({
                        "code": "0",
                        "msg": "",
                        "data": data,
                    }))
                }
            }),
        );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        axum::serve(listener, router.into_make_service())
            .await
            .unwrap();
    });

    wait_for_server(addr, "/api/v5/public/instruments").await;

    let base_url = format!("http://{addr}");
    let client = OKXHttpClient::new(
        Some(base_url),
        60,
        3,
        1000,
        10_000,
        OKXEnvironment::Live,
        None,
    )
    .unwrap();

    for instrument in load_instruments_any() {
        client.cache_instrument(instrument);
    }

    // Request trades from 2.5 hours ago to 1.5 hours ago
    let now = Utc::now();
    let start = now - ChronoDuration::milliseconds(2 * 3600 * 1000 + 1800 * 1000);
    let end = now - ChronoDuration::milliseconds(3600 * 1000 + 1800 * 1000);

    let trades = client
        .request_trades(
            InstrumentId::from("BTC-USD.OKX"),
            Some(start),
            Some(end),
            None,
        )
        .await
        .unwrap();

    assert_eq!(
        trades.len(),
        2,
        "Should retrieve trades after paginating through filtered pages"
    );

    for i in 1..trades.len() {
        assert!(trades[i].ts_event >= trades[i - 1].ts_event);
    }
}
#[tokio::test]
async fn test_request_trades_multiple_trades_same_id() {
    // Regression test: When multiple trades share the same trade ID (e.g., block trades
    // or trades at the same millisecond), pagination cursor must use the deduplicated
    // trade ID to avoid re-fetching the same trades and getting stuck in a loop

    let call_count = Arc::new(AtomicUsize::new(0));
    let call_count_clone = Arc::clone(&call_count);

    let router = Router::new()
        .route(
            "/api/v5/public/instruments",
            get({
                move || async move {
                    Json(load_test_data("http_get_instruments_spot.json")).into_response()
                }
            }),
        )
        .route(
            "/api/v5/market/history-trades",
            get({
                move |Query(params): Query<HashMap<String, String>>| async move {
                    call_count_clone.fetch_add(1, Ordering::SeqCst);

                    // OKX backwards semantics: 'after' is used for backward pagination
                    let after_id = params.get("after");
                    let now_ms = Utc::now().timestamp_millis();

                    let data = if after_id.is_none() {
                        vec![
                            json!({"instId": "BTC-USD", "side": "buy", "sz": "1.0", "px": "50000.0", "tradeId": "1005", "ts": (now_ms - 5000).to_string()}),
                            json!({"instId": "BTC-USD", "side": "sell", "sz": "2.0", "px": "50001.0", "tradeId": "1004", "ts": (now_ms - 6000).to_string()}),
                            json!({"instId": "BTC-USD", "side": "buy", "sz": "0.5", "px": "50002.0", "tradeId": "1003", "ts": (now_ms - 7000).to_string()}),
                            json!({"instId": "BTC-USD", "side": "sell", "sz": "0.3", "px": "50003.0", "tradeId": "1003", "ts": (now_ms - 8000).to_string()}),
                            json!({"instId": "BTC-USD", "side": "buy", "sz": "0.2", "px": "50004.0", "tradeId": "1003", "ts": (now_ms - 9000).to_string()}),
                        ]
                    } else if after_id == Some(&"1003".to_string()) {
                        vec![
                            json!({"instId": "BTC-USD", "side": "sell", "sz": "1.5", "px": "49999.0", "tradeId": "1002", "ts": (now_ms - 10000).to_string()}),
                            json!({"instId": "BTC-USD", "side": "buy", "sz": "0.8", "px": "49998.0", "tradeId": "1001", "ts": (now_ms - 11000).to_string()}),
                        ]
                    } else {
                        vec![]
                    };

                    Json(json!({
                        "code": "0",
                        "msg": "",
                        "data": data,
                    }))
                }
            }),
        );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        axum::serve(listener, router.into_make_service())
            .await
            .unwrap();
    });

    let base_url = format!("http://{addr}");
    let client = OKXHttpClient::new(
        Some(base_url),
        60,
        3,
        1000,
        10_000,
        OKXEnvironment::Live,
        None,
    )
    .unwrap();

    for instrument in load_instruments_any() {
        client.cache_instrument(instrument);
    }

    // Request with time range to trigger multi-page pagination
    let start = Utc::now() - ChronoDuration::hours(1);
    let end = Utc::now();
    let trades = client
        .request_trades(
            InstrumentId::from("BTC-USD.OKX"),
            Some(start),
            Some(end),
            None,
        )
        .await
        .unwrap();

    // Should get 7 unique trades (5 from page 1 + 2 from page 2)
    // Even though 3 trades on page 1 share trade ID "1003"
    assert_eq!(trades.len(), 7, "Should collect all trades from both pages");
    assert_eq!(
        call_count.load(Ordering::SeqCst),
        3,
        "Should make 3 API calls (page 1, page 2, and empty confirmation)"
    );

    // Verify chronological order
    for i in 1..trades.len() {
        assert!(
            trades[i].ts_event >= trades[i - 1].ts_event,
            "Trades should be in chronological order"
        );
    }

    // Verify the 3 trades with same ID are all present (different timestamps)
    let id_1003_count = trades
        .iter()
        .filter(|t| t.trade_id.to_string() == "1003")
        .count();
    assert_eq!(id_1003_count, 3, "Should have all 3 trades with ID 1003");
}

#[rstest]
#[tokio::test]
async fn test_http_get_order_algo_pending_requires_credentials() {
    let addr = start_test_server(Arc::new(TestServerState::default())).await;
    let base_url = format!("http://{addr}");

    let client = OKXRawHttpClient::new(
        Some(base_url),
        60,
        3,
        1000,
        10_000,
        OKXEnvironment::Live,
        None,
    )
    .unwrap();

    let params = GetAlgoOrdersParamsBuilder::default()
        .inst_type(OKXInstrumentType::Swap)
        .build()
        .unwrap();

    let result = client.get_order_algo_pending(params).await;

    match result {
        Err(OKXHttpError::MissingCredentials) => {}
        other => panic!("expected MissingCredentials error: {other:?}"),
    }
}

#[rstest]
#[tokio::test]
async fn test_http_get_order_algo_pending_returns_data() {
    let addr = start_test_server(Arc::new(TestServerState::default())).await;
    let base_url = format!("http://{addr}");

    let params = GetAlgoOrdersParamsBuilder::default()
        .inst_type(OKXInstrumentType::Swap)
        .build()
        .unwrap();
    let client = OKXRawHttpClient::with_credentials(
        "test_key".to_string(),
        "test_secret".to_string(),
        "test_passphrase".to_string(),
        base_url,
        60,
        3,
        1000,
        10_000,
        OKXEnvironment::Live,
        None,
    )
    .unwrap();

    let orders = client.get_order_algo_pending(params).await.unwrap();

    assert!(!orders.is_empty());
    assert_eq!(orders[0].algo_id, "123456789");
    assert_eq!(orders[0].inst_type, OKXInstrumentType::Swap);
}

#[rstest]
#[tokio::test]
async fn test_http_get_order_algo_history_returns_data() {
    let addr = start_test_server(Arc::new(TestServerState::default())).await;
    let base_url = format!("http://{addr}");

    let params = GetAlgoOrdersParamsBuilder::default()
        .inst_type(OKXInstrumentType::Swap)
        .build()
        .unwrap();
    let client = OKXRawHttpClient::with_credentials(
        "test_key".to_string(),
        "test_secret".to_string(),
        "test_passphrase".to_string(),
        base_url,
        60,
        3,
        1000,
        10_000,
        OKXEnvironment::Live,
        None,
    )
    .unwrap();

    let orders = client.get_order_algo_history(params).await.unwrap();

    assert!(!orders.is_empty());
    assert_eq!(orders[0].algo_id, "987654321");
    assert_eq!(orders[0].state, OKXOrderStatus::Effective);
}

#[rstest]
#[tokio::test]
async fn test_http_request_algo_order_status_report_parses_close_fraction_conditional_order() {
    let addr = start_test_server(Arc::new(TestServerState::default())).await;
    let base_url = format!("http://{addr}");

    let swap_instruments = load_swap_instruments_any();
    let size_precision = swap_instruments
        .iter()
        .find(|instrument| instrument.id() == InstrumentId::from("BTC-USDT-SWAP.OKX"))
        .expect("expected BTC-USDT-SWAP instrument")
        .size_precision();

    let client = OKXHttpClient::with_credentials(
        Some("test_key".to_string()),
        Some("test_secret".to_string()),
        Some("test_passphrase".to_string()),
        Some(base_url),
        60,
        3,
        1000,
        10_000,
        OKXEnvironment::Live,
        None,
    )
    .unwrap();

    for instrument in swap_instruments {
        client.cache_instrument(instrument);
    }

    let report = client
        .request_algo_order_status_report(
            AccountId::new("OKX-001"),
            InstrumentId::from("BTC-USDT-SWAP.OKX"),
            ClientOrderId::from("O-close-frac-status"),
        )
        .await
        .unwrap()
        .expect("expected algo report");

    assert_eq!(report.order_type, OrderType::StopMarket);
    assert_eq!(report.trigger_price, Some(Price::from("50000")));
    assert_eq!(report.trigger_type, Some(TriggerType::LastPrice));
    assert_eq!(report.price, None);
    assert_eq!(report.quantity, Quantity::zero(size_precision));
    assert!(report.reduce_only);
}

#[rstest]
#[tokio::test]
async fn test_http_request_algo_order_status_report_queries_attached_oco_with_ord_type() {
    let state = Arc::new(TestServerState::default());
    let addr = start_test_server(state.clone()).await;
    let base_url = format!("http://{addr}");

    let client = OKXHttpClient::with_credentials(
        Some("test_key".to_string()),
        Some("test_secret".to_string()),
        Some("test_passphrase".to_string()),
        Some(base_url),
        60,
        3,
        1000,
        10_000,
        OKXEnvironment::Live,
        None,
    )
    .unwrap();

    for instrument in load_swap_instruments_any() {
        client.cache_instrument(instrument);
    }

    let report = client
        .request_algo_order_status_report(
            AccountId::new("OKX-001"),
            InstrumentId::from("ETH-USDT-SWAP.OKX"),
            ClientOrderId::from("O-attached-oco"),
        )
        .await
        .unwrap()
        .expect("expected attached OCO algo report");

    assert_eq!(
        report.client_order_id,
        Some(ClientOrderId::from("O-attached-oco"))
    );
    assert!(report.trigger_price.is_some());

    let pending_queries = state.algo_pending_queries.lock().await.clone();
    assert!(
        pending_queries
            .iter()
            .any(|query| query.get("ordType").map(String::as_str) == Some("oco")),
        "expected at least one pending algo query with ordType=oco, found {pending_queries:?}",
    );
}

#[rstest]
#[tokio::test]
async fn test_http_request_order_status_reports_preserves_attached_tp_sl_child_ids() {
    let addr = start_test_server(Arc::new(TestServerState::default())).await;
    let base_url = format!("http://{addr}");

    let client = OKXHttpClient::with_credentials(
        Some("test_key".to_string()),
        Some("test_secret".to_string()),
        Some("test_passphrase".to_string()),
        Some(base_url),
        60,
        3,
        1000,
        10_000,
        OKXEnvironment::Live,
        None,
    )
    .unwrap();

    for instrument in load_swap_instruments_any() {
        client.cache_instrument(instrument);
    }

    let reports = client
        .request_order_status_reports(
            AccountId::new("OKX-001"),
            Some(OKXInstrumentType::Swap),
            Some(InstrumentId::from("ETH-USDT-SWAP.OKX")),
            None,
            None,
            true,
            Some(50),
        )
        .await
        .unwrap();

    let report = reports
        .into_iter()
        .find(|report| report.client_order_id == Some(ClientOrderId::from("O-attached-entry")))
        .expect("expected attached entry report");

    let linked_order_ids = report
        .linked_order_ids
        .expect("expected linked child order ids");
    assert!(linked_order_ids.contains(&ClientOrderId::from("O-attached-sl")));
    assert!(linked_order_ids.contains(&ClientOrderId::from("O-attached-tp")));
}

#[rstest]
#[tokio::test]
async fn test_http_place_algo_order_with_close_fraction_uses_conditional_close_order_payload() {
    let state = Arc::new(TestServerState::default());
    let addr = start_test_server(state.clone()).await;
    let base_url = format!("http://{addr}");

    let client = OKXHttpClient::with_credentials(
        Some("test_key".to_string()),
        Some("test_secret".to_string()),
        Some("test_passphrase".to_string()),
        Some(base_url),
        60,
        3,
        1000,
        10_000,
        OKXEnvironment::Live,
        None,
    )
    .unwrap();

    for instrument in load_swap_instruments_any() {
        client.cache_instrument(instrument);
    }

    let response = client
        .place_algo_order_with_domain_types(
            InstrumentId::from("BTC-USDT-SWAP.OKX"),
            OKXTradeMode::Cross,
            ClientOrderId::from("O-close-frac"),
            OrderSide::Sell,
            OrderType::StopMarket,
            Quantity::from("0.01"),
            Some(Price::from("50000")),
            Some(TriggerType::Default),
            None,
            None,
            Some("1".to_string()),
            None,
            None,
            None,
        )
        .await
        .unwrap();

    assert_eq!(response.algo_id, "12345");

    let body = state
        .last_algo_order_body
        .lock()
        .await
        .clone()
        .expect("expected algo order payload");

    assert_eq!(body["ordType"], "conditional");
    assert_eq!(body["closeFraction"], "1");
    assert_eq!(body["reduceOnly"], true);
    assert_eq!(body["posSide"], "net");
    assert_eq!(body["slTriggerPx"], "50000");
    assert_eq!(body["slOrdPx"], "-1");
    assert_eq!(body["slTriggerPxType"], "last");
    assert!(body.get("sz").is_none());
    assert!(body.get("triggerPx").is_none());
    assert!(body.get("orderPx").is_none());
}

#[rstest]
#[tokio::test]
async fn test_http_place_order_with_attached_tp_sl_uses_single_oco_payload() {
    let state = Arc::new(TestServerState::default());
    let addr = start_test_server(state.clone()).await;
    let base_url = format!("http://{addr}");

    let client = OKXHttpClient::with_credentials(
        Some("test_key".to_string()),
        Some("test_secret".to_string()),
        Some("test_passphrase".to_string()),
        Some(base_url),
        60,
        3,
        1000,
        10_000,
        OKXEnvironment::Live,
        None,
    )
    .unwrap();

    for instrument in load_swap_instruments_any() {
        client.cache_instrument(instrument);
    }

    let response = client
        .place_order_with_domain_types(
            InstrumentId::from("ETH-USDT-SWAP.OKX"),
            OKXTradeMode::Cross,
            ClientOrderId::from("O-bracket-entry"),
            OrderSide::Buy,
            OrderType::Market,
            Quantity::from("0.01"),
            None,
            None,
            None,
            None,
            None,
            None,
            Some(vec![OKXAttachAlgoOrdRequest {
                attach_algo_cl_ord_id: Some("O-bracket-sl".to_string()),
                sl_trigger_px: Some("39000.00".to_string()),
                sl_ord_px: Some("-1".to_string()),
                sl_trigger_px_type: Some(OKXTriggerType::Last),
                tp_trigger_px: Some("41000.00".to_string()),
                tp_ord_px: Some("-1".to_string()),
                tp_trigger_px_type: Some(OKXTriggerType::Last),
            }]),
            None,
            None,
        )
        .await
        .unwrap();

    assert_eq!(response.ord_id, Some(Ustr::from("12345")));

    let body = state
        .last_order_body
        .lock()
        .await
        .clone()
        .expect("expected order payload");

    assert_eq!(body["instId"], "ETH-USDT-SWAP");
    assert_eq!(body["tdMode"], "cross");
    assert_eq!(body["clOrdId"], "O-bracket-entry");
    assert_eq!(body["side"], "buy");
    assert_eq!(body["ordType"], "market");
    assert_eq!(body["sz"], "0.01");

    let attach_algo_ords = body["attachAlgoOrds"]
        .as_array()
        .expect("expected attachAlgoOrds array");
    assert_eq!(attach_algo_ords.len(), 1);
    assert_eq!(attach_algo_ords[0]["attachAlgoClOrdId"], "O-bracket-sl");
    assert_eq!(attach_algo_ords[0]["slTriggerPx"], "39000.00");
    assert_eq!(attach_algo_ords[0]["slOrdPx"], "-1");
    assert_eq!(attach_algo_ords[0]["tpTriggerPx"], "41000.00");
    assert_eq!(attach_algo_ords[0]["tpOrdPx"], "-1");
}

// Note: place_algo_order and cancel_algo_order are on OKXHttpClient (not Raw),
// and will be tested via WebSocket client tests instead.

#[rstest]
#[tokio::test]
async fn test_http_set_position_mode_requires_credentials() {
    let addr = start_test_server(Arc::new(TestServerState::default())).await;
    let base_url = format!("http://{addr}");

    let client = OKXRawHttpClient::new(
        Some(base_url),
        60,
        3,
        1000,
        10_000,
        OKXEnvironment::Live,
        None,
    )
    .unwrap();

    let params = SetPositionModeParamsBuilder::default()
        .pos_mode(OKXPositionMode::LongShortMode)
        .build()
        .unwrap();

    let result = client.set_position_mode(params).await;

    match result {
        Err(OKXHttpError::MissingCredentials) => {}
        other => panic!("expected MissingCredentials error: {other:?}"),
    }
}

#[rstest]
#[tokio::test]
async fn test_http_set_position_mode_returns_response() {
    let addr = start_test_server(Arc::new(TestServerState::default())).await;
    let base_url = format!("http://{addr}");

    let client = OKXRawHttpClient::with_credentials(
        "test_key".to_string(),
        "test_secret".to_string(),
        "test_passphrase".to_string(),
        base_url,
        60,
        3,
        1000,
        10_000,
        OKXEnvironment::Live,
        None,
    )
    .unwrap();

    let params = SetPositionModeParamsBuilder::default()
        .pos_mode(OKXPositionMode::LongShortMode)
        .build()
        .unwrap();

    let response = client.set_position_mode(params).await.unwrap();

    assert!(!response.is_empty());
}

#[rstest]
#[tokio::test]
async fn test_http_get_position_tiers_returns_data() {
    let addr = start_test_server(Arc::new(TestServerState::default())).await;
    let base_url = format!("http://{addr}");

    let params = GetPositionTiersParamsBuilder::default()
        .inst_type(OKXInstrumentType::Spot)
        .inst_id("BTC-USDT")
        .build()
        .unwrap();
    let client = OKXRawHttpClient::with_credentials(
        "test_key".to_string(),
        "test_secret".to_string(),
        "test_passphrase".to_string(),
        base_url,
        60,
        3,
        1000,
        10_000,
        OKXEnvironment::Live,
        None,
    )
    .unwrap();

    let tiers = client.get_position_tiers(params).await.unwrap();

    assert!(!tiers.is_empty());
    assert_eq!(tiers[0].inst_id, Ustr::from("BTC-USDT"));
}

#[rstest]
#[tokio::test]
async fn test_http_get_trade_fee_requires_credentials() {
    let addr = start_test_server(Arc::new(TestServerState::default())).await;
    let base_url = format!("http://{addr}");

    let client = OKXRawHttpClient::new(
        Some(base_url),
        60,
        3,
        1000,
        10_000,
        OKXEnvironment::Live,
        None,
    )
    .unwrap();

    let params = GetTradeFeeParamsBuilder::default()
        .inst_type(OKXInstrumentType::Spot)
        .uly("")
        .inst_family("")
        .build()
        .unwrap();

    let result = client.get_trade_fee(params).await;

    match result {
        Err(OKXHttpError::MissingCredentials) => {}
        other => panic!("expected MissingCredentials error: {other:?}"),
    }
}

#[rstest]
#[tokio::test]
async fn test_http_get_trade_fee_returns_data() {
    let addr = start_test_server(Arc::new(TestServerState::default())).await;
    let base_url = format!("http://{addr}");

    let params = GetTradeFeeParamsBuilder::default()
        .inst_type(OKXInstrumentType::Spot)
        .uly("")
        .inst_family("")
        .build()
        .unwrap();
    let client = OKXRawHttpClient::with_credentials(
        "test_key".to_string(),
        "test_secret".to_string(),
        "test_passphrase".to_string(),
        base_url,
        60,
        3,
        1000,
        10_000,
        OKXEnvironment::Live,
        None,
    )
    .unwrap();

    let fees = client.get_trade_fee(params).await.unwrap();

    assert!(!fees.is_empty());
    assert_eq!(fees[0].inst_type, OKXInstrumentType::Spot);
}

#[rstest]
#[tokio::test]
async fn test_http_get_positions_requires_credentials() {
    let addr = start_test_server(Arc::new(TestServerState::default())).await;
    let base_url = format!("http://{addr}");

    let client = OKXRawHttpClient::new(
        Some(base_url),
        60,
        3,
        1000,
        10_000,
        OKXEnvironment::Live,
        None,
    )
    .unwrap();

    let params = GetPositionsParamsBuilder::default().build().unwrap();

    let result = client.get_positions(params).await;

    match result {
        Err(OKXHttpError::MissingCredentials) => {}
        other => panic!("expected MissingCredentials error: {other:?}"),
    }
}

#[rstest]
#[tokio::test]
async fn test_http_get_positions_returns_data() {
    let addr = start_test_server(Arc::new(TestServerState::default())).await;
    let base_url = format!("http://{addr}");

    let params = GetPositionsParamsBuilder::default().build().unwrap();
    let client = OKXRawHttpClient::with_credentials(
        "test_key".to_string(),
        "test_secret".to_string(),
        "test_passphrase".to_string(),
        base_url,
        60,
        3,
        1000,
        10_000,
        OKXEnvironment::Live,
        None,
    )
    .unwrap();

    let positions = client.get_positions(params).await.unwrap();

    assert!(!positions.is_empty());
    assert_eq!(positions[0].inst_id, Ustr::from("BTC-USDT-SWAP"));
}

#[rstest]
#[tokio::test]
async fn test_http_get_fills_requires_credentials() {
    let addr = start_test_server(Arc::new(TestServerState::default())).await;
    let base_url = format!("http://{addr}");

    let client = OKXRawHttpClient::new(
        Some(base_url),
        60,
        3,
        1000,
        10_000,
        OKXEnvironment::Live,
        None,
    )
    .unwrap();

    let params = GetTransactionDetailsParamsBuilder::default()
        .build()
        .unwrap();

    let result = client.get_fills(params).await;

    match result {
        Err(OKXHttpError::MissingCredentials) => {}
        other => panic!("expected MissingCredentials error: {other:?}"),
    }
}

#[rstest]
#[tokio::test]
async fn test_http_get_fills_returns_data() {
    let addr = start_test_server(Arc::new(TestServerState::default())).await;
    let base_url = format!("http://{addr}");

    let params = GetTransactionDetailsParamsBuilder::default()
        .build()
        .unwrap();
    let client = OKXRawHttpClient::with_credentials(
        "test_key".to_string(),
        "test_secret".to_string(),
        "test_passphrase".to_string(),
        base_url,
        60,
        3,
        1000,
        10_000,
        OKXEnvironment::Live,
        None,
    )
    .unwrap();

    let fills = client.get_fills(params).await.unwrap();

    assert!(!fills.is_empty());
}

// Error Handling Tests

#[rstest]
#[tokio::test]
async fn test_http_network_error_invalid_port() {
    let base_url = "http://127.0.0.1:1".to_string();

    let client = OKXRawHttpClient::new(
        Some(base_url),
        1,
        0,
        1000,
        10_000,
        OKXEnvironment::Live,
        None,
    )
    .unwrap();

    let params = GetInstrumentsParamsBuilder::default()
        .inst_type(OKXInstrumentType::Spot)
        .build()
        .unwrap();

    let result = client.get_instruments(params).await;

    assert!(result.is_err());
    match result {
        Err(OKXHttpError::HttpClientError(_)) => {}
        other => panic!("expected HttpClientError: {other:?}"),
    }
}

#[rstest]
#[tokio::test]
async fn test_http_okx_error_response() {
    let router = Router::new().route(
        "/api/v5/public/instruments",
        get(|| async {
            (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "code": "51000",
                    "msg": "Parameter instType can not be empty",
                    "data": [],
                })),
            )
        }),
    );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        axum::serve(listener, router.into_make_service())
            .await
            .unwrap();
    });

    wait_for_server(addr, "/api/v5/public/instruments").await;

    let base_url = format!("http://{addr}");
    let client = OKXRawHttpClient::new(
        Some(base_url),
        60,
        3,
        1000,
        10_000,
        OKXEnvironment::Live,
        None,
    )
    .unwrap();

    let params = GetInstrumentsParamsBuilder::default()
        .inst_type(OKXInstrumentType::Spot)
        .build()
        .unwrap();

    let result = client.get_instruments(params).await;

    assert!(result.is_err());
    match result {
        Err(OKXHttpError::OkxError {
            error_code,
            message,
        }) => {
            assert_eq!(error_code, "51000");
            assert!(message.contains("instType"));
        }
        other => panic!("expected OkxError: {other:?}"),
    }
}

#[rstest]
#[tokio::test]
async fn test_http_okx_error_falls_back_to_s_msg_on_http_200() {
    let router = Router::new().route(
        "/api/v5/account/set-position-mode",
        post(|headers: HeaderMap| async move {
            if !has_auth_headers(&headers) {
                return (
                    StatusCode::UNAUTHORIZED,
                    Json(json!({
                        "code": "401",
                        "msg": "Missing authentication headers",
                        "data": [],
                    })),
                )
                    .into_response();
            }

            Json(json!({
                "code": "1",
                "msg": "",
                "data": [{
                    "sCode": "51000",
                    "sMsg": "Parameter triggerPx error",
                }],
            }))
            .into_response()
        }),
    );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        axum::serve(listener, router.into_make_service())
            .await
            .unwrap();
    });

    wait_for_server(addr, "/api/v5/account/set-position-mode").await;

    let base_url = format!("http://{addr}");
    let client = OKXRawHttpClient::with_credentials(
        "test_key".to_string(),
        "test_secret".to_string(),
        "test_passphrase".to_string(),
        base_url,
        60,
        3,
        1000,
        10_000,
        OKXEnvironment::Live,
        None,
    )
    .unwrap();

    let params = SetPositionModeParamsBuilder::default()
        .pos_mode(OKXPositionMode::LongShortMode)
        .build()
        .unwrap();
    let result = client.set_position_mode(params).await;

    match result {
        Err(OKXHttpError::OkxError {
            error_code,
            message,
        }) => {
            assert_eq!(error_code, "1");
            assert_eq!(message, "Parameter triggerPx error");
        }
        other => panic!("expected OkxError: {other:?}"),
    }
}

#[rstest]
#[tokio::test]
async fn test_http_okx_error_falls_back_to_s_msg_on_http_400() {
    let router = Router::new().route(
        "/api/v5/account/set-position-mode",
        post(|headers: HeaderMap| async move {
            if !has_auth_headers(&headers) {
                return (
                    StatusCode::UNAUTHORIZED,
                    Json(json!({
                        "code": "401",
                        "msg": "Missing authentication headers",
                        "data": [],
                    })),
                )
                    .into_response();
            }

            (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "code": "1",
                    "msg": "",
                    "data": [{
                        "sCode": "51000",
                        "sMsg": "Parameter triggerPx error",
                    }],
                })),
            )
                .into_response()
        }),
    );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        axum::serve(listener, router.into_make_service())
            .await
            .unwrap();
    });

    wait_for_server(addr, "/api/v5/account/set-position-mode").await;

    let base_url = format!("http://{addr}");
    let client = OKXRawHttpClient::with_credentials(
        "test_key".to_string(),
        "test_secret".to_string(),
        "test_passphrase".to_string(),
        base_url,
        60,
        3,
        1000,
        10_000,
        OKXEnvironment::Live,
        None,
    )
    .unwrap();

    let params = SetPositionModeParamsBuilder::default()
        .pos_mode(OKXPositionMode::LongShortMode)
        .build()
        .unwrap();
    let result = client.set_position_mode(params).await;

    match result {
        Err(OKXHttpError::OkxError {
            error_code,
            message,
        }) => {
            assert_eq!(error_code, "1");
            assert_eq!(message, "Parameter triggerPx error");
        }
        other => panic!("expected OkxError: {other:?}"),
    }
}

#[rstest]
#[tokio::test]
async fn test_http_okx_error_falls_back_to_s_code_when_s_msg_empty() {
    let router = Router::new().route(
        "/api/v5/account/set-position-mode",
        post(|headers: HeaderMap| async move {
            if !has_auth_headers(&headers) {
                return (
                    StatusCode::UNAUTHORIZED,
                    Json(json!({
                        "code": "401",
                        "msg": "Missing authentication headers",
                        "data": [],
                    })),
                )
                    .into_response();
            }

            Json(json!({
                "code": "1",
                "msg": "",
                "data": [{
                    "sCode": "51008",
                    "sMsg": "",
                }],
            }))
            .into_response()
        }),
    );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        axum::serve(listener, router.into_make_service())
            .await
            .unwrap();
    });

    wait_for_server(addr, "/api/v5/account/set-position-mode").await;

    let base_url = format!("http://{addr}");
    let client = OKXRawHttpClient::with_credentials(
        "test_key".to_string(),
        "test_secret".to_string(),
        "test_passphrase".to_string(),
        base_url,
        60,
        3,
        1000,
        10_000,
        OKXEnvironment::Live,
        None,
    )
    .unwrap();

    let params = SetPositionModeParamsBuilder::default()
        .pos_mode(OKXPositionMode::LongShortMode)
        .build()
        .unwrap();
    let result = client.set_position_mode(params).await;

    match result {
        Err(OKXHttpError::OkxError {
            error_code,
            message,
        }) => {
            assert_eq!(error_code, "1");
            assert_eq!(message, "51008");
        }
        other => panic!("expected OkxError: {other:?}"),
    }
}

#[rstest]
#[tokio::test]
async fn test_http_malformed_json_response() {
    let router = Router::new().route(
        "/api/v5/public/instruments",
        get(|| async { "not valid json" }),
    );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        axum::serve(listener, router.into_make_service())
            .await
            .unwrap();
    });

    wait_for_server(addr, "/api/v5/public/instruments").await;

    let base_url = format!("http://{addr}");
    let client = OKXRawHttpClient::new(
        Some(base_url),
        60,
        3,
        1000,
        10_000,
        OKXEnvironment::Live,
        None,
    )
    .unwrap();

    let params = GetInstrumentsParamsBuilder::default()
        .inst_type(OKXInstrumentType::Spot)
        .build()
        .unwrap();

    let result = client.get_instruments(params).await;

    assert!(result.is_err());
    match result {
        Err(OKXHttpError::JsonError(_)) => {}
        other => panic!("expected JsonError: {other:?}"),
    }
}

#[rstest]
#[tokio::test]
async fn test_http_500_internal_server_error() {
    let router = Router::new().route(
        "/api/v5/public/instruments",
        get(|| async {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "code": "50000",
                    "msg": "Internal server error",
                    "data": [],
                })),
            )
        }),
    );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        axum::serve(listener, router.into_make_service())
            .await
            .unwrap();
    });

    wait_for_server(addr, "/api/v5/public/instruments").await;

    let base_url = format!("http://{addr}");
    let client = OKXRawHttpClient::new(
        Some(base_url),
        60,
        3,
        1000,
        10_000,
        OKXEnvironment::Live,
        None,
    )
    .unwrap();

    let params = GetInstrumentsParamsBuilder::default()
        .inst_type(OKXInstrumentType::Spot)
        .build()
        .unwrap();

    let result = client.get_instruments(params).await;

    assert!(result.is_err());
    match result {
        Err(OKXHttpError::OkxError { error_code, .. }) => {
            assert_eq!(error_code, "50000");
        }
        other => panic!("expected OkxError: {other:?}"),
    }
}

#[rstest]
#[tokio::test]
async fn test_http_503_service_unavailable() {
    let router = Router::new().route(
        "/api/v5/public/instruments",
        get(|| async {
            (
                StatusCode::SERVICE_UNAVAILABLE,
                "Service temporarily unavailable",
            )
        }),
    );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        axum::serve(listener, router.into_make_service())
            .await
            .unwrap();
    });

    wait_for_server(addr, "/api/v5/public/instruments").await;

    let base_url = format!("http://{addr}");
    let client = OKXRawHttpClient::new(
        Some(base_url),
        60,
        0,
        1000,
        10_000,
        OKXEnvironment::Live,
        None,
    )
    .unwrap();

    let params = GetInstrumentsParamsBuilder::default()
        .inst_type(OKXInstrumentType::Spot)
        .build()
        .unwrap();

    let result = client.get_instruments(params).await;

    assert!(result.is_err());
    match result {
        Err(OKXHttpError::UnexpectedStatus { status, .. }) => {
            assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        }
        other => panic!("expected UnexpectedStatus: {other:?}"),
    }
}

#[rstest]
#[tokio::test]
async fn test_http_invalid_response_structure() {
    let router = Router::new().route(
        "/api/v5/public/instruments",
        get(|| async {
            Json(json!({
                "code": "0",
                "msg": "",
                "data": [
                    {
                        "instId": "BTC-USDT",
                        "missing_required_field": "value"
                    }
                ],
            }))
        }),
    );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        axum::serve(listener, router.into_make_service())
            .await
            .unwrap();
    });

    wait_for_server(addr, "/api/v5/public/instruments").await;

    let base_url = format!("http://{addr}");
    let client = OKXRawHttpClient::new(
        Some(base_url),
        60,
        3,
        1000,
        10_000,
        OKXEnvironment::Live,
        None,
    )
    .unwrap();

    let params = GetInstrumentsParamsBuilder::default()
        .inst_type(OKXInstrumentType::Spot)
        .build()
        .unwrap();

    let result = client.get_instruments(params).await;

    assert!(result.is_err());
    match result {
        Err(OKXHttpError::JsonError(msg)) => {
            assert!(msg.contains("missing field") || msg.contains("UninitializedField"));
        }
        other => panic!("expected JsonError: {other:?}"),
    }
}

#[rstest]
#[tokio::test]
async fn test_http_rate_limit_error_different_code() {
    let router = Router::new().route(
        "/api/v5/account/balance",
        get(|| async {
            (
                StatusCode::TOO_MANY_REQUESTS,
                Json(json!({
                    "code": "50011",
                    "msg": "Request too frequent",
                    "data": [],
                })),
            )
        }),
    );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        axum::serve(listener, router.into_make_service())
            .await
            .unwrap();
    });

    wait_for_server(addr, "/api/v5/public/instruments").await;

    let base_url = format!("http://{addr}");
    let client = OKXRawHttpClient::with_credentials(
        "test_key".to_string(),
        "test_secret".to_string(),
        "test_passphrase".to_string(),
        base_url,
        60,
        0,
        1000,
        10_000,
        OKXEnvironment::Live,
        None,
    )
    .unwrap();

    let result = client.get_balance().await;

    assert!(result.is_err());
    match result {
        Err(OKXHttpError::OkxError {
            error_code,
            message,
        }) => {
            assert_eq!(error_code, "50011");
            assert!(message.contains("frequent"));
        }
        other => panic!("expected OkxError: {other:?}"),
    }
}

#[rstest]
#[tokio::test]
async fn test_http_empty_response_data() {
    let router = Router::new().route(
        "/api/v5/public/instruments",
        get(|| async {
            Json(json!({
                "code": "0",
                "msg": "",
                "data": [],
            }))
        }),
    );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        axum::serve(listener, router.into_make_service())
            .await
            .unwrap();
    });

    wait_for_server(addr, "/api/v5/public/instruments").await;

    let base_url = format!("http://{addr}");
    let client = OKXRawHttpClient::new(
        Some(base_url),
        60,
        3,
        1000,
        10_000,
        OKXEnvironment::Live,
        None,
    )
    .unwrap();

    let params = GetInstrumentsParamsBuilder::default()
        .inst_type(OKXInstrumentType::Spot)
        .build()
        .unwrap();

    let result = client.get_instruments(params).await.unwrap();

    assert!(result.is_empty());
}

#[tokio::test]
async fn test_request_book_snapshot() {
    let router = Router::new()
        .route(
            "/api/v5/public/instruments",
            get(|| async {
                Json(load_test_data("http_get_instruments_swap.json")).into_response()
            }),
        )
        .route(
            "/api/v5/market/books",
            get(|| async { Json(load_test_data("http_get_order_book.json")).into_response() }),
        );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        axum::serve(listener, router.into_make_service())
            .await
            .unwrap();
    });

    wait_for_server(addr, "/api/v5/public/instruments").await;

    let base_url = format!("http://{addr}");
    let client = OKXHttpClient::new(
        Some(base_url),
        60,
        3,
        1000,
        10_000,
        OKXEnvironment::Live,
        None,
    )
    .unwrap();

    for instrument in load_swap_instruments_any() {
        client.cache_instrument(instrument);
    }

    let book = client
        .request_book_snapshot(InstrumentId::from("BTC-USDT-SWAP.OKX"), Some(5))
        .await
        .unwrap();

    assert_eq!(book.bids(None).count(), 3);
    assert_eq!(book.asks(None).count(), 3);
}

#[tokio::test]
async fn test_request_funding_rates() {
    let router = Router::new()
        .route(
            "/api/v5/public/instruments",
            get(|| async {
                Json(load_test_data("http_get_instruments_swap.json")).into_response()
            }),
        )
        .route(
            "/api/v5/public/funding-rate-history",
            get(|| async {
                Json(load_test_data("http_get_funding_rate_history.json")).into_response()
            }),
        );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        axum::serve(listener, router.into_make_service())
            .await
            .unwrap();
    });

    wait_for_server(addr, "/api/v5/public/instruments").await;

    let base_url = format!("http://{addr}");
    let client = OKXHttpClient::new(
        Some(base_url),
        60,
        3,
        1000,
        10_000,
        OKXEnvironment::Live,
        None,
    )
    .unwrap();

    for instrument in load_swap_instruments_any() {
        client.cache_instrument(instrument);
    }

    let rates = client
        .request_funding_rates(
            InstrumentId::from("BTC-USDT-SWAP.OKX"),
            None,
            None,
            Some(10),
        )
        .await
        .unwrap();

    assert_eq!(rates.len(), 2);
    assert_eq!(
        rates[0].instrument_id,
        InstrumentId::from("BTC-USDT-SWAP.OKX")
    );
    assert_eq!(
        rates[1].instrument_id,
        InstrumentId::from("BTC-USDT-SWAP.OKX")
    );
}

#[rstest]
#[tokio::test]
async fn test_http_get_option_summary_returns_data() {
    let state = Arc::new(TestServerState::default());
    let addr = start_test_server(state.clone()).await;
    let base_url = format!("http://{addr}");
    let client = OKXRawHttpClient::new(
        Some(base_url),
        60,
        3,
        1000,
        10_000,
        OKXEnvironment::Live,
        None,
    )
    .unwrap();

    let params = GetOptionSummaryParamsBuilder::default()
        .inst_family("BTC-USD")
        .exp_time("241217")
        .build()
        .unwrap();
    let summaries = client.get_option_summary(params).await.unwrap();

    assert_eq!(summaries.len(), 4);
    assert_eq!(summaries[0].inst_id, Ustr::from("BTC-USD-241217-92000-C"));

    let queries = state.option_summary_queries.lock().await;
    assert_eq!(queries.len(), 1);
    assert_eq!(queries[0].get("instFamily"), Some(&"BTC-USD".to_string()));
    assert_eq!(queries[0].get("expTime"), Some(&"241217".to_string()));
}

#[rstest]
#[tokio::test]
async fn test_request_forward_prices_for_single_instrument_uses_inst_family_and_exp_time() {
    let state = Arc::new(TestServerState::default());
    let addr = start_test_server(state.clone()).await;
    let base_url = format!("http://{addr}");
    let client = OKXHttpClient::new(
        Some(base_url),
        60,
        3,
        1000,
        10_000,
        OKXEnvironment::Live,
        None,
    )
    .unwrap();

    for instrument in load_option_instruments_any() {
        client.cache_instrument(instrument);
    }

    let instrument_id = InstrumentId::from("BTC-USD-241217-92000-C.OKX");
    let forward_prices = client
        .request_forward_prices("BTC", Some(instrument_id))
        .await
        .unwrap();

    assert_eq!(forward_prices.len(), 1);
    assert_eq!(forward_prices[0].instrument_id, instrument_id);
    assert_eq!(forward_prices[0].forward_price.to_string(), "97000");
    assert_eq!(
        forward_prices[0].underlying_index.as_deref(),
        Some("BTC-USD")
    );

    let queries = state.option_summary_queries.lock().await;
    assert_eq!(queries.len(), 1);
    assert_eq!(queries[0].get("instFamily"), Some(&"BTC-USD".to_string()));
    assert_eq!(queries[0].get("expTime"), Some(&"241217".to_string()));
}

#[rstest]
#[tokio::test]
async fn test_request_forward_prices_bulk_deduplicates_by_expiry() {
    let state = Arc::new(TestServerState::default());
    let addr = start_test_server(state.clone()).await;
    let base_url = format!("http://{addr}");
    let client = OKXHttpClient::new(
        Some(base_url),
        60,
        3,
        1000,
        10_000,
        OKXEnvironment::Live,
        None,
    )
    .unwrap();

    for instrument in load_option_instruments_any() {
        client.cache_instrument(instrument);
    }

    let forward_prices = client.request_forward_prices("BTC", None).await.unwrap();

    assert_eq!(forward_prices.len(), 1);
    assert_eq!(
        forward_prices[0].instrument_id,
        InstrumentId::from("BTC-USD-241217-92000-C.OKX")
    );
    assert_eq!(forward_prices[0].forward_price.to_string(), "97000");

    let queries = state.option_summary_queries.lock().await;
    assert_eq!(queries.len(), 1);
    assert_eq!(queries[0].get("instFamily"), Some(&"BTC-USD".to_string()));
    assert!(!queries[0].contains_key("expTime"));
}

#[rstest]
#[tokio::test]
async fn test_request_forward_prices_errors_on_empty_cache() {
    let state = Arc::new(TestServerState::default());
    let addr = start_test_server(state.clone()).await;
    let base_url = format!("http://{addr}");
    let client = OKXHttpClient::new(
        Some(base_url),
        60,
        3,
        1000,
        10_000,
        OKXEnvironment::Live,
        None,
    )
    .unwrap();

    let result = client.request_forward_prices("BTC", None).await;

    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("No cached OKX option families"),
        "unexpected error: {err}"
    );
}

#[rstest]
#[tokio::test]
async fn test_request_forward_prices_skips_zero_fwd_px() {
    let response = json!({
        "code": "0",
        "msg": "",
        "data": [
            {
                "instType": "OPTION",
                "instId": "BTC-USD-241217-92000-C",
                "uly": "BTC-USD",
                "bidVol": "0.45",
                "askVol": "0.46",
                "markVol": "0.455",
                "fwdPx": "0",
                "ts": "1734166000000"
            },
            {
                "instType": "OPTION",
                "instId": "BTC-USD-241217-94000-C",
                "uly": "BTC-USD",
                "bidVol": "0.45",
                "askVol": "0.46",
                "markVol": "0.455",
                "fwdPx": "97000",
                "ts": "1734166000000"
            }
        ]
    });
    let state = Arc::new(TestServerState::default());
    *state.option_summary_response.lock().await = Some(response);
    let addr = start_test_server(state.clone()).await;
    let base_url = format!("http://{addr}");
    let client = OKXHttpClient::new(
        Some(base_url),
        60,
        3,
        1000,
        10_000,
        OKXEnvironment::Live,
        None,
    )
    .unwrap();

    for instrument in load_option_instruments_any() {
        client.cache_instrument(instrument);
    }

    let instrument_id = InstrumentId::from("BTC-USD-241217-94000-C.OKX");
    let forward_prices = client
        .request_forward_prices("BTC", Some(instrument_id))
        .await
        .unwrap();

    assert_eq!(forward_prices.len(), 1);
    assert_eq!(forward_prices[0].instrument_id, instrument_id);
    assert_eq!(forward_prices[0].forward_price.to_string(), "97000");
}

#[rstest]
#[tokio::test]
async fn test_request_forward_prices_skips_non_option_inst_type() {
    let response = json!({
        "code": "0",
        "msg": "",
        "data": [
            {
                "instType": "SWAP",
                "instId": "BTC-USD-241217-92000-C",
                "uly": "BTC-USD",
                "bidVol": "0.45",
                "askVol": "0.46",
                "markVol": "0.455",
                "fwdPx": "97000",
                "ts": "1734166000000"
            },
            {
                "instType": "OPTION",
                "instId": "BTC-USD-241217-94000-C",
                "uly": "BTC-USD",
                "bidVol": "0.45",
                "askVol": "0.46",
                "markVol": "0.455",
                "fwdPx": "97000",
                "ts": "1734166000000"
            }
        ]
    });
    let state = Arc::new(TestServerState::default());
    *state.option_summary_response.lock().await = Some(response);
    let addr = start_test_server(state.clone()).await;
    let base_url = format!("http://{addr}");
    let client = OKXHttpClient::new(
        Some(base_url),
        60,
        3,
        1000,
        10_000,
        OKXEnvironment::Live,
        None,
    )
    .unwrap();

    for instrument in load_option_instruments_any() {
        client.cache_instrument(instrument);
    }

    let instrument_id = InstrumentId::from("BTC-USD-241217-94000-C.OKX");
    let forward_prices = client
        .request_forward_prices("BTC", Some(instrument_id))
        .await
        .unwrap();

    assert_eq!(forward_prices.len(), 1);
    assert_eq!(forward_prices[0].instrument_id, instrument_id);
}

#[rstest]
#[tokio::test]
async fn test_request_forward_prices_skips_invalid_fwd_px() {
    let response = json!({
        "code": "0",
        "msg": "",
        "data": [
            {
                "instType": "OPTION",
                "instId": "BTC-USD-241217-92000-C",
                "uly": "BTC-USD",
                "bidVol": "0.45",
                "askVol": "0.46",
                "markVol": "0.455",
                "fwdPx": "not_a_number",
                "ts": "1734166000000"
            },
            {
                "instType": "OPTION",
                "instId": "BTC-USD-241217-94000-C",
                "uly": "BTC-USD",
                "bidVol": "0.45",
                "askVol": "0.46",
                "markVol": "0.455",
                "fwdPx": "97000",
                "ts": "1734166000000"
            }
        ]
    });
    let state = Arc::new(TestServerState::default());
    *state.option_summary_response.lock().await = Some(response);
    let addr = start_test_server(state.clone()).await;
    let base_url = format!("http://{addr}");
    let client = OKXHttpClient::new(
        Some(base_url),
        60,
        3,
        1000,
        10_000,
        OKXEnvironment::Live,
        None,
    )
    .unwrap();

    for instrument in load_option_instruments_any() {
        client.cache_instrument(instrument);
    }

    let instrument_id = InstrumentId::from("BTC-USD-241217-94000-C.OKX");
    let forward_prices = client
        .request_forward_prices("BTC", Some(instrument_id))
        .await
        .unwrap();

    assert_eq!(forward_prices.len(), 1);
    assert_eq!(forward_prices[0].instrument_id, instrument_id);
    assert_eq!(forward_prices[0].forward_price.to_string(), "97000");
}

#[rstest]
#[tokio::test]
async fn test_request_forward_prices_bulk_deduplicates_across_multiple_expiries() {
    let response = json!({
        "code": "0",
        "msg": "",
        "data": [
            {
                "instType": "OPTION",
                "instId": "BTC-USD-241217-92000-C",
                "uly": "BTC-USD",
                "bidVol": "0.45",
                "askVol": "0.46",
                "markVol": "0.455",
                "fwdPx": "97000",
                "ts": "1734166000000"
            },
            {
                "instType": "OPTION",
                "instId": "BTC-USD-241217-94000-P",
                "uly": "BTC-USD",
                "bidVol": "0.45",
                "askVol": "0.46",
                "markVol": "0.455",
                "fwdPx": "97000",
                "ts": "1734166000000"
            },
            {
                "instType": "OPTION",
                "instId": "BTC-USD-250117-100000-C",
                "uly": "BTC-USD",
                "bidVol": "0.50",
                "askVol": "0.51",
                "markVol": "0.505",
                "fwdPx": "99000",
                "ts": "1734166000000"
            },
            {
                "instType": "OPTION",
                "instId": "BTC-USD-250117-105000-P",
                "uly": "BTC-USD",
                "bidVol": "0.50",
                "askVol": "0.51",
                "markVol": "0.505",
                "fwdPx": "99000",
                "ts": "1734166000000"
            }
        ]
    });
    let state = Arc::new(TestServerState::default());
    *state.option_summary_response.lock().await = Some(response);
    let addr = start_test_server(state.clone()).await;
    let base_url = format!("http://{addr}");
    let client = OKXHttpClient::new(
        Some(base_url),
        60,
        3,
        1000,
        10_000,
        OKXEnvironment::Live,
        None,
    )
    .unwrap();

    for instrument in load_option_instruments_any() {
        client.cache_instrument(instrument);
    }

    let forward_prices = client.request_forward_prices("BTC", None).await.unwrap();

    assert_eq!(forward_prices.len(), 2);
    assert_eq!(
        forward_prices[0].instrument_id,
        InstrumentId::from("BTC-USD-241217-92000-C.OKX")
    );
    assert_eq!(forward_prices[0].forward_price.to_string(), "97000");
    assert_eq!(
        forward_prices[1].instrument_id,
        InstrumentId::from("BTC-USD-250117-100000-C.OKX")
    );
    assert_eq!(forward_prices[1].forward_price.to_string(), "99000");
}

#[rstest]
#[tokio::test]
async fn test_http_place_algo_order_returns_error_on_nonzero_scode() {
    let router = Router::new()
        .route(
            "/api/v5/public/instruments",
            get(|| async { Json(load_test_data("http_get_instruments_swap.json")) }),
        )
        .route(
            "/api/v5/trade/order-algo",
            post(|_headers: HeaderMap| async {
                Json(load_test_data("http_place_algo_order_rejected.json"))
            }),
        );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, router.into_make_service())
            .await
            .unwrap();
    });
    wait_for_server(addr, "/api/v5/public/instruments").await;

    let base_url = format!("http://{addr}");
    let client = OKXHttpClient::with_credentials(
        Some("test_key".to_string()),
        Some("test_secret".to_string()),
        Some("test_passphrase".to_string()),
        Some(base_url),
        60,
        3,
        1000,
        10_000,
        OKXEnvironment::Live,
        None,
    )
    .unwrap();

    for instrument in load_swap_instruments_any() {
        client.cache_instrument(instrument);
    }

    let result = client
        .place_algo_order_with_domain_types(
            InstrumentId::from("BTC-USDT-SWAP.OKX"),
            OKXTradeMode::Cross,
            ClientOrderId::from("Orejtest"),
            OrderSide::Sell,
            OrderType::StopMarket,
            Quantity::from("0.01"),
            Some(Price::from("50000")),
            Some(TriggerType::Default),
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .await;

    assert!(result.is_err(), "non-zero sCode should return error");
    let err = result.unwrap_err();
    let err_str = err.to_string();
    assert!(
        err_str.contains("51000"),
        "error should contain sCode 51000, was: {err_str}"
    );
    assert!(
        err_str.contains("Parameter instId error"),
        "error should contain sMsg, was: {err_str}"
    );
}

/// Verifies that `request_spot_margin_position_reports` resolves a BTC
/// margin balance to the highest-priority spot pair, never to a derivative
/// sharing the same base currency. With BTC-USD, BTC-USDT, and
/// BTC-USDT-SWAP all cached, the USDT-quoted spot pair must win (quote
/// priority USDT > USD) and the perpetual must never be selected.
#[rstest]
#[tokio::test]
async fn test_http_request_spot_margin_position_reports_prefers_currency_pair() {
    // Balance payload with a BTC short margin position (`spotInUseAmt` < 0,
    // non-zero `liab`). Built from a raw string to stay under the `json!`
    // macro recursion limit on the `OKXBalanceDetail` shape.
    let balance_body: serde_json::Value = serde_json::from_str(
        r#"{
            "code": "0",
            "msg": "",
            "data": [{
                "adjEq": "",
                "borrowFroz": "",
                "imr": "0",
                "isoEq": "0",
                "mgnRatio": "",
                "mmr": "0",
                "notionalUsd": "0",
                "notionalUsdForBorrow": "0",
                "notionalUsdForFutures": "0",
                "notionalUsdForOption": "0",
                "notionalUsdForSwap": "0",
                "ordFroz": "0",
                "totalEq": "0",
                "uTime": "1744498994783",
                "upl": "0",
                "details": [{
                    "accAvgPx": "",
                    "availBal": "0",
                    "availEq": "0",
                    "borrowFroz": "",
                    "cashBal": "0",
                    "ccy": "BTC",
                    "clSpotInUseAmt": "",
                    "collateralEnabled": false,
                    "crossLiab": "0.1",
                    "disEq": "0",
                    "eq": "-0.1",
                    "eqUsd": "0",
                    "fixedBal": "0",
                    "frozenBal": "0",
                    "imr": "0",
                    "interest": "",
                    "isoEq": "0",
                    "isoLiab": "",
                    "isoUpl": "0",
                    "liab": "0.1",
                    "maxLoan": "",
                    "maxSpotInUseAmt": "",
                    "mgnRatio": "",
                    "mmr": "0",
                    "notionalLever": "0",
                    "openAvgPx": "",
                    "ordFrozen": "0",
                    "rewardBal": "0",
                    "smtSyncEq": "0",
                    "spotBal": "",
                    "spotCopyTradingEq": "0",
                    "spotInUseAmt": "-0.1",
                    "spotIsoBal": "0",
                    "spotUpl": "",
                    "spotUplRatio": "",
                    "stgyEq": "0",
                    "totalPnl": "",
                    "totalPnlRatio": "",
                    "twap": "0",
                    "uTime": "1744498994783",
                    "upl": "0",
                    "uplLiab": ""
                }]
            }]
        }"#,
    )
    .expect("balance fixture must parse");

    let router = Router::new().route(
        "/api/v5/account/balance",
        get(move |headers: HeaderMap| {
            let body = balance_body.clone();
            async move {
                if !has_auth_headers(&headers) {
                    return (
                        StatusCode::UNAUTHORIZED,
                        Json(json!({"code": "401", "msg": "", "data": []})),
                    )
                        .into_response();
                }
                Json(body).into_response()
            }
        }),
    );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, router.into_make_service())
            .await
            .unwrap();
    });

    // No /public/instruments route, so poll an always-404 path to confirm the
    // server is accepting connections.
    wait_until_async(
        || {
            let addr_copy = addr;
            async move { tokio::net::TcpStream::connect(addr_copy).await.is_ok() }
        },
        Duration::from_secs(5),
    )
    .await;

    let base_url = format!("http://{addr}");
    let client = OKXHttpClient::with_credentials(
        Some("test_key".to_string()),
        Some("test_secret".to_string()),
        Some("test_passphrase".to_string()),
        Some(base_url),
        60,
        3,
        1000,
        10_000,
        OKXEnvironment::Live,
        None,
    )
    .unwrap();

    // Seed three sources that all share BTC as the base currency:
    //   - `http_get_instruments_spot.json` contributes BTC-USD (CurrencyPair)
    //   - `http_get_instruments_margin.json` contributes BTC-USDT (also
    //     parses to CurrencyPair via the Margin → spot path)
    //   - `http_get_instruments_swap.json` contributes BTC-USDT-SWAP
    //     (CryptoPerpetual)
    // The filter must pick BTC-USDT (quote-priority USDT > USD), never the
    // lexically-earlier BTC-USD and never the BTC-USDT-SWAP derivative.
    for instrument in load_instruments_any() {
        client.cache_instrument(instrument);
    }

    for instrument in load_instruments_from("http_get_instruments_margin.json") {
        client.cache_instrument(instrument);
    }

    for instrument in load_swap_instruments_any() {
        client.cache_instrument(instrument);
    }

    let reports = client
        .request_spot_margin_position_reports(AccountId::new("OKX-001"))
        .await
        .unwrap();

    assert_eq!(reports.len(), 1, "expected one BTC margin report");
    let report = &reports[0];

    assert_eq!(
        report.instrument_id,
        InstrumentId::from("BTC-USDT.OKX"),
        "spot-margin report must prefer the USDT-quoted pair over USD and never a derivative",
    );
    assert_ne!(
        report.instrument_id,
        InstrumentId::from("BTC-USDT-SWAP.OKX"),
        "derivative must never win the base-currency lookup",
    );
    assert_ne!(
        report.instrument_id,
        InstrumentId::from("BTC-USD.OKX"),
        "USD must lose to USDT in the quote-priority tiebreaker",
    );
}
