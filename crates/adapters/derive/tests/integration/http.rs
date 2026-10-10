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

//! Integration tests for the Derive HTTP client using an axum mock server.
//!
//! Covers the request shape produced by `dispatch()`: URL formation,
//! `Content-Type`, body, and the `X-Derive*` auth-header injection for
//! authenticated calls. Pure decoding behavior lives in the unit tests
//! beside `decode_envelope`.

use std::{
    collections::HashMap,
    net::SocketAddr,
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use alloy::{
    primitives::{Signature, eip191_hash_message, hex},
    signers::local::PrivateKeySigner,
};
use axum::{
    Router,
    extract::State,
    http::{HeaderMap, StatusCode, Uri},
    response::{IntoResponse, Json, Response},
    routing::post,
};
use nautilus_common::testing::wait_until_async;
use nautilus_derive::{
    common::{
        consts::{HEADER_DERIVE_SIGNATURE, HEADER_DERIVE_TIMESTAMP, HEADER_DERIVE_WALLET},
        enums::{DeriveInstrumentType, DeriveOrderSide, DeriveOrderType, DeriveTimeInForce},
        retry::http_retry_config,
    },
    http::{
        DeriveCredentials, DeriveHttpClient, DeriveHttpError,
        query::{
            DeriveCancelByLabelParams, DeriveGetOpenOrdersParams, DeriveGetOrderHistoryParams,
            DeriveGetOrderParams, DeriveGetPositionsParams, DeriveGetSubaccountParams,
            DeriveGetTradeHistoryParams, DeriveGetTriggerOrdersParams, DeriveOrderParams,
            DeriveSignedEnvelope,
        },
    },
};
use nautilus_network::{http::HttpClient, retry::RetryError};
use rstest::rstest;
use rust_decimal_macros::dec;
use serde_json::{Value, json};

const SESSION_KEY_HEX: &str = "0x2ae8be44db8a590d20bffbe3b6872df9b569147d3bf6801a35a28281a4816bbd";
const TEST_WALLET: &str = "0x000000000000000000000000000000000000aaaa";

#[derive(Clone)]
struct CapturedRequest {
    path: String,
    headers: HashMap<String, String>,
    body: Value,
    received_at_ms: u64,
    received_at: Instant,
}

#[derive(Clone, Default)]
struct TestServerState {
    captured: Arc<tokio::sync::Mutex<Vec<CapturedRequest>>>,
    response_body: Arc<tokio::sync::Mutex<Value>>,
    response_status: Arc<tokio::sync::Mutex<StatusCode>>,
    response_headers: Arc<tokio::sync::Mutex<HeaderMap>>,
    delay: Arc<tokio::sync::Mutex<Option<Duration>>>,
}

impl TestServerState {
    fn with_success_response() -> Self {
        let state = Self::default();
        let body = json!({"id": 1, "result": {"ok": true}});
        // Default response: 200 + success envelope. Tests override per case.
        *state.response_body.try_lock().unwrap() = body;
        *state.response_status.try_lock().unwrap() = StatusCode::OK;
        state
    }

    async fn captured(&self) -> CapturedRequest {
        self.captured
            .lock()
            .await
            .last()
            .cloned()
            .expect("no request captured")
    }

    async fn captured_all(&self) -> Vec<CapturedRequest> {
        self.captured.lock().await.clone()
    }
}

async fn handle(
    path: &str,
    state: TestServerState,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    let parsed_body: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    let mut header_map = HashMap::new();

    for (name, value) in &headers {
        if let Ok(v) = value.to_str() {
            header_map.insert(name.as_str().to_lowercase(), v.to_string());
        }
    }

    let received_at_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time is after unix epoch")
        .as_millis() as u64;
    state.captured.lock().await.push(CapturedRequest {
        path: path.to_string(),
        headers: header_map,
        body: parsed_body,
        received_at_ms,
        received_at: Instant::now(),
    });

    let delay = *state.delay.lock().await;
    if let Some(delay) = delay {
        tokio::time::sleep(delay).await;
    }

    let status = *state.response_status.lock().await;
    let body = state.response_body.lock().await.clone();
    let response_headers = state.response_headers.lock().await.clone();
    (status, response_headers, Json(body)).into_response()
}

async fn handle_get_instruments(
    State(state): State<TestServerState>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    handle("/public/get_instruments", state, headers, body).await
}

async fn handle_get_instrument(
    State(state): State<TestServerState>,
    uri: Uri,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    handle(uri.path(), state, headers, body).await
}

async fn handle_order(
    State(state): State<TestServerState>,
    uri: Uri,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    handle(uri.path(), state, headers, body).await
}

async fn handle_cancel_by_label(
    State(state): State<TestServerState>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    handle("/private/cancel_by_label", state, headers, body).await
}

async fn handle_trade_history(
    State(state): State<TestServerState>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    handle("/public/get_trade_history", state, headers, body).await
}

async fn handle_funding_rate_history(
    State(state): State<TestServerState>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    handle("/public/get_funding_rate_history", state, headers, body).await
}

async fn handle_tradingview_chart_data(
    State(state): State<TestServerState>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    handle("/public/get_tradingview_chart_data", state, headers, body).await
}

async fn handle_tickers(
    State(state): State<TestServerState>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    handle("/public/get_tickers", state, headers, body).await
}

async fn handle_health() -> impl IntoResponse {
    StatusCode::OK
}

async fn start_mock_server(state: TestServerState) -> SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let router = Router::new()
        .route("/public/get_instruments", post(handle_get_instruments))
        .route("/public/get_instrument", post(handle_get_instrument))
        .route("/v3/public/get_instrument", post(handle_get_instrument))
        .route("/public/get_trade_history", post(handle_trade_history))
        .route(
            "/public/get_funding_rate_history",
            post(handle_funding_rate_history),
        )
        .route(
            "/public/get_tradingview_chart_data",
            post(handle_tradingview_chart_data),
        )
        .route("/public/get_tickers", post(handle_tickers))
        .route("/private/get_order", post(handle_order))
        .route("/private/get_open_orders", post(handle_order))
        .route("/private/get_trigger_orders", post(handle_order))
        .route("/private/get_order_history", post(handle_order))
        .route("/private/get_trade_history", post(handle_order))
        .route("/private/get_subaccount", post(handle_order))
        .route("/private/get_positions", post(handle_order))
        .route("/private/order", post(handle_order))
        .route("/v3/private/order", post(handle_order))
        .route("/private/cancel_by_label", post(handle_cancel_by_label))
        .route("/health", axum::routing::get(handle_health))
        .with_state(state);

    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });

    let health_url = format!("http://{addr}/health");
    let http_client = HttpClient::builder().build().unwrap();
    wait_until_async(
        || {
            let url = health_url.clone();
            let client = http_client.clone();
            async move { client.get(url, None, None, Some(1), None).await.is_ok() }
        },
        Duration::from_secs(5),
    )
    .await;

    addr
}

