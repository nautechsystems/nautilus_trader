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
//! `Content-Type`, body, and the `X-LYRA*` auth-header injection for
//! authenticated calls. Pure decoding behavior lives in the unit tests
//! beside `decode_envelope`.

use std::{
    collections::HashMap,
    net::SocketAddr,
    path::PathBuf,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use axum::{
    Router,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Json, Response},
    routing::post,
};
use nautilus_common::testing::wait_until_async;
use nautilus_core::UnixNanos;
use nautilus_derive::{
    common::{
        consts::{HEADER_LYRA_SIGNATURE, HEADER_LYRA_TIMESTAMP, HEADER_LYRA_WALLET},
        enums::{DeriveInstrumentType, DeriveOrderSide, DeriveOrderType, DeriveTimeInForce},
        retry::http_retry_config,
    },
    http::{
        DeriveCredentials, DeriveHttpClient,
        query::{DeriveOrderParams, DeriveSignedEnvelope},
    },
    websocket::parse_candle_record,
};
use nautilus_model::data::BarType;
use nautilus_network::http::HttpClient;
use rstest::rstest;
use rust_decimal_macros::dec;
use serde_json::{Value, json};

const SESSION_KEY_HEX: &str = "0x2ae8be44db8a590d20bffbe3b6872df9b569147d3bf6801a35a28281a4816bbd";
const TEST_WALLET: &str = "0x000000000000000000000000000000000000aaaa";

#[derive(Clone, Default)]
struct CapturedRequest {
    path: String,
    headers: HashMap<String, String>,
    body: Value,
}

#[derive(Clone, Default)]
struct TestServerState {
    captured: Arc<tokio::sync::Mutex<Option<CapturedRequest>>>,
    response_body: Arc<tokio::sync::Mutex<Value>>,
    response_status: Arc<tokio::sync::Mutex<StatusCode>>,
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
            .clone()
            .expect("no request captured")
    }
}

async fn handle(
    path: &str,
    state: TestServerState,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    if let Some(delay) = *state.delay.lock().await {
        // Intentional mock response delay for timeout/retry cases, not a readiness wait.
        tokio::time::sleep(delay).await;
    }

    let parsed_body: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    let mut header_map = HashMap::new();

    for (name, value) in &headers {
        if let Ok(v) = value.to_str() {
            header_map.insert(name.as_str().to_lowercase(), v.to_string());
        }
    }

    *state.captured.lock().await = Some(CapturedRequest {
        path: path.to_string(),
        headers: header_map,
        body: parsed_body,
    });

    let status = *state.response_status.lock().await;
    let body = state.response_body.lock().await.clone();
    (status, Json(body)).into_response()
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
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    handle("/public/get_instrument", state, headers, body).await
}

async fn handle_order(
    State(state): State<TestServerState>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    handle("/private/order", state, headers, body).await
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
        .route("/private/order", post(handle_order))
        .route("/health", axum::routing::get(handle_health))
        .with_state(state);

    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });

    let health_url = format!("http://{addr}/health");
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
    *state.response_body.lock().await = load_json("perps/http_get_instruments_eth.json");
    let addr = start_mock_server(state.clone()).await;

    let client = DeriveHttpClient::new(base_url(addr), Some(5), None, None).unwrap();
    let instruments = client
        .get_instruments("ETH", DeriveInstrumentType::Perp, false)
        .await
        .unwrap();

    let captured = state.captured().await;
    assert_eq!(captured.path, "/public/get_instruments");
    assert_eq!(
        captured.body,
        json!({"currency": "ETH", "instrument_type": "perp", "expired": false})
    );
    assert_eq!(
        captured.headers.get("content-type").map(String::as_str),
        Some("application/json"),
    );
    assert!(
        !captured
            .headers
            .contains_key(&HEADER_LYRA_WALLET.to_lowercase())
    );
    assert!(
        !captured
            .headers
            .contains_key(&HEADER_LYRA_TIMESTAMP.to_lowercase())
    );
    assert!(
        !captured
            .headers
            .contains_key(&HEADER_LYRA_SIGNATURE.to_lowercase())
    );
    assert_eq!(instruments.len(), 1);
    assert_eq!(instruments[0].instrument_name, "ETH-PERP");
}

