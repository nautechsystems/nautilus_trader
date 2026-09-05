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
//! Exercises selected `DataClient` subscription and request surfaces against axum mocks for the
//! Gamma, CLOB public, and Data API endpoints.

use std::{
    net::SocketAddr,
    num::NonZeroUsize,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};

use axum::{
    Router,
    extract::{
        State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::StatusCode,
    response::{IntoResponse, Json, Response},
    routing::get,
};
use futures_util::StreamExt;
use jiff::{SignedDuration, Timestamp, tz::Offset};
use nautilus_common::{
    clients::DataClient,
    live::runner::{replace_data_event_sender, replace_system_event_sender},
    messages::{
        DataEvent, DataResponse, SystemEvent,
        data::{
            RequestBookSnapshot, RequestInstrument, RequestInstruments, RequestTrades,
            SubscribeBookDepth10, SubscribeCustomData, SubscribeInstrument,
            SubscribeInstrumentClose, SubscribeInstrumentStatus, SubscribeQuotes,
            UnsubscribeInstrument,
        },
        system::SocketState,
    },
    testing::wait_until_async,
};
use nautilus_core::{Params, UUID4, UnixNanos};
use nautilus_live::{SocketReconnectRegistry, SocketReconnectRequestOutcome};
use nautilus_model::{
    data::{Data as NautilusData, DataType},
    enums::BookType,
    identifiers::InstrumentId,
    instruments::InstrumentAny,
};
use nautilus_network::{retry::RetryConfig, websocket::TransportBackend};
use nautilus_polymarket::{
    common::consts::{POLYMARKET_CLIENT_ID, POLYMARKET_VENUE, WS_DEFAULT_SUBSCRIPTIONS},
    config::PolymarketDataClientConfig,
    data::PolymarketDataClient,
    data_types::PolymarketRtdsCryptoTwap,
    http::{
        clob::PolymarketClobPublicClient, data_api::PolymarketDataApiHttpClient,
        gamma::PolymarketGammaHttpClient,
    },
    websocket::pool::PolymarketMarketConnectionPool,
};
use nautilus_testkit::events::{collect_data_events_until_response, drain_data_events};
use rstest::rstest;
use rust_decimal_macros::dec;
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

fn future_end_date_string() -> String {
    let future_date = Offset::UTC
        .to_datetime(Timestamp::now() + SignedDuration::from_hours(24 * 365))
        .date();
    format!("{}T00:00:00Z", future_date.strftime("%Y-%m-%d"))
}

fn set_future_end_date(value: &mut Value) {
    let end_date = future_end_date_string();
    let end_date_iso = end_date[..10].to_string();

    if let Some(root) = value.as_object_mut() {
        root.insert("endDate".to_string(), Value::String(end_date.clone()));
        root.insert("endDateIso".to_string(), Value::String(end_date_iso));

        if let Some(events) = root.get_mut("events").and_then(Value::as_array_mut) {
            for event in events {
                if let Some(event_obj) = event.as_object_mut() {
                    event_obj.insert("endDate".to_string(), Value::String(end_date.clone()));
                }
            }
        }
    }
}

fn gamma_market_request_fixture() -> Value {
    let mut value = load_json("gamma_market.json");
    set_future_end_date(&mut value);
    value
}

#[derive(Clone, Default)]
struct TestServerState {
    gamma_response: Arc<tokio::sync::Mutex<Option<Value>>>,
    gamma_request_count: Arc<AtomicUsize>,
    book_response: Arc<tokio::sync::Mutex<Option<Value>>>,
    trades_response: Arc<tokio::sync::Mutex<Option<Value>>>,
    trades_request_count: Arc<AtomicUsize>,
    trades_error: Arc<AtomicBool>,
    market_payloads: Arc<tokio::sync::Mutex<Vec<Value>>>,
    rtds_payloads: Arc<tokio::sync::Mutex<Vec<Value>>>,
}

async fn handle_gamma_markets(State(state): State<TestServerState>) -> Json<Value> {
    state.gamma_request_count.fetch_add(1, Ordering::Relaxed);
    let body = state
        .gamma_response
        .lock()
        .await
        .clone()
        .unwrap_or_else(|| serde_json::json!([]));
    Json(body)
}

async fn handle_gamma_markets_keyset(State(state): State<TestServerState>) -> Json<Value> {
    let Json(markets) = handle_gamma_markets(State(state)).await;
    Json(serde_json::json!({"markets": markets}))
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

async fn handle_trades(State(state): State<TestServerState>) -> Response {
    state.trades_request_count.fetch_add(1, Ordering::Relaxed);

    if state.trades_error.load(Ordering::Relaxed) {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }

    let body = state
        .trades_response
        .lock()
        .await
        .clone()
        .unwrap_or_else(|| load_json("data_api_trades_response.json"));
    Json(body).into_response()
}

async fn handle_market_upgrade(
    ws: WebSocketUpgrade,
    State(state): State<TestServerState>,
) -> Response {
    ws.on_upgrade(move |socket| handle_market_socket(socket, state))
}

async fn handle_market_socket(mut socket: WebSocket, state: TestServerState) {
    while let Some(result) = socket.next().await {
        let Ok(msg) = result else { break };

        match msg {
            Message::Text(text) => {
                let Ok(payload) = serde_json::from_str::<Value>(&text) else {
                    continue;
                };

                if payload.get("type").and_then(Value::as_str) == Some("market")
                    || payload.get("operation").and_then(Value::as_str).is_some()
                {
                    state.market_payloads.lock().await.push(payload);
                }
            }
            Message::Close(_) => break,
            _ => {}
        }
    }
}

async fn handle_rtds_socket(mut socket: WebSocket, state: TestServerState) {
    while let Some(result) = socket.next().await {
        let Ok(message) = result else { break };

        match message {
            Message::Text(text) => {
                let Ok(payload) = serde_json::from_str::<Value>(&text) else {
                    continue;
                };
                let is_twap_subscribe = payload.get("action").and_then(Value::as_str)
                    == Some("subscribe")
                    && payload
                        .get("subscriptions")
                        .and_then(Value::as_array)
                        .is_some_and(|subscriptions| {
                            subscriptions.iter().any(|subscription| {
                                subscription.get("topic").and_then(Value::as_str)
                                    == Some("crypto_prices_twap_sixty")
                            })
                        });
                state.rtds_payloads.lock().await.push(payload);

                if is_twap_subscribe {
                    let update = load_json("rtds_crypto_twap_sixty_update.json").to_string();
                    if socket.send(Message::Text(update.into())).await.is_err() {
                        break;
                    }
                }
            }
            Message::Ping(data) => {
                if socket.send(Message::Pong(data)).await.is_err() {
                    break;
                }
            }
            Message::Close(_) => break,
            _ => {}
        }
    }
}

async fn handle_rtds_upgrade(
    ws: WebSocketUpgrade,
    State(state): State<TestServerState>,
) -> Response {
    ws.on_upgrade(move |socket| handle_rtds_socket(socket, state))
}

fn create_router(state: TestServerState) -> Router {
    Router::new()
        .route("/markets", get(handle_gamma_markets))
        .route("/markets/keyset", get(handle_gamma_markets_keyset))
        .route("/book", get(handle_book))
        .route("/trades", get(handle_trades))
        .route("/ws/market", get(handle_market_upgrade))
        .route("/rtds", get(handle_rtds_upgrade))
        .with_state(state)
}

fn crypto_twap_data_type(symbol: &str, window_seconds: u64) -> DataType {
    let mut metadata = Params::new();
    metadata.insert("symbol".to_string(), Value::String(symbol.to_string()));
    metadata.insert(
        "window_seconds".to_string(),
        Value::Number(window_seconds.into()),
    );
    DataType::new("PolymarketRtdsCryptoTwap", Some(metadata), None)
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
    create_test_data_client_with_new_markets(addr, false)
}

#[rstest]
#[tokio::test]
async fn test_connect_emits_market_socket_state_change() {
    let state = TestServerState::default();
    let addr = start_mock_server(state).await;
    let (system_tx, mut system_rx) = tokio::sync::mpsc::unbounded_channel();
    replace_system_event_sender(system_tx);
    let registry = SocketReconnectRegistry::default();
    let (mut client, _data_rx) = registry.scope(|| create_test_data_client(addr));

    client.connect().await.expect("connect data client");

    let event = tokio::time::timeout(Duration::from_secs(5), system_rx.recv())
        .await
        .expect("wait for socket state change")
        .expect("system event channel closed");
    let SystemEvent::SocketState(change) = event;
    let endpoint = ustr::Ustr::from("polymarket-market-streams");
    let handle = registry.handle(*POLYMARKET_CLIENT_ID, endpoint).unwrap();

    assert_eq!(change.client_id, *POLYMARKET_CLIENT_ID);
    assert_eq!(change.venue, Some(*POLYMARKET_VENUE));
    assert_eq!(change.endpoint, endpoint);
    assert_eq!(change.state, SocketState::Connected);
    assert_eq!(
        handle.request_reconnect(),
        SocketReconnectRequestOutcome::Accepted
    );

    let event = tokio::time::timeout(Duration::from_secs(5), system_rx.recv())
        .await
        .expect("wait for socket state change")
        .expect("system event channel closed");
    let SystemEvent::SocketState(change) = event;
    assert_eq!(change.client_id, *POLYMARKET_CLIENT_ID);
    assert_eq!(change.venue, Some(*POLYMARKET_VENUE));
    assert_eq!(change.endpoint, endpoint);
    assert_eq!(change.state, SocketState::Disconnected);

    client.disconnect().await.expect("disconnect data client");
    assert!(registry.handle(*POLYMARKET_CLIENT_ID, endpoint).is_none());
}

#[rstest]
#[tokio::test]
async fn test_subscribe_crypto_twap_sends_exact_topic_and_emits_exact_update() {
    let state = TestServerState::default();
    let addr = start_mock_server(state.clone()).await;
    let (mut client, mut data_rx) = create_test_data_client(addr);
    let data_type = crypto_twap_data_type("BTC/USD", 60);

    client
        .subscribe(SubscribeCustomData::new(
            Some(*POLYMARKET_CLIENT_ID),
            None,
            data_type.clone(),
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ))
        .expect("subscribe 60-second TWAP");
    client.connect().await.expect("connect data client");

    let custom = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let event = data_rx.recv().await.expect("data event channel closed");
            if let DataEvent::Data(NautilusData::Custom(custom)) = event {
                break custom;
            }
        }
    })
    .await
    .expect("wait for TWAP custom data");
    let payload = custom
        .data
        .as_any()
        .downcast_ref::<PolymarketRtdsCryptoTwap>()
        .expect("PolymarketRtdsCryptoTwap");
    let requests = state.rtds_payloads.lock().await.clone();

    assert_eq!(custom.data_type, data_type);
    assert_eq!(payload.symbol, "btc/usd");
    assert_eq!(payload.window_seconds, 60);
    assert_eq!(payload.value, dec!(65000.123456789012345678));
    assert_eq!(payload.observation_timestamp_ms, 1772752581815);
    assert_eq!(payload.message_timestamp_ms, 1772752582004);
    assert_eq!(
        requests,
        vec![serde_json::json!({
            "action": "subscribe",
            "subscriptions": [{
                "topic": "crypto_prices_twap_sixty",
                "type": "update",
            }],
        })],
    );

    client.disconnect().await.expect("disconnect data client");
}