fn base_url(addr: SocketAddr) -> String {
    format!("http://{addr}")
}

fn data_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test_data")
}

fn load_json(filename: &str) -> Value {
    let content = std::fs::read_to_string(data_path().join(filename))
        .unwrap_or_else(|_| panic!("failed to read {filename}"));
    serde_json::from_str(&content).expect("invalid json")
}

fn test_credentials() -> DeriveCredentials {
    DeriveCredentials::new(TEST_WALLET, SESSION_KEY_HEX).unwrap()
}

#[rstest]
#[tokio::test]
async fn test_send_public_posts_params_with_no_auth_headers() {
    let state = TestServerState::with_success_response();
    *state.response_body.lock().await = load_json("perps/http_get_instrument_eth.json");
    let addr = start_mock_server(state.clone()).await;

    let client = DeriveHttpClient::new(base_url(addr), Some(5), None, None).unwrap();
    let instrument = client.get_instrument("ETH-PERP").await.unwrap();

    let captured = state.captured().await;
    assert_eq!(captured.path, "/public/get_instrument");
    assert_eq!(captured.body, json!({"instrument_name": "ETH-PERP"}));
    assert_eq!(
        captured.headers.get("content-type").map(String::as_str),
        Some("application/json"),
    );
    assert!(
        !captured
            .headers
            .contains_key(&HEADER_DERIVE_WALLET.to_lowercase())
    );
    assert!(
        !captured
            .headers
            .contains_key(&HEADER_DERIVE_TIMESTAMP.to_lowercase())
    );
    assert!(
        !captured
            .headers
            .contains_key(&HEADER_DERIVE_SIGNATURE.to_lowercase())
    );
    assert!(
        !captured
            .headers
            .keys()
            .any(|name| name.starts_with("x-lyra"))
    );
    assert_eq!(instrument.instrument_name, "ETH-PERP");
}

