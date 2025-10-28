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

//! Integration tests for the OKX HTTP client using a mock Axum server.

use std::{collections::HashMap, net::SocketAddr, path::PathBuf, sync::Arc};

use axum::{
    Router,
    extract::Query,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Json},
    routing::get,
};
use chrono::{Duration as ChronoDuration, Utc};
use nautilus_core::UnixNanos;
use nautilus_model::{identifiers::InstrumentId, instruments::InstrumentAny};
use nautilus_okx::{
    common::enums::{OKXInstrumentType, OKXOrderStatus},
    http::{
        client::OKXHttpInnerClient,
        error::OKXHttpError,
        query::{
            GetInstrumentsParamsBuilder, GetOrderHistoryParams, GetOrderParamsBuilder,
            GetPendingOrdersParams,
        },
    },
};
use rstest::rstest;
use serde_json::{Value, json};
use tokio::sync::Mutex;

#[derive(Clone, Default)]
struct TestServerState {
    request_count: Arc<Mutex<usize>>,
    last_history_trades_query: Arc<Mutex<Option<HashMap<String, String>>>>,
    last_pending_orders_query: Arc<Mutex<Option<HashMap<String, String>>>>,
    last_order_history_query: Arc<Mutex<Option<HashMap<String, String>>>>,
    last_order_detail_query: Arc<Mutex<Option<HashMap<String, String>>>>,
}

fn manifest_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn load_test_data(filename: &str) -> Value {
    let path = manifest_path().join("test_data").join(filename);
    let content = std::fs::read_to_string(path).expect("failed to read test data");
    serde_json::from_str(&content).expect("failed to parse test data")
}

fn has_auth_headers(headers: &HeaderMap) -> bool {
    headers.contains_key("ok-access-key")
        && headers.contains_key("ok-access-passphrase")
        && headers.contains_key("ok-access-timestamp")
        && headers.contains_key("ok-access-sign")
}

fn load_instruments_any() -> Vec<InstrumentAny> {
    let payload = load_test_data("http_get_instruments_spot.json");
    let response: nautilus_okx::http::client::OKXResponse<
        nautilus_okx::common::models::OKXInstrument,
    > = serde_json::from_value(payload).expect("invalid instrument payload");
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
    let pending_state = state.clone();
    let order_history_state = state.clone();
    let order_detail_state = state;
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

                        *state.last_pending_orders_query.lock().await = Some(params);
                        Json(load_test_data("http_get_orders_pending.json")).into_response()
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
            ),
        )
}

async fn start_test_server(state: Arc<TestServerState>) -> SocketAddr {
    let router = create_router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("failed to bind test server");
    let addr = listener.local_addr().expect("missing local addr");

    tokio::spawn(async move {
        axum::serve(listener, router.into_make_service())
            .await
            .expect("test server failed");
    });

    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    addr
}

#[rstest]
#[tokio::test]
async fn test_http_get_instruments_returns_data() {
    let addr = start_test_server(Arc::new(TestServerState::default())).await;
    let base_url = format!("http://{}", addr);

    let params = GetInstrumentsParamsBuilder::default()
        .inst_type(OKXInstrumentType::Spot)
        .build()
        .expect("failed to build instrument params");
    let client = OKXHttpInnerClient::new(Some(base_url.clone()), Some(60), None, None, None, false)
        .expect("failed to create http client");

    let instruments = client
        .http_get_instruments(params)
        .await
        .expect("failed to fetch instruments");

    assert!(!instruments.is_empty());
    assert_eq!(instruments[0].inst_type, OKXInstrumentType::Spot);
}

#[rstest]
#[tokio::test]
async fn test_http_get_balance_requires_credentials() {
    let addr = start_test_server(Arc::new(TestServerState::default())).await;
    let base_url = format!("http://{}", addr);

    let client = OKXHttpInnerClient::new(Some(base_url), Some(60), None, None, None, false)
        .expect("failed to create http client");

    let result = client.http_get_balance().await;

    match result {
        Err(OKXHttpError::MissingCredentials) => {}
        other => panic!("expected MissingCredentials error, got {other:?}"),
    }
}

#[rstest]
#[tokio::test]
async fn test_http_get_balance_with_credentials_succeeds() {
    let addr = start_test_server(Arc::new(TestServerState::default())).await;
    let base_url = format!("http://{}", addr);

    let client = OKXHttpInnerClient::with_credentials(
        "test_key".to_string(),
        "test_secret".to_string(),
        "passphrase".to_string(),
        base_url.clone(),
        Some(60),
        None,
        None,
        None,
        false,
    )
    .expect("failed to create authenticated client");

    let accounts = client
        .http_get_balance()
        .await
        .expect("expected balance response");

    assert!(!accounts.is_empty());
}

#[rstest]
#[tokio::test]
async fn test_http_get_instruments_handles_rate_limit_error() {
    let state = Arc::new(TestServerState::default());
    let addr = start_test_server(state.clone()).await;
    let base_url = format!("http://{}", addr);

    let params = GetInstrumentsParamsBuilder::default()
        .inst_type(OKXInstrumentType::Spot)
        .build()
        .expect("failed to build instrument params");
    let client =
        OKXHttpInnerClient::new(Some(base_url.clone()), Some(60), Some(0), None, None, false)
            .expect("failed to create http client");

    let mut last_error = None;
    for _ in 0..5 {
        match client.http_get_instruments(params.clone()).await {
            Ok(_) => continue,
            Err(e) => {
                last_error = Some(e);
                break;
            }
        }
    }

    match last_error.expect("expected rate limit error") {
        OKXHttpError::OkxError { error_code, .. } => assert_eq!(error_code, "50116"),
        other => panic!("expected OkxError, got {other:?}"),
    }
}

