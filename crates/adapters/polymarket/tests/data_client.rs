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

//! Integration tests for the Polymarket data client.
//!
//! Exercises the `DataClient` trait surface (`request_instrument`,
//! `request_instruments`, `request_book_snapshot`, `request_trades`) against
//! axum mocks for the Gamma, CLOB public, and Data API endpoints.

use std::{net::SocketAddr, num::NonZeroUsize, path::PathBuf, sync::Arc, time::Duration};

use axum::{Router, extract::State, response::Json, routing::get};
use nautilus_common::{
    clients::DataClient,
    live::runner::replace_data_event_sender,
    messages::{
        DataEvent, DataResponse,
        data::{RequestBookSnapshot, RequestInstrument, RequestInstruments, RequestTrades},
    },
};
use nautilus_core::UUID4;
use nautilus_model::identifiers::{ClientId, InstrumentId};
use nautilus_network::{retry::RetryConfig, websocket::TransportBackend};
use nautilus_polymarket::{
    config::PolymarketDataClientConfig,
    data::PolymarketDataClient,
    http::{
        clob::PolymarketClobPublicClient, data_api::PolymarketDataApiHttpClient,
        gamma::PolymarketGammaHttpClient,
    },
    websocket::client::PolymarketWebSocketClient,
};
use rstest::rstest;
use serde_json::Value;

const TEST_CONDITION_ID: &str =
    "0x78443f961b9a65869dcb39359de9960165c7e5cbad0904eac7f29cd77872a63b";
const TEST_TOKEN_ID_YES: &str =
    "104239898038807136052399800151408521467737075933964991162589336683346093173875";

fn data_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test_data")
}

fn load_json(filename: &str) -> Value {
    let content = std::fs::read_to_string(data_path().join(filename))
        .unwrap_or_else(|_| panic!("failed to read {filename}"));
    serde_json::from_str(&content).expect("invalid json")
}

#[derive(Clone, Default)]
struct TestServerState {
    gamma_response: Arc<tokio::sync::Mutex<Option<Value>>>,
    book_response: Arc<tokio::sync::Mutex<Option<Value>>>,
    trades_response: Arc<tokio::sync::Mutex<Option<Value>>>,
}

async fn handle_gamma_markets(State(state): State<TestServerState>) -> Json<Value> {
    let body = state
        .gamma_response
        .lock()
        .await
        .clone()
        .unwrap_or_else(|| serde_json::json!([]));
    Json(body)
}

async fn handle_book(State(state): State<TestServerState>) -> Json<Value> {
    let body = state
        .book_response
        .lock()
        .await
        .clone()
        .unwrap_or_else(|| load_json("clob_book_response.json"));
    Json(body)
}

async fn handle_trades(State(state): State<TestServerState>) -> Json<Value> {
    let body = state
        .trades_response
        .lock()
        .await
        .clone()
        .unwrap_or_else(|| load_json("data_api_trades_response.json"));
    Json(body)
}

fn create_router(state: TestServerState) -> Router {
    Router::new()
        .route("/markets", get(handle_gamma_markets))
        .route("/book", get(handle_book))
        .route("/trades", get(handle_trades))
        .with_state(state)
}

async fn start_mock_server(state: TestServerState) -> SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind failed");
    let addr = listener.local_addr().expect("local_addr");
    let router = create_router(state);
    tokio::spawn(async move { axum::serve(listener, router).await.expect("serve failed") });
    addr
}

fn create_test_data_client(
    addr: SocketAddr,
) -> (
    PolymarketDataClient,
    tokio::sync::mpsc::UnboundedReceiver<DataEvent>,
) {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    // Use replace_ rather than set_ so this test can run on a thread that
    // already had a sender installed by another test in the same harness.
    replace_data_event_sender(tx);

    let base_url = format!("http://{addr}");
    let gamma = PolymarketGammaHttpClient::new(Some(base_url.clone()), 5, RetryConfig::default())
        .expect("gamma client");
    let clob_public = PolymarketClobPublicClient::new(Some(base_url.clone()), 5).expect("clob");
    let data_api = PolymarketDataApiHttpClient::new(Some(base_url.clone()), 5).expect("data_api");
    let ws = PolymarketWebSocketClient::new_market(
        Some(format!("ws://{addr}/ws/market")),
        false,
        TransportBackend::default(),
    );

    let config = PolymarketDataClientConfig {
        base_url_http: Some(base_url.clone()),
        base_url_ws: Some(format!("ws://{addr}/ws")),
        base_url_gamma: Some(base_url.clone()),
        base_url_data_api: Some(base_url),
        ..PolymarketDataClientConfig::default()
    };
    let client = PolymarketDataClient::new(
        ClientId::new("POLYMARKET"),
        config,
        gamma,
        clob_public,
        data_api,
        ws,
    );

    (client, rx)
}

fn gamma_market_fixture() -> Value {
    load_json("gamma_market.json")
}