#[rstest]
#[case("")]
#[case("/v3")]
#[tokio::test]
async fn test_get_instrument_posts_instrument_name(#[case] prefix: &str) {
    let state = TestServerState::with_success_response();
    *state.response_body.lock().await = load_json("perps/http_get_instrument_eth.json");
    let addr = start_mock_server(state.clone()).await;

    let client =
        DeriveHttpClient::new(format!("{}{prefix}", base_url(addr)), Some(5), None, None).unwrap();
    let instrument = client.get_instrument("ETH-PERP").await.unwrap();

    let captured = state.captured().await;
    assert_eq!(captured.path, format!("{prefix}/public/get_instrument"));
    assert_eq!(captured.body, json!({"instrument_name": "ETH-PERP"}));
    assert_eq!(instrument.instrument_name, "ETH-PERP");
    assert_eq!(instrument.instrument_type, DeriveInstrumentType::Perp);
}

#[rstest]
#[case("subaccount")]
#[case("positions")]
#[tokio::test]
async fn test_private_snapshot_requires_requested_account_identity(#[case] source: &str) {
    let state = TestServerState::with_success_response();

    let mut result = load_json(if source == "subaccount" {
        "common/http_subaccount_usdc.json"
    } else {
        "perps/http_positions_result_eth.json"
    });

    result["subaccount_id"] = json!(43);
    *state.response_body.lock().await = json!({"id": 1, "result": result.clone()});
    let addr = start_mock_server(state.clone()).await;
    let client =
        DeriveHttpClient::with_credentials(base_url(addr), test_credentials(), Some(5), None, None)
            .unwrap();

    let outcome = if source == "subaccount" {
        client
            .get_subaccount(&DeriveGetSubaccountParams::new(42))
            .await
            .map(|_| ())
    } else {
        client
            .get_positions(&DeriveGetPositionsParams::new(42))
            .await
            .map(|_| ())
    };

    let error = outcome.expect_err("foreign account snapshot must fail");
    assert!(
        matches!(error, DeriveHttpError::Decode(detail) if detail == "subaccount response identity mismatch: requested 42, received 43")
    );
    result["subaccount_id"] = json!(42);
    *state.response_body.lock().await = json!({"id": 1, "result": result});

    if source == "subaccount" {
        let snapshot = client
            .get_subaccount(&DeriveGetSubaccountParams::new(42))
            .await
            .unwrap();
        assert_eq!(snapshot.subaccount_id, 42);
    } else {
        let snapshot = client
            .get_positions(&DeriveGetPositionsParams::new(42))
            .await
            .unwrap();
        assert_eq!(snapshot.subaccount_id, 42);
        assert_eq!(snapshot.positions.len(), 1);
        assert_eq!(snapshot.positions[0].amount, dec!(1));
        assert_eq!(snapshot.positions[0].average_price, dec!(3500));
    }

    let captured = state.captured_all().await;
    assert_eq!(captured.len(), 2);
    assert_eq!(captured[0].body, json!({"subaccount_id": 42}));
    assert_eq!(captured[1].body, captured[0].body);
}