fn create_test_data_client_with_new_markets(
    addr: SocketAddr,
    subscribe_new_markets: bool,
) -> (
    PolymarketDataClient,
    tokio::sync::mpsc::UnboundedReceiver<DataEvent>,
) {
    create_test_data_client_with_resolution_options(
        addr,
        subscribe_new_markets,
        PolymarketDataClientConfig::default().resolve_poll_enabled,
    )
}

fn create_test_data_client_with_resolution_options(
    addr: SocketAddr,
    subscribe_new_markets: bool,
    resolve_poll_enabled: bool,
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
    let ws = PolymarketMarketConnectionPool::new(
        Some(format!("ws://{addr}/ws/market")),
        subscribe_new_markets,
        TransportBackend::default(),
        WS_DEFAULT_SUBSCRIPTIONS,
    );

    let config = PolymarketDataClientConfig {
        base_url_http: Some(base_url.clone()),
        base_url_ws: Some(format!("ws://{addr}/ws")),
        base_url_rtds: Some(format!("ws://{addr}/rtds")),
        base_url_gamma: Some(base_url.clone()),
        base_url_data_api: Some(base_url),
        subscribe_new_markets,
        resolve_poll_enabled,
        ..PolymarketDataClientConfig::default()
    };
    let client = PolymarketDataClient::new(
        *POLYMARKET_CLIENT_ID,
        config,
        gamma,
        clob_public,
        data_api,
        ws,
    );

    (client, rx)
}