fn yes_instrument_id() -> InstrumentId {
    InstrumentId::from(format!("{TEST_CONDITION_ID}-{TEST_TOKEN_ID_YES}.POLYMARKET").as_str())
}

async fn drain_data_events(
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<DataEvent>,
    timeout: Duration,
) -> Vec<DataEvent> {
    let mut events = Vec::new();
    let deadline = tokio::time::Instant::now() + timeout;
    while let Ok(Some(event)) = tokio::time::timeout_at(deadline, rx.recv()).await {
        events.push(event);
    }
    events
}

#[rstest]
#[tokio::test]
async fn test_request_instrument_publishes_event_and_response() {
    // Regression test for the `DataEvent::Instrument` publish in
    // `request_instrument` (data.rs:1183-1187). Without it the exec client
    // does not pick up newly fetched instruments and the WS dispatcher
    // logs `Unknown asset_id in order update`.
    let state = TestServerState::default();
    *state.gamma_response.lock().await = Some(serde_json::json!([gamma_market_fixture()]));
    let addr = start_mock_server(state).await;
    let (client, mut rx) = create_test_data_client(addr);

    let request = RequestInstrument::new(
        yes_instrument_id(),
        None,
        None,
        Some(ClientId::new("POLYMARKET")),
        UUID4::new(),
        nautilus_core::UnixNanos::default(),
        None,
    );
    client
        .request_instrument(request)
        .expect("request_instrument");

    let events = drain_data_events(&mut rx, Duration::from_secs(5)).await;

    let publish_count = events
        .iter()
        .filter(|e| matches!(e, DataEvent::Instrument(_)))
        .count();
    assert_eq!(
        publish_count, 1,
        "request_instrument must publish exactly one DataEvent::Instrument; got events: {events:?}"
    );

    let response_count = events
        .iter()
        .filter(|e| matches!(e, DataEvent::Response(DataResponse::Instrument(_))))
        .count();
    assert_eq!(
        response_count, 1,
        "request_instrument must also send a DataResponse::Instrument; got events: {events:?}"
    );
}

#[rstest]
#[tokio::test]
async fn test_request_instrument_not_found_emits_no_publish() {
    // Gamma returns an empty array. The instrument lookup misses, the
    // method logs an error, and no events are emitted.
    let state = TestServerState::default();
    *state.gamma_response.lock().await = Some(serde_json::json!([]));
    let addr = start_mock_server(state).await;
    let (client, mut rx) = create_test_data_client(addr);

    let request = RequestInstrument::new(
        yes_instrument_id(),
        None,
        None,
        None,
        UUID4::new(),
        nautilus_core::UnixNanos::default(),
        None,
    );
    client
        .request_instrument(request)
        .expect("request_instrument");

    let events = drain_data_events(&mut rx, Duration::from_millis(500)).await;
    assert!(
        events.is_empty(),
        "missing instrument must not produce any DataEvents; got: {events:?}",
    );
}

#[rstest]
#[tokio::test]
async fn test_request_instruments_emits_response() {
    // Baseline for the bulk request path. Today this only sends a
    // DataResponse and does NOT publish per-instrument events. The audit
    // flagged this as a follow-up vs. `request_instrument`. Pin the
    // current behaviour so any future change is deliberate.
    let state = TestServerState::default();
    *state.gamma_response.lock().await = Some(serde_json::json!([gamma_market_fixture()]));
    let addr = start_mock_server(state).await;
    let (client, mut rx) = create_test_data_client(addr);

    let request = RequestInstruments::new(
        None,
        None,
        Some(ClientId::new("POLYMARKET")),
        None,
        UUID4::new(),
        nautilus_core::UnixNanos::default(),
        None,
    );
    client
        .request_instruments(request)
        .expect("request_instruments");

    let events = drain_data_events(&mut rx, Duration::from_secs(5)).await;

    let response_count = events
        .iter()
        .filter(|e| matches!(e, DataEvent::Response(DataResponse::Instruments(_))))
        .count();
    assert_eq!(
        response_count, 1,
        "request_instruments must send a DataResponse::Instruments; got: {events:?}",
    );

    let publish_count = events
        .iter()
        .filter(|e| matches!(e, DataEvent::Instrument(_)))
        .count();
    assert_eq!(
        publish_count, 0,
        "request_instruments does not currently publish per-instrument events; \
         if it ever does, update this test deliberately",
    );
}