#[rstest]
#[case("order")]
#[case("open-orders")]
#[case("trigger-orders")]
#[case("order-history")]
#[case("trade-history")]
#[case("embedded-order")]
#[tokio::test]
async fn test_private_getters_reject_foreign_account_rows(
    #[case] source: &str,
    #[values(false, true)] foreign_envelope: bool,
) {
    let state = TestServerState::with_success_response();
    let expected = 42;

    let returned_account = if foreign_envelope { 43 } else { expected };
    let mut order = load_json("perps/http_order_eth_partially_filled.json");
    order["subaccount_id"] = json!(if foreign_envelope { expected } else { 43 });
    let mut trade = load_json("perps/http_private_trade_eth.json");
    trade["subaccount_id"] = order["subaccount_id"].clone();

    let result = match source {
        "order" => {
            order["subaccount_id"] = json!(43);
            order
        }
        "open-orders" | "trigger-orders" => {
            json!({"orders": [order], "subaccount_id": returned_account})
        }
        "order-history" => {
            json!({"orders": [order], "pagination": {"count": 1, "num_pages": 1}, "subaccount_id": returned_account})
        }
        "trade-history" => {
            json!({"trades": [trade], "pagination": {"count": 1, "num_pages": 1}, "subaccount_id": returned_account})
        }
        "embedded-order" => {
            let mut snapshot = load_json("common/http_subaccount_usdc.json");
            snapshot["subaccount_id"] = json!(returned_account);
            snapshot["open_orders"] = json!([order]);
            snapshot
        }
        _ => unreachable!(),
    };

    *state.response_body.lock().await = json!({"id": 1, "result": result});
    let addr = start_mock_server(state.clone()).await;
    let client =
        DeriveHttpClient::with_credentials(base_url(addr), test_credentials(), Some(5), None, None)
            .unwrap();

    let outcome = match source {
        "order" => client
            .get_order(&DeriveGetOrderParams::new(42, "order-abc"))
            .await
            .map(|_| ()),
        "open-orders" => client
            .get_open_orders(&DeriveGetOpenOrdersParams::new(42))
            .await
            .map(|_| ()),
        "trigger-orders" => client
            .get_trigger_orders(&DeriveGetTriggerOrdersParams::new(42))
            .await
            .map(|_| ()),
        "order-history" => client
            .get_order_history(&DeriveGetOrderHistoryParams::new(42, 1, 100))
            .await
            .map(|_| ()),
        "trade-history" => client
            .get_private_trade_history(&DeriveGetTradeHistoryParams::new(42, 1, 100))
            .await
            .map(|_| ()),
        "embedded-order" => client
            .get_subaccount(&DeriveGetSubaccountParams::new(42))
            .await
            .map(|_| ()),
        _ => unreachable!(),
    };

    assert!(
        matches!(outcome, Err(DeriveHttpError::Decode(detail)) if detail == "subaccount response identity mismatch: requested 42, received 43")
    );
    let captured = state.captured_all().await;
    assert_eq!(captured.len(), 1);
    assert_eq!(captured[0].body["subaccount_id"], json!(42));
}

#[tokio::test]
async fn test_get_instrument_rejects_foreign_definition() {
    let state = TestServerState::with_success_response();
    let mut response = load_json("perps/http_get_instrument_eth.json");
    response["result"]["instrument_name"] = json!("BTC-PERP");
    *state.response_body.lock().await = response;
    let addr = start_mock_server(state.clone()).await;
    let client = DeriveHttpClient::new(base_url(addr), Some(5), None, None).unwrap();

    let error = client.get_instrument("ETH-PERP").await.unwrap_err();

    assert!(matches!(&error, DeriveHttpError::Decode(message)
        if message == "instrument response identity mismatch: requested ETH-PERP, received BTC-PERP"));
    let captured = state.captured_all().await;
    assert_eq!(captured.len(), 1);
    assert_eq!(captured[0].path, "/public/get_instrument");
    assert_eq!(captured[0].body, json!({"instrument_name": "ETH-PERP"}));
}