#[rstest]
#[tokio::test]
async fn test_get_instrument_posts_instrument_name() {
    let state = TestServerState::with_success_response();
    *state.response_body.lock().await = load_json("perps/http_get_instrument_eth.json");
    let addr = start_mock_server(state.clone()).await;

    let client = DeriveHttpClient::new(base_url(addr), Some(5), None, None).unwrap();
    let instrument = client.get_instrument("ETH-PERP").await.unwrap();

    let captured = state.captured().await;
    assert_eq!(captured.path, "/public/get_instrument");
    assert_eq!(captured.body, json!({"instrument_name": "ETH-PERP"}));
    assert_eq!(instrument.instrument_name, "ETH-PERP");
    assert_eq!(instrument.instrument_type, DeriveInstrumentType::Perp);
}

#[rstest]
#[tokio::test]
async fn test_send_private_attaches_all_lyra_auth_headers() {
    let state = TestServerState::with_success_response();
    *state.response_body.lock().await = json!({
        "id": 1,
        "result": {"order": load_json("perps/http_order_eth_partially_filled.json")},
    });
    let addr = start_mock_server(state.clone()).await;

    let client =
        DeriveHttpClient::with_credentials(base_url(addr), test_credentials(), Some(5), None, None)
            .unwrap();
    let payload = DeriveOrderParams {
        envelope: DeriveSignedEnvelope {
            subaccount_id: 42,
            nonce: 123,
            signer: "0xsigner".to_string(),
            signature_expiry_sec: 1_700_001_000,
            signature: "0x00".to_string(),
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
    };
    let order = client.submit_order(&payload).await.unwrap();

    let captured = state.captured().await;
    assert_eq!(captured.path, "/private/order");
    assert_eq!(
        captured.body,
        json!({
            "amount": "1",
            "direction": "buy",
            "instrument_name": "ETH-PERP",
            "label": "client-1",
            "limit_price": "3500",
            "max_fee": "1",
            "nonce": 123,
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
        .get(&HEADER_LYRA_WALLET.to_lowercase())
        .expect("wallet header present");
    assert_eq!(wallet, TEST_WALLET);

    let timestamp = captured
        .headers
        .get(&HEADER_LYRA_TIMESTAMP.to_lowercase())
        .expect("timestamp header present");
    let ts: u64 = timestamp.parse().expect("timestamp is a u64 millis string");
    assert!(ts > 1_700_000_000_000, "timestamp must be a recent unix ms");

    let signature = captured
        .headers
        .get(&HEADER_LYRA_SIGNATURE.to_lowercase())
        .expect("signature header present");
    assert!(signature.starts_with("0x"));
    assert_eq!(signature.len(), 2 + 130, "signature must be 65 bytes hex");

    assert_eq!(order.order_id, "abc-123");
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
    assert_eq!(ticker.instrument_name.as_str(), instrument_name);
    if expect_option_pricing {
        let pricing = ticker.option_pricing.expect("option ticker has pricing");
        assert_eq!(pricing.forward_price.to_string(), "3505");
    } else {
        assert!(ticker.option_pricing.is_none());
    }
}

#[rstest]
#[tokio::test]
#[ignore = "live network call against api.lyra.finance; run with --include-ignored"]
async fn test_live_get_ticker_smoke() {
    let client = DeriveHttpClient::new("https://api.lyra.finance", Some(10), None, None).unwrap();
    let ticker = client
        .get_ticker("ETH-PERP")
        .await
        .expect("live get_ticker must succeed");
    assert_eq!(ticker.instrument_name.as_str(), "ETH-PERP");
    // Perp tickers don't carry option_pricing; the option-chain path always
    // probes a specific option instrument, so any non-zero mark price proves
    // the wire shape is intact.
    assert!(!ticker.mark_price.is_zero());
}

#[rstest]
#[tokio::test]
#[ignore = "live network call against api.lyra.finance; run with --include-ignored"]
async fn test_live_get_candles_smoke() {
    let client = DeriveHttpClient::new("https://api.lyra.finance", Some(10), None, None).unwrap();
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    let start_ts = now - 2 * 3600;

    let candles = client
        .get_candles("ETH-PERP", start_ts, now, 900)
        .await
        .expect("live get_candles must succeed");
    assert!(!candles.is_empty(), "expected non-empty candle window");
    let first = &candles[0];
    assert!(first.high_price >= first.low_price);
    assert!(first.timestamp_bucket >= start_ts);
    assert!(first.timestamp_bucket <= now);

    let bar_type = BarType::from("ETH-PERP.DERIVE-15-MINUTE-LAST-EXTERNAL");
    let bar = parse_candle_record(first, bar_type, 2, 3, UnixNanos::default()).expect("bar parses");
    assert_eq!(bar.bar_type, bar_type);
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