#[rstest]
#[tokio::test]
async fn test_request_book_snapshot_returns_book_response() {
    // request_book_snapshot needs the instrument cached; we prime the
    // cache by issuing a request_instrument first, then the book snapshot.
    let state = TestServerState::default();
    *state.gamma_response.lock().await = Some(serde_json::json!([gamma_market_fixture()]));
    let addr = start_mock_server(state).await;
    let (client, mut rx) = create_test_data_client(addr);

    let instrument_id = yes_instrument_id();

    let request = RequestInstrument::new(
        instrument_id,
        None,
        None,
        None,
        UUID4::new(),
        nautilus_core::UnixNanos::default(),
        None,
    );
    client.request_instrument(request).expect("prime cache");
    let _prime_events = drain_data_events(&mut rx, Duration::from_secs(5)).await;

    let snapshot_request = RequestBookSnapshot::new(
        instrument_id,
        Some(NonZeroUsize::new(10).unwrap()),
        Some(ClientId::new("POLYMARKET")),
        UUID4::new(),
        nautilus_core::UnixNanos::default(),
        None,
    );
    client
        .request_book_snapshot(snapshot_request)
        .expect("request_book_snapshot");

    let events = drain_data_events(&mut rx, Duration::from_secs(5)).await;
    let book_response_count = events
        .iter()
        .filter(|e| matches!(e, DataEvent::Response(DataResponse::Book(_))))
        .count();
    assert_eq!(
        book_response_count, 1,
        "request_book_snapshot must send a DataResponse::Book; got: {events:?}",
    );
}

#[rstest]
#[tokio::test]
async fn test_request_trades_returns_trades_response() {
    // The data API returns trades for all outcomes of a condition; the
    // adapter filters by token_id. Build an inline fixture where two
    // trades match TEST_TOKEN_ID_YES and one belongs to a sibling token,
    // so we exercise the filter and assert exact counts/fields rather
    // than a bare "response was emitted" check.
    let other_token = "0".repeat(76);
    let trades_fixture = serde_json::json!([
        {
            "asset": TEST_TOKEN_ID_YES,
            "conditionId": TEST_CONDITION_ID,
            "side": "BUY",
            "price": 0.55,
            "size": 100.0,
            "timestamp": 1_710_000_000,
            "transactionHash": "0xabc123def456789012345678901234567890abcdef1234567890abcdef123456",
            "proxyWallet": "0x1111111111111111111111111111111111111111",
            "title": "GTA VI",
            "slug": "gta-vi"
        },
        {
            "asset": other_token,
            "conditionId": TEST_CONDITION_ID,
            "side": "SELL",
            "price": 0.45,
            "size": 50.0,
            "timestamp": 1_710_000_010,
            "transactionHash": "0xdef456789012345678901234567890abcdef1234567890abcdef123456789abc",
            "proxyWallet": "0x2222222222222222222222222222222222222222",
            "title": "GTA VI",
            "slug": "gta-vi"
        },
        {
            "asset": TEST_TOKEN_ID_YES,
            "conditionId": TEST_CONDITION_ID,
            "side": "SELL",
            "price": 0.53,
            "size": 25.0,
            "timestamp": 1_710_000_020,
            "transactionHash": "0xfeedface789012345678901234567890abcdef1234567890abcdef123456beef",
            "proxyWallet": "0x3333333333333333333333333333333333333333",
            "title": "GTA VI",
            "slug": "gta-vi"
        }
    ]);

    let state = TestServerState::default();
    *state.gamma_response.lock().await = Some(serde_json::json!([gamma_market_fixture()]));
    *state.trades_response.lock().await = Some(trades_fixture);
    let addr = start_mock_server(state).await;
    let (client, mut rx) = create_test_data_client(addr);

    let instrument_id = yes_instrument_id();

    // Prime cache so request_trades can resolve the instrument.
    let request = RequestInstrument::new(
        instrument_id,
        None,
        None,
        None,
        UUID4::new(),
        nautilus_core::UnixNanos::default(),
        None,
    );
    client.request_instrument(request).expect("prime cache");
    let _prime_events = drain_data_events(&mut rx, Duration::from_secs(5)).await;

    let trades_request = RequestTrades::new(
        instrument_id,
        None,
        None,
        Some(NonZeroUsize::new(50).unwrap()),
        Some(ClientId::new("POLYMARKET")),
        UUID4::new(),
        nautilus_core::UnixNanos::default(),
        None,
    );
    client
        .request_trades(trades_request)
        .expect("request_trades");

    let events = drain_data_events(&mut rx, Duration::from_secs(5)).await;
    let trades_response = events
        .iter()
        .find_map(|e| match e {
            DataEvent::Response(DataResponse::Trades(r)) => Some(r),
            _ => None,
        })
        .unwrap_or_else(|| panic!("expected DataResponse::Trades; got: {events:?}"));

    assert_eq!(
        trades_response.instrument_id, instrument_id,
        "response must carry the requested instrument_id",
    );
    // Two of three trades match TEST_TOKEN_ID_YES; the sibling token is filtered out.
    assert_eq!(
        trades_response.data.len(),
        2,
        "response must contain exactly the two trades for the requested token",
    );
    let prices: Vec<f64> = trades_response
        .data
        .iter()
        .map(|t| t.price.as_f64())
        .collect();
    assert!(
        prices.contains(&0.55),
        "response missing 0.55 trade: {prices:?}"
    );
    assert!(
        prices.contains(&0.53),
        "response missing 0.53 trade: {prices:?}"
    );
    assert!(
        !prices.contains(&0.45),
        "response leaked sibling-token trade: {prices:?}",
    );
}