fn yes_instrument_id() -> InstrumentId {
    InstrumentId::from(format!("{TEST_CONDITION_ID}-{TEST_TOKEN_ID_YES}.POLYMARKET").as_str())
}

#[derive(Clone, Copy)]
enum UnsupportedGenericSubscription {
    BookDepth10,
}

async fn wait_for_market_payload_count(
    state: &TestServerState,
    expected: usize,
    empty_assets: bool,
    timeout: Duration,
) {
    wait_until_async(
        || {
            let state = state.clone();
            async move {
                state
                    .market_payloads
                    .lock()
                    .await
                    .iter()
                    .filter(|payload| {
                        payload
                            .get("assets_ids")
                            .and_then(Value::as_array)
                            .is_some_and(|ids| ids.is_empty() == empty_assets)
                    })
                    .count()
                    >= expected
            }
        },
        timeout,
    )
    .await;
}

async fn market_payload_count(state: &TestServerState, empty_assets: bool) -> usize {
    state
        .market_payloads
        .lock()
        .await
        .iter()
        .filter(|payload| {
            payload
                .get("assets_ids")
                .and_then(Value::as_array)
                .is_some_and(|ids| ids.is_empty() == empty_assets)
        })
        .count()
}

#[rstest]
#[tokio::test]
async fn test_request_instrument_fetches_fresh_definition() {
    let state = TestServerState::default();
    *state.gamma_response.lock().await = Some(serde_json::json!([gamma_market_request_fixture()]));
    let addr = start_mock_server(state.clone()).await;
    let (client, mut rx) = create_test_data_client(addr);

    let request_id = UUID4::new();
    let request = RequestInstrument::new(
        yes_instrument_id(),
        None,
        None,
        Some(*POLYMARKET_CLIENT_ID),
        request_id,
        nautilus_core::UnixNanos::default(),
        None,
    );
    client
        .request_instrument(request)
        .expect("request_instrument");

    let events =
        collect_data_events_until_response(&mut rx, request_id, Duration::from_secs(5)).await;

    let publish_count = events
        .iter()
        .filter(|e| matches!(e, DataEvent::Instrument(_)))
        .count();
    assert_eq!(
        publish_count, 1,
        "request_instrument must publish exactly one DataEvent::Instrument; events were: {events:?}"
    );

    let response_count = events
        .iter()
        .filter(|e| matches!(e, DataEvent::Response(DataResponse::Instrument(_))))
        .count();
    assert_eq!(
        response_count, 1,
        "request_instrument must also send a DataResponse::Instrument; events were: {events:?}"
    );

    let expected_description = "Fresh Gamma instrument definition";
    let mut updated_market = gamma_market_request_fixture();
    updated_market["question"] = Value::String(expected_description.to_string());
    *state.gamma_response.lock().await = Some(serde_json::json!([updated_market]));
    let second_request_id = UUID4::new();
    client
        .request_instrument(RequestInstrument::new(
            yes_instrument_id(),
            None,
            None,
            Some(*POLYMARKET_CLIENT_ID),
            second_request_id,
            UnixNanos::default(),
            None,
        ))
        .expect("second request_instrument");

    let second_events = [
        tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .expect("second instrument event timeout")
            .expect("second instrument event"),
        tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .expect("second instrument response timeout")
            .expect("second instrument response"),
    ];
    assert_eq!(state.gamma_request_count.load(Ordering::Relaxed), 2);
    let response = second_events
        .iter()
        .find_map(|event| match event {
            DataEvent::Response(DataResponse::Instrument(response)) => Some(response),
            _ => None,
        })
        .expect("second instrument response");
    assert_eq!(response.correlation_id, second_request_id);
    assert_eq!(response.client_id, *POLYMARKET_CLIENT_ID);
    assert_eq!(response.instrument_id, yes_instrument_id());
    match &response.data {
        InstrumentAny::BinaryOption(instrument) => {
            assert_eq!(instrument.description, Some(expected_description.into()));
        }
        other => panic!("expected BinaryOption response, received {other:?}"),
    }
}