#[rstest]
#[case("")]
#[case("/v3")]
#[tokio::test]
async fn test_send_private_attaches_all_derive_auth_headers(#[case] prefix: &str) {
    let state = TestServerState::with_success_response();
    *state.response_body.lock().await = json!({
        "id": 1,
        "result": {"order": load_json("perps/http_order_eth_partially_filled.json")},
    });
    let addr = start_mock_server(state.clone()).await;

    let client = DeriveHttpClient::with_credentials(
        format!("{}{prefix}", base_url(addr)),
        test_credentials(),
        Some(5),
        None,
        None,
    )
    .unwrap();

    let payload = DeriveOrderParams {
        envelope: DeriveSignedEnvelope {
            subaccount_id: 42,
            nonce: 123,
            signer: "0xsigner".to_string(),
            signature_expiry_sec: 1_700_001_000,
            signature: "0x00".into(),
        },
        instrument_name: "ETH-PERP".into(),
        direction: DeriveOrderSide::Buy,
        order_type: DeriveOrderType::Limit,
        time_in_force: DeriveTimeInForce::Gtc,
        limit_price: dec!(3500),
        amount: dec!(1),
        max_fee: dec!(1),
        label: "client-1".to_string(),
        referral_code: "nautilus".to_string(),
        reduce_only: None,
        mmp: None,
        trigger_price: None,
        trigger_price_type: None,
        trigger_type: None,
    };

    let order = client.submit_order(&payload).await.unwrap();

    let captured = state.captured().await;
    assert_eq!(captured.path, format!("{prefix}/private/order"));
    assert_eq!(
        captured.body,
        json!({
            "amount": "1",
            "direction": "buy",
            "instrument_name": "ETH-PERP",
            "label": "client-1",
            "limit_price": "3500",
            "max_fee": "1",
            "nonce": "123",
            "order_type": "limit",
            "referral_code": "nautilus",
            "signature": "0x00",
            "signature_expiry_sec": 1_700_001_000_i64,
            "signer": "0xsigner",
            "subaccount_id": 42,
            "time_in_force": "gtc",
        })
    );

    let wallet = captured
        .headers
        .get("x-derivewallet")
        .expect("wallet header present");
    assert_eq!(wallet, TEST_WALLET);

    let timestamp = captured
        .headers
        .get("x-derivetimestamp")
        .expect("timestamp header present");
    let ts: u64 = timestamp.parse().expect("timestamp is a u64 millis string");
    assert!(
        ts.abs_diff(captured.received_at_ms) < 1000,
        "timestamp must be current Unix milliseconds"
    );

    let signature = captured
        .headers
        .get("x-derivesignature")
        .expect("signature header present");
    assert!(signature.starts_with("0x"));
    assert_eq!(signature.len(), 2 + 130, "signature must be 65 bytes hex");

    let signature = Signature::try_from(
        hex::decode(signature.trim_start_matches("0x"))
            .unwrap()
            .as_slice(),
    )
    .unwrap();
    let signer: PrivateKeySigner = SESSION_KEY_HEX.parse().unwrap();
    assert_eq!(
        signature
            .recover_address_from_prehash(&eip191_hash_message(timestamp.as_bytes()))
            .unwrap(),
        signer.address()
    );
    assert!(
        !captured
            .headers
            .keys()
            .any(|name| name.starts_with("x-lyra"))
    );

    assert_eq!(order.order_id, "abc-123");
}

#[rstest]
#[tokio::test]
async fn test_cancel_order_by_label_http_preserves_count() {
    let state = TestServerState::with_success_response();
    *state.response_body.lock().await = load_json("common/ws_cancel_by_label_nonzero.json");
    let addr = start_mock_server(state.clone()).await;

    let client =
        DeriveHttpClient::with_credentials(base_url(addr), test_credentials(), Some(5), None, None)
            .unwrap();
    let result = client
        .cancel_by_label(&DeriveCancelByLabelParams::new(42, "CLIENT-ORDER-42"))
        .await
        .unwrap();

    let captured = state.captured().await;
    assert_eq!(captured.path, "/private/cancel_by_label");
    assert_eq!(
        captured.body,
        json!({"subaccount_id": 42, "label": "CLIENT-ORDER-42"})
    );
    assert_eq!(result.cancelled_orders, 2);
}

#[rstest]
#[tokio::test]
async fn test_paced_http_writes_build_auth_headers_after_waiting() {
    let state = TestServerState::with_success_response();
    *state.response_body.lock().await = json!({
        "id": 1,
        "result": {"order": load_json("perps/http_order_eth_partially_filled.json")},
    });
    let addr = start_mock_server(state.clone()).await;
    // The fixed window is aligned to client construction, so measure from
    // before the build to bound the reset wait.
    let started = Instant::now();
    let client =
        DeriveHttpClient::with_credentials(base_url(addr), test_credentials(), Some(5), None, None)
            .unwrap();

    let payload = DeriveOrderParams {
        envelope: DeriveSignedEnvelope {
            subaccount_id: 42,
            nonce: 123,
            signer: "0xsigner".to_string(),
            signature_expiry_sec: 1_700_001_000,
            signature: "0x00".into(),
        },
        instrument_name: "ETH-PERP".into(),
        direction: DeriveOrderSide::Buy,
        order_type: DeriveOrderType::Limit,
        time_in_force: DeriveTimeInForce::Gtc,
        limit_price: dec!(3500),
        amount: dec!(1),
        max_fee: dec!(1),
        label: "client-paced".to_string(),
        referral_code: "nautilus".to_string(),
        reduce_only: None,
        mmp: None,
        trigger_price: None,
        trigger_price_type: None,
        trigger_type: None,
    };

    let requests = (0..7).map(|sequence| {
        let client = client.clone();
        let mut payload = payload.clone();
        payload.envelope.nonce += sequence;
        async move { client.submit_order(&payload).await }
    });

    let outcomes = futures_util::future::join_all(requests).await;
    let elapsed = started.elapsed();
    let captured = state.captured_all().await;

    assert!(outcomes.iter().all(Result::is_ok));
    assert_eq!(captured.len(), 7);
    assert!(
        elapsed >= Duration::from_secs(4),
        "writes past the five-request burst must wait for the discrete window \
         reset (~5s), elapsed {elapsed:?}",
    );

    for request in captured {
        let timestamp = request
            .headers
            .get(&HEADER_DERIVE_TIMESTAMP.to_lowercase())
            .expect("timestamp header present")
            .parse::<u64>()
            .expect("timestamp header is milliseconds");
        let age_ms = request.received_at_ms.saturating_sub(timestamp);
        assert!(
            age_ms < 900,
            "auth timestamp must be built after the limiter wait, age was {age_ms} ms",
        );
    }
}