#[rstest]
#[tokio::test]
async fn test_http_get_pending_orders_requires_credentials() {
    let addr = start_test_server(Arc::new(TestServerState::default())).await;
    let base_url = format!("http://{}", addr);

    let client = OKXHttpInnerClient::new(Some(base_url), Some(60), None, None, None, false)
        .expect("failed to create anonymous client");

    let params = GetPendingOrdersParams {
        inst_type: OKXInstrumentType::Swap,
        inst_id: "BTC-USDT-SWAP".to_string(),
        pos_side: None,
    };

    match client.http_get_pending_orders(params).await {
        Err(OKXHttpError::MissingCredentials) => {}
        other => panic!("expected MissingCredentials error, got {other:?}"),
    }
}

#[rstest]
#[tokio::test]
async fn test_http_get_pending_orders_returns_live_orders() {
    let state = Arc::new(TestServerState::default());
    let addr = start_test_server(state.clone()).await;
    let base_url = format!("http://{}", addr);

    let client = OKXHttpInnerClient::with_credentials(
        "key".to_string(),
        "secret".to_string(),
        "pass".to_string(),
        base_url.clone(),
        Some(60),
        None,
        None,
        None,
        false,
    )
    .expect("failed to create authenticated client");

    let params = GetPendingOrdersParams {
        inst_type: OKXInstrumentType::Swap,
        inst_id: "BTC-USDT-SWAP".to_string(),
        pos_side: None,
    };

    let orders = client
        .http_get_pending_orders(params)
        .await
        .expect("expected pending orders response");

    assert_eq!(orders.len(), 1);
    assert_eq!(orders[0].state, OKXOrderStatus::Live);
    assert_eq!(orders[0].inst_id.as_str(), "BTC-USDT-SWAP");

    let query = state
        .last_pending_orders_query
        .lock()
        .await
        .clone()
        .expect("pending orders query missing");
    assert_eq!(query.get("instType"), Some(&"SWAP".to_string()));
    assert_eq!(query.get("instId"), Some(&"BTC-USDT-SWAP".to_string()));
}

#[rstest]
#[tokio::test]
async fn test_http_get_order_history_applies_filters() {
    let state = Arc::new(TestServerState::default());
    let addr = start_test_server(state.clone()).await;
    let base_url = format!("http://{}", addr);

    let client = OKXHttpInnerClient::with_credentials(
        "key".to_string(),
        "secret".to_string(),
        "pass".to_string(),
        base_url.clone(),
        Some(60),
        None,
        None,
        None,
        false,
    )
    .expect("failed to create authenticated client");

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

    let orders = client
        .http_get_order_history(params)
        .await
        .expect("expected order history response");
    assert!(!orders.is_empty());

    let query = state
        .last_order_history_query
        .lock()
        .await
        .clone()
        .expect("order history query missing");
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
    let base_url = format!("http://{}", addr);

    let client = OKXHttpInnerClient::with_credentials(
        "key".to_string(),
        "secret".to_string(),
        "pass".to_string(),
        base_url.clone(),
        Some(60),
        None,
        None,
        None,
        false,
    )
    .expect("failed to create authenticated client");

    let params = GetOrderParamsBuilder::default()
        .inst_type(OKXInstrumentType::Swap)
        .inst_id("BTC-USDT-SWAP")
        .ord_id("1234567890123456789")
        .cl_ord_id("client-order-1")
        .build()
        .expect("failed to build order params");

    let orders = client
        .http_get_order(params)
        .await
        .expect("expected order detail response");
    assert_eq!(orders.len(), 1);

    let query = state
        .last_order_detail_query
        .lock()
        .await
        .clone()
        .expect("order detail query missing");
    assert_eq!(query.get("instType"), Some(&"SWAP".to_string()));
    assert_eq!(query.get("instId"), Some(&"BTC-USDT-SWAP".to_string()));
    assert_eq!(query.get("ordId"), Some(&"1234567890123456789".to_string()));
    assert_eq!(query.get("clOrdId"), Some(&"client-order-1".to_string()));
}

#[tokio::test]
async fn test_request_trades_uses_after_before() {
    let state = Arc::new(TestServerState::default());
    let addr = start_test_server(state.clone()).await;
    let base_url = format!("http://{}", addr);

    let mut client = nautilus_okx::http::client::OKXHttpClient::new(
        Some(base_url),
        Some(60),
        None,
        None,
        None,
        false,
    )
    .expect("failed to create http client");

    for instrument in load_instruments_any() {
        client.add_instrument(instrument);
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
        .expect("request_trades should succeed");
    assert!(trades.is_empty());

    let query = state
        .last_history_trades_query
        .lock()
        .await
        .clone()
        .expect("history trades query missing");

    assert_eq!(query.get("instId"), Some(&"BTC-USD".to_string()));
    assert_eq!(
        query.get("after"),
        Some(&start.timestamp_millis().to_string())
    );
    assert_eq!(
        query.get("before"),
        Some(&end.timestamp_millis().to_string())
    );
    assert_eq!(query.get("limit"), Some(&"100".to_string()));
}