#[rstest]
#[tokio::test]
async fn test_subscribe_instrument_does_not_replay_cached_definition() {
    let state = TestServerState::default();
    *state.gamma_response.lock().await = Some(serde_json::json!([gamma_market_request_fixture()]));
    let addr = start_mock_server(state).await;
    let (mut client, mut rx) = create_test_data_client(addr);
    let instrument_id = yes_instrument_id();

    let request_id = UUID4::new();
    client
        .request_instrument(RequestInstrument::new(
            instrument_id,
            None,
            None,
            Some(*POLYMARKET_CLIENT_ID),
            request_id,
            UnixNanos::default(),
            None,
        ))
        .expect("prime cache");
    let prime_events =
        collect_data_events_until_response(&mut rx, request_id, Duration::from_secs(5)).await;
    assert_eq!(
        prime_events
            .iter()
            .filter(|event| matches!(event, DataEvent::Instrument(_)))
            .count(),
        1,
    );

    client
        .subscribe_instrument(SubscribeInstrument::new(
            instrument_id,
            Some(*POLYMARKET_CLIENT_ID),
            None,
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ))
        .expect("subscribe instrument");

    assert!(matches!(
        rx.try_recv(),
        Err(tokio::sync::mpsc::error::TryRecvError::Empty)
    ));

    client
        .unsubscribe_instrument(&UnsubscribeInstrument::new(
            instrument_id,
            Some(*POLYMARKET_CLIENT_ID),
            None,
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ))
        .expect("unsubscribe instrument");
    assert!(matches!(
        rx.try_recv(),
        Err(tokio::sync::mpsc::error::TryRecvError::Empty)
    ));
}