#[rstest]
#[tokio::test]
async fn test_method_path_with_leading_slash_resolves_same_url() {
    let state = TestServerState::with_success_response();
    *state.response_body.lock().await = json!({"id": 1, "result": "ok"});
    let addr = start_mock_server(state.clone()).await;

    let client = DeriveHttpClient::new(base_url(addr), Some(5), None, None).unwrap();
    let value: Value = client
        .send_public(
            "/public/get_instruments",
            &json!({"currency": "BTC", "expired": false}),
        )
        .await
        .unwrap();

    let captured = state.captured().await;
    // The leading slash must be trimmed so the URL hits the same route as
    // `method="public/get_instruments"`. Without the trim, the URL would
    // become `http://addr//public/...` and 404.
    assert_eq!(captured.path, "/public/get_instruments");
    assert_eq!(value, json!("ok"));
}

#[rstest]
#[tokio::test]
async fn test_get_trade_history_posts_pagination_params() {
    let state = TestServerState::with_success_response();
    *state.response_body.lock().await =
        json!({"id": 1, "result": load_json("perps/http_public_trades_result_eth.json")});
    let addr = start_mock_server(state.clone()).await;

    let client = DeriveHttpClient::new(base_url(addr), Some(5), None, None).unwrap();
    let result = client
        .get_trade_history(
            "ETH-PERP",
            Some(1_700_000_000_000),
            Some(1_700_000_500_000),
            2,
            500,
        )
        .await
        .unwrap();

    let captured = state.captured().await;
    assert_eq!(captured.path, "/public/get_trade_history");
    assert_eq!(
        captured.body,
        json!({
            "instrument_name": "ETH-PERP",
            "page": 2,
            "page_size": 500,
            "from_timestamp": 1_700_000_000_000_i64,
            "to_timestamp": 1_700_000_500_000_i64,
        })
    );
    assert_eq!(result.trades.len(), 1);
    assert_eq!(result.pagination.num_pages, 1);
}

#[rstest]
#[tokio::test]
async fn test_get_trade_history_omits_unset_timestamps() {
    let state = TestServerState::with_success_response();
    *state.response_body.lock().await =
        json!({"id": 1, "result": load_json("perps/http_public_trades_result_eth.json")});
    let addr = start_mock_server(state.clone()).await;

    let client = DeriveHttpClient::new(base_url(addr), Some(5), None, None).unwrap();
    client
        .get_trade_history("ETH-PERP", None, None, 1, 1000)
        .await
        .unwrap();

    let captured = state.captured().await;
    assert_eq!(
        captured.body,
        json!({
            "instrument_name": "ETH-PERP",
            "page": 1,
            "page_size": 1000,
        })
    );
}

#[rstest]
#[tokio::test]
async fn test_get_funding_rate_history_posts_instrument_and_window() {
    let state = TestServerState::with_success_response();
    *state.response_body.lock().await = json!({
        "id": 1,
        "result": load_json("perps/http_public_funding_rate_history_eth.json"),
    });
    let addr = start_mock_server(state.clone()).await;

    let client = DeriveHttpClient::new(base_url(addr), Some(5), None, None).unwrap();
    let result = client
        .get_funding_rate_history(
            "ETH-PERP",
            Some(1_700_000_000_000),
            Some(1_700_007_200_000),
            Some(3600),
        )
        .await
        .unwrap();

    let captured = state.captured().await;
    assert_eq!(captured.path, "/public/get_funding_rate_history");
    assert_eq!(
        captured.body,
        json!({
            "instrument_name": "ETH-PERP",
            "start_timestamp": 1_700_000_000_000_i64,
            "end_timestamp": 1_700_007_200_000_i64,
            "period": 3600,
        })
    );
    assert_eq!(result.funding_rate_history.len(), 3);
}