#[rstest]
#[case::book_depth10(
    UnsupportedGenericSubscription::BookDepth10,
    "Polymarket does not support OrderBookDepth10 subscriptions; use managed L2_MBP order book deltas"
)]
#[tokio::test]
async fn test_unsupported_generic_subscription_returns_exact_reason(
    #[case] subscription: UnsupportedGenericSubscription,
    #[case] expected: &str,
) {
    let state = TestServerState::default();
    let addr = start_mock_server(state).await;
    let (mut client, _rx) = create_test_data_client(addr);
    let instrument_id = yes_instrument_id();

    let result = match subscription {
        UnsupportedGenericSubscription::BookDepth10 => {
            client.subscribe_book_depth10(SubscribeBookDepth10::new(
                instrument_id,
                BookType::L2_MBP,
                Some(*POLYMARKET_CLIENT_ID),
                None,
                UUID4::new(),
                UnixNanos::default(),
                NonZeroUsize::new(10),
                true,
                None,
                None,
            ))
        }
    };

    let error = result.expect_err("subscription should be unsupported");
    assert_eq!(error.to_string(), expected);
}

#[rstest]
#[tokio::test]
async fn test_instrument_status_and_close_subscriptions_are_accepted_without_position() {
    let state = TestServerState::default();
    let addr = start_mock_server(state).await;
    let (mut client, _rx) = create_test_data_client(addr);
    let instrument_id = yes_instrument_id();

    client
        .subscribe_instrument_status(SubscribeInstrumentStatus::new(
            instrument_id,
            Some(*POLYMARKET_CLIENT_ID),
            None,
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ))
        .expect("instrument status subscription should establish resolution monitoring");
    client
        .subscribe_instrument_close(SubscribeInstrumentClose::new(
            instrument_id,
            Some(*POLYMARKET_CLIENT_ID),
            None,
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ))
        .expect("instrument close subscription should establish resolution monitoring");
}

#[rstest]
#[tokio::test]
async fn test_request_instrument_not_found_emits_no_publish() {
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

    // A missing instrument produces no terminal response, so this is the one deliberate bounded
    // absence window in these tests.
    let events = drain_data_events(&mut rx, Duration::from_millis(500)).await;
    assert!(
        events.is_empty(),
        "missing instrument must not produce any DataEvents; events were: {events:?}",
    );
}

#[rstest]
#[tokio::test]
async fn test_request_instruments_emits_response() {
    let state = TestServerState::default();
    *state.gamma_response.lock().await = Some(serde_json::json!([gamma_market_request_fixture()]));
    let addr = start_mock_server(state.clone()).await;
    let (client, mut rx) = create_test_data_client(addr);

    let instrument_request_id = UUID4::new();
    let instrument_request = RequestInstrument::new(
        yes_instrument_id(),
        None,
        None,
        Some(*POLYMARKET_CLIENT_ID),
        instrument_request_id,
        nautilus_core::UnixNanos::default(),
        None,
    );
    client
        .request_instrument(instrument_request)
        .expect("request_instrument");
    let _ =
        collect_data_events_until_response(&mut rx, instrument_request_id, Duration::from_secs(5))
            .await;

    *state.gamma_response.lock().await = Some(serde_json::json!([]));

    let request_id = UUID4::new();
    let request = RequestInstruments::new(
        None,
        None,
        Some(*POLYMARKET_CLIENT_ID),
        None,
        request_id,
        nautilus_core::UnixNanos::default(),
        None,
    );
    client
        .request_instruments(request)
        .expect("request_instruments");

    let events =
        collect_data_events_until_response(&mut rx, request_id, Duration::from_secs(5)).await;

    let response_count = events
        .iter()
        .filter(|e| matches!(e, DataEvent::Response(DataResponse::Instruments(_))))
        .count();
    assert_eq!(
        response_count, 1,
        "request_instruments must send a DataResponse::Instruments; events were: {events:?}",
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

    let response = events
        .iter()
        .find_map(|event| match event {
            DataEvent::Response(DataResponse::Instruments(response)) => Some(response),
            _ => None,
        })
        .expect("instruments response");
    assert_eq!(response.correlation_id, request_id);
    assert_eq!(response.client_id, *POLYMARKET_CLIENT_ID);
    assert_eq!(response.venue, *POLYMARKET_VENUE);
    assert!(response.start.is_none());
    assert!(response.end.is_none());
    assert!(response.params.is_none());
    assert_eq!(
        response.data.len(),
        0,
        "request_instruments should return the fresh Gamma response instead of the cached instrument",
    );
}

#[rstest]
#[tokio::test]
async fn test_request_book_snapshot_returns_book_response() {
    let state = TestServerState::default();
    *state.gamma_response.lock().await = Some(serde_json::json!([gamma_market_request_fixture()]));
    let addr = start_mock_server(state).await;
    let (client, mut rx) = create_test_data_client(addr);

    let instrument_id = yes_instrument_id();

    let request_id = UUID4::new();
    let request = RequestInstrument::new(
        instrument_id,
        None,
        None,
        None,
        request_id,
        nautilus_core::UnixNanos::default(),
        None,
    );
    client.request_instrument(request).expect("prime cache");
    let _prime_events =
        collect_data_events_until_response(&mut rx, request_id, Duration::from_secs(5)).await;

    let snapshot_request_id = UUID4::new();
    let snapshot_request = RequestBookSnapshot::new(
        instrument_id,
        Some(NonZeroUsize::new(10).unwrap()),
        Some(*POLYMARKET_CLIENT_ID),
        snapshot_request_id,
        nautilus_core::UnixNanos::default(),
        None,
    );
    client
        .request_book_snapshot(snapshot_request)
        .expect("request_book_snapshot");

    let events =
        collect_data_events_until_response(&mut rx, snapshot_request_id, Duration::from_secs(5))
            .await;
    let book_response_count = events
        .iter()
        .filter(|e| matches!(e, DataEvent::Response(DataResponse::Book(_))))
        .count();
    assert_eq!(
        book_response_count, 1,
        "request_book_snapshot must send a DataResponse::Book; events were: {events:?}",
    );
}

#[rstest]
#[tokio::test]
async fn test_request_trades_returns_trades_response() {
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
    *state.gamma_response.lock().await = Some(serde_json::json!([gamma_market_request_fixture()]));
    *state.trades_response.lock().await = Some(trades_fixture);
    let addr = start_mock_server(state).await;
    let (client, mut rx) = create_test_data_client(addr);

    let instrument_id = yes_instrument_id();

    let request_id = UUID4::new();
    let request = RequestInstrument::new(
        instrument_id,
        None,
        None,
        None,
        request_id,
        nautilus_core::UnixNanos::default(),
        None,
    );
    client.request_instrument(request).expect("prime cache");
    let _prime_events =
        collect_data_events_until_response(&mut rx, request_id, Duration::from_secs(5)).await;

    let trades_request_id = UUID4::new();
    let trades_request = RequestTrades::new(
        instrument_id,
        None,
        None,
        Some(NonZeroUsize::new(50).unwrap()),
        Some(*POLYMARKET_CLIENT_ID),
        trades_request_id,
        nautilus_core::UnixNanos::default(),
        None,
    );
    client
        .request_trades(trades_request)
        .expect("request_trades");

    let events =
        collect_data_events_until_response(&mut rx, trades_request_id, Duration::from_secs(5))
            .await;
    let trades_response = events
        .iter()
        .find_map(|e| match e {
            DataEvent::Response(DataResponse::Trades(r)) => Some(r),
            _ => None,
        })
        .unwrap_or_else(|| panic!("expected DataResponse::Trades; events were: {events:?}"));

    assert_eq!(
        trades_response.instrument_id, instrument_id,
        "response must carry the requested instrument_id",
    );
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

#[rstest]
#[tokio::test]
async fn test_request_trades_emits_no_response_on_data_api_error() {
    let state = TestServerState::default();
    *state.gamma_response.lock().await = Some(serde_json::json!([gamma_market_request_fixture()]));
    state.trades_error.store(true, Ordering::Relaxed);
    let addr = start_mock_server(state.clone()).await;
    let (client, mut rx) = create_test_data_client(addr);
    let instrument_id = yes_instrument_id();

    let request_id = UUID4::new();
    client
        .request_instrument(RequestInstrument::new(
            instrument_id,
            None,
            None,
            None,
            request_id,
            UnixNanos::default(),
            None,
        ))
        .expect("prime cache");
    let _prime_events =
        collect_data_events_until_response(&mut rx, request_id, Duration::from_secs(5)).await;

    let trades_request_id = UUID4::new();
    client
        .request_trades(RequestTrades::new(
            instrument_id,
            None,
            None,
            None,
            Some(*POLYMARKET_CLIENT_ID),
            trades_request_id,
            UnixNanos::default(),
            None,
        ))
        .expect("request_trades");

    wait_until_async(
        || async { state.trades_request_count.load(Ordering::Relaxed) == 1 },
        Duration::from_secs(5),
    )
    .await;
    let events = drain_data_events(&mut rx, Duration::from_secs(1)).await;
    let response_count = events
        .iter()
        .filter(|event| {
            matches!(
                event,
                DataEvent::Response(response) if response.correlation_id() == &trades_request_id
            )
        })
        .count();

    assert_eq!(state.trades_request_count.load(Ordering::Relaxed), 1);
    assert_eq!(
        response_count, 0,
        "Data API errors must not emit a correlated success; events were: {events:?}",
    );
}

#[rstest]
#[tokio::test]
async fn test_request_trades_emits_one_empty_response_for_empty_result() {
    let state = TestServerState::default();
    *state.gamma_response.lock().await = Some(serde_json::json!([gamma_market_request_fixture()]));
    *state.trades_response.lock().await = Some(serde_json::json!([]));
    let addr = start_mock_server(state).await;
    let (client, mut rx) = create_test_data_client(addr);
    let instrument_id = yes_instrument_id();

    let request_id = UUID4::new();
    client
        .request_instrument(RequestInstrument::new(
            instrument_id,
            None,
            None,
            None,
            request_id,
            UnixNanos::default(),
            None,
        ))
        .expect("prime cache");
    let _prime_events =
        collect_data_events_until_response(&mut rx, request_id, Duration::from_secs(5)).await;

    let trades_request_id = UUID4::new();
    client
        .request_trades(RequestTrades::new(
            instrument_id,
            None,
            None,
            None,
            Some(*POLYMARKET_CLIENT_ID),
            trades_request_id,
            UnixNanos::default(),
            None,
        ))
        .expect("request_trades");

    let events =
        collect_data_events_until_response(&mut rx, trades_request_id, Duration::from_secs(5))
            .await;
    let trades_responses = events
        .iter()
        .filter_map(|event| match event {
            DataEvent::Response(DataResponse::Trades(response))
                if response.correlation_id == trades_request_id =>
            {
                Some(response)
            }
            _ => None,
        })
        .collect::<Vec<_>>();

    assert_eq!(
        trades_responses.len(),
        1,
        "an empty Data API result must emit exactly one correlated trades response; events were: {events:?}",
    );
    assert_eq!(trades_responses[0].instrument_id, instrument_id);
    assert!(trades_responses[0].data.is_empty());
}

#[rstest]
#[tokio::test]
async fn test_stop_reconnect_recreates_market_message_receiver() {
    let state = TestServerState::default();
    *state.gamma_response.lock().await = Some(serde_json::json!([gamma_market_request_fixture()]));
    let addr = start_mock_server(state).await;
    let (mut client, _rx) = create_test_data_client(addr);

    client.connect().await.expect("connect #1");
    client.stop().expect("stop");
    client.connect().await.expect("connect #2");
    client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test]
async fn test_cached_resolution_subscription_replays_after_disconnect_reconnect() {
    let state = TestServerState::default();
    *state.gamma_response.lock().await = Some(serde_json::json!([gamma_market_request_fixture()]));
    let addr = start_mock_server(state.clone()).await;
    let (mut client, _rx) = create_test_data_client_with_resolution_options(addr, true, false);
    let instrument_id = yes_instrument_id();

    client.connect().await.expect("connect #1");
    client
        .subscribe_instrument_status(SubscribeInstrumentStatus::new(
            instrument_id,
            Some(*POLYMARKET_CLIENT_ID),
            None,
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ))
        .expect("subscribe instrument status");
    client
        .subscribe_instrument_close(SubscribeInstrumentClose::new(
            instrument_id,
            Some(*POLYMARKET_CLIENT_ID),
            None,
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ))
        .expect("subscribe instrument close");
    wait_for_market_payload_count(&state, 1, false, Duration::from_secs(5)).await;

    client.disconnect().await.expect("disconnect #1");
    client.connect().await.expect("connect #2");
    wait_for_market_payload_count(&state, 2, false, Duration::from_secs(5)).await;

    let asset_payloads = state
        .market_payloads
        .lock()
        .await
        .iter()
        .filter_map(|payload| {
            payload
                .get("assets_ids")
                .and_then(Value::as_array)
                .filter(|ids| !ids.is_empty())
                .cloned()
        })
        .collect::<Vec<_>>();
    let expected_assets = vec![Value::String(TEST_TOKEN_ID_YES.to_string())];

    client.disconnect().await.expect("disconnect #2");

    assert_eq!(
        asset_payloads,
        vec![expected_assets.clone(), expected_assets]
    );
}

#[rstest]
#[tokio::test]
async fn test_reset_reconnect_does_not_replay_stale_market_subscriptions() {
    let state = TestServerState::default();
    *state.gamma_response.lock().await = Some(serde_json::json!([gamma_market_request_fixture()]));
    let addr = start_mock_server(state.clone()).await;
    let (mut client, mut rx) = create_test_data_client_with_new_markets(addr, true);
    let instrument_id = yes_instrument_id();

    let prime_1_request_id = UUID4::new();
    let prime_1 = RequestInstrument::new(
        instrument_id,
        None,
        None,
        Some(*POLYMARKET_CLIENT_ID),
        prime_1_request_id,
        UnixNanos::default(),
        None,
    );
    client.request_instrument(prime_1).expect("prime cache #1");
    let _ = collect_data_events_until_response(&mut rx, prime_1_request_id, Duration::from_secs(5))
        .await;

    client.connect().await.expect("connect #1");

    let sub_1 = SubscribeQuotes::new(
        instrument_id,
        Some(*POLYMARKET_CLIENT_ID),
        None,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );
    client.subscribe_quotes(sub_1).expect("subscribe quotes #1");

    wait_for_market_payload_count(&state, 1, false, Duration::from_secs(5)).await;

    client.reset().expect("reset");
    client.connect().await.expect("connect #2");

    wait_for_market_payload_count(&state, 2, true, Duration::from_secs(5)).await;
    let replay_count = market_payload_count(&state, false).await;
    assert_eq!(
        replay_count, 1,
        "reset + reconnect must not replay stale market subscriptions, saw {replay_count} payload(s)",
    );

    let prime_2_request_id = UUID4::new();
    let prime_2 = RequestInstrument::new(
        instrument_id,
        None,
        None,
        Some(*POLYMARKET_CLIENT_ID),
        prime_2_request_id,
        UnixNanos::default(),
        None,
    );
    client.request_instrument(prime_2).expect("prime cache #2");
    let _ = collect_data_events_until_response(&mut rx, prime_2_request_id, Duration::from_secs(5))
        .await;

    let sub_2 = SubscribeQuotes::new(
        instrument_id,
        Some(*POLYMARKET_CLIENT_ID),
        None,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );
    client.subscribe_quotes(sub_2).expect("subscribe quotes #2");

    wait_for_market_payload_count(&state, 2, false, Duration::from_secs(5)).await;

    client.disconnect().await.expect("disconnect");
}