#[rstest]
#[tokio::test]
async fn test_get_candles_posts_instrument_and_window() {
    let state = TestServerState::with_success_response();
    *state.response_body.lock().await = json!({
        "id": 1,
        "result": load_json("perps/http_public_candles_eth.json"),
    });
    let addr = start_mock_server(state.clone()).await;

    let client = DeriveHttpClient::new(base_url(addr), Some(5), None, None).unwrap();
    let candles = client
        .get_candles("ETH-PERP", 1_700_000_000, 1_700_002_700, 900)
        .await
        .unwrap();

    let captured = state.captured().await;
    assert_eq!(captured.path, "/public/get_tradingview_chart_data");
    assert_eq!(
        captured.body,
        json!({
            "instrument_name": "ETH-PERP",
            "start_timestamp": 1_700_000_000_i64,
            "end_timestamp": 1_700_002_700_i64,
            "period": 900,
        })
    );
    assert_eq!(candles.len(), 3);
    assert_eq!(candles[0].open_price.to_string(), "3500.0");
    assert_eq!(candles[2].timestamp_bucket, 1_700_001_800);
}

#[rstest]
#[case::option(
    "ETH-20260627-3500-C",
    "options/http_ticker_eth_snapshot.json",
    "option",
    "ETH",
    Some("20260627"),
    true
)]
#[case::perp(
    "ETH-PERP",
    "perps/http_ticker_eth_snapshot.json",
    "perp",
    "ETH",
    None,
    false
)]
#[case::spot(
    "ETH-USDC",
    "perps/http_ticker_eth_snapshot.json",
    "erc20",
    "ETH",
    None,
    false
)]
#[tokio::test]
async fn test_get_ticker_uses_get_tickers_and_selects_instrument(
    #[case] instrument_name: &str,
    #[case] fixture_path: &str,
    #[case] instrument_type: &str,
    #[case] currency: &str,
    #[case] expiry_date: Option<&str>,
    #[case] expect_option_pricing: bool,
) {
    let state = TestServerState::with_success_response();
    *state.response_body.lock().await = json!({
        "id": 1,
        "result": {
            "tickers": {
                instrument_name: load_json(fixture_path),
            },
        },
    });
    let addr = start_mock_server(state.clone()).await;

    let client = DeriveHttpClient::new(base_url(addr), Some(5), None, None).unwrap();
    let ticker = client.get_ticker(instrument_name).await.unwrap();

    let captured = state.captured().await;
    let mut expected_body = serde_json::Map::new();
    expected_body.insert("currency".to_string(), currency.into());
    if let Some(expiry_date) = expiry_date {
        expected_body.insert("expiry_date".to_string(), expiry_date.into());
    }

    expected_body.insert("instrument_type".to_string(), instrument_type.into());

    assert_eq!(captured.path, "/public/get_tickers");
    assert_eq!(captured.body, Value::Object(expected_body));
    assert_eq!(ticker.instrument_name, instrument_name);

    if expect_option_pricing {
        let pricing = ticker.option_pricing.expect("option ticker has pricing");
        assert_eq!(pricing.forward_price.to_string(), "3505");
    } else {
        assert!(ticker.option_pricing.is_none());
    }
}

#[rstest]
#[tokio::test]
async fn test_timeout_surfaces_as_transport_error() {
    let state = TestServerState::with_success_response();
    *state.delay.lock().await = Some(Duration::from_secs(3));
    let addr = start_mock_server(state).await;

    // Disable retries so the test isolates the timeout-to-transport-error
    // mapping; the default policy would retry transport errors and multiply
    // the wall-clock wait. ExponentialBackoff rejects a zero initial delay,
    // so use 1ms bounds with max_retries=0: the manager allocates the
    // backoff but never advances it.
    let no_retries = http_retry_config(0, 1, 1);
    let client = DeriveHttpClient::new(base_url(addr), Some(1), None, Some(no_retries)).unwrap();
    let err = client
        .send_public::<_, Value>("public/get_instruments", &json!({"currency": "ETH"}))
        .await
        .expect_err("must time out");
    assert!(
        err.is_transport_error(),
        "timeout must surface as transport error, was: {err:?}",
    );
}

#[rstest]
#[case::backend_unavailable(9002, 2)]
#[case::order_confirmation_timeout(9000, 1)]
#[case::engine_confirmation_timeout(9001, 1)]
#[tokio::test]
async fn test_read_retry_uses_venue_error_code(#[case] code: i64, #[case] attempts: usize) {
    let state = TestServerState::with_success_response();
    *state.response_body.lock().await = json!({
        "id": 1,
        "error": {"code": code, "message": "Venue response", "data": null},
    });
    let addr = start_mock_server(state.clone()).await;
    let mut retry_config = http_retry_config(1, 1, 1);
    retry_config.jitter_ms = 0;
    let client = DeriveHttpClient::new(base_url(addr), Some(5), None, Some(retry_config)).unwrap();

    let error = client.get_instrument("ETH-PERP").await.unwrap_err();
    let requests = state.captured_all().await;

    assert!(matches!(error, DeriveHttpError::JsonRpc { code: actual, .. } if actual == code));
    assert_eq!(requests.len(), attempts);

    for request in requests {
        assert_eq!(request.path, "/public/get_instrument");
        assert_eq!(request.body, json!({"instrument_name": "ETH-PERP"}));
    }
}

#[rstest]
#[case::http_delay(false, None, 2)]
#[case::http_budget(false, Some(150), 1)]
#[case::jsonrpc_delay(true, None, 2)]
#[case::jsonrpc_budget(true, Some(150), 1)]
#[tokio::test]
async fn test_retry_after_preserves_minimum_delay_and_elapsed_budget(
    #[case] jsonrpc: bool,
    #[case] budget_ms: Option<u64>,
    #[case] attempts: usize,
) {
    let state = TestServerState::with_success_response();
    if jsonrpc {
        *state.response_body.lock().await = json!({
            "error": {"code": 9002, "message": "Backend unavailable", "data": null},
        });
    } else {
        *state.response_status.lock().await = StatusCode::TOO_MANY_REQUESTS;
        *state.response_body.lock().await = json!({"message": "Rate limited"});
    }

    state
        .response_headers
        .lock()
        .await
        .insert("retry-after", "1".parse().unwrap());
    let addr = start_mock_server(state.clone()).await;
    let mut config = http_retry_config(1, 1, 1);
    config.jitter_ms = 0;
    config.max_elapsed_ms = budget_ms;
    let client = DeriveHttpClient::new(base_url(addr), Some(5), None, Some(config)).unwrap();
    let _error = client.get_instrument("ETH-PERP").await.unwrap_err();
    let requests = state.captured_all().await;
    assert_eq!(requests.len(), attempts);

    for request in &requests {
        assert_eq!(request.path, "/public/get_instrument");
        assert_eq!(request.body, json!({"instrument_name": "ETH-PERP"}));
    }

    if attempts == 2 {
        assert!(
            requests[1]
                .received_at
                .duration_since(requests[0].received_at)
                >= Duration::from_secs(1)
        );
    }
}

#[rstest]
#[tokio::test]
async fn test_read_per_attempt_timeout_uses_remaining_retry_budget() {
    let state = TestServerState::with_success_response();
    *state.delay.lock().await = Some(Duration::from_millis(200));
    let addr = start_mock_server(state.clone()).await;
    let mut config = http_retry_config(1, 1, 1);
    config.jitter_ms = 0;
    config.operation_timeout_ms = Some(50);
    let client = DeriveHttpClient::new(base_url(addr), Some(5), None, Some(config)).unwrap();

    let error = client.get_instrument("ETH-PERP").await.unwrap_err();
    wait_until_async(
        || async { state.captured_all().await.len() == 2 },
        Duration::from_secs(5),
    )
    .await;
    let requests = state.captured_all().await;

    assert!(matches!(
        error,
        DeriveHttpError::Retry(RetryError::OperationTimeout { timeout_ms: 50 })
    ));
    assert_eq!(requests.len(), 2);

    for request in requests {
        assert_eq!(request.path, "/public/get_instrument");
        assert_eq!(request.body, json!({"instrument_name": "ETH-PERP"}));
    }
}
