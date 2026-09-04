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

//! Integration tests for `DeribitDataClient`.
//!
//! These tests verify the full data flow from WebSocket messages through
//! parsing to event emission via the data event channel.

use std::{
    net::SocketAddr,
    path::PathBuf,
    sync::{
        Arc, OnceLock,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};

use ahash::{AHashMap, AHashSet};
use axum::{
    Router,
    extract::{
        State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    response::{IntoResponse, Json, Response},
    routing::{get, post},
};
use log::{Level, LevelFilter, Log, Metadata, Record};
use nautilus_common::{
    clients::DataClient,
    live::runner::{replace_data_event_sender, replace_system_event_sender, set_data_event_sender},
    messages::{
        DataEvent, DataResponse, SystemEvent,
        data::{
            InstrumentResponse, RequestCustomData, RequestInstrument, SubscribeBars,
            SubscribeBookDeltas, SubscribeBookDepth10, SubscribeFundingRates, SubscribeIndexPrices,
            SubscribeMarkPrices, SubscribeOptionGreeks, SubscribeQuotes, SubscribeTrades,
            UnsubscribeTrades,
        },
        system::SocketState,
    },
    testing::wait_until_async,
};
use nautilus_core::{Params, UUID4, UnixNanos};
use nautilus_deribit::{
    common::{
        consts::{DERIBIT_CLIENT_ID, DERIBIT_VENUE},
        enums::DeribitEnvironment,
    },
    config::DeribitDataClientConfig,
    data::DeribitDataClient,
    data_types::DeribitBookSummary,
    http::models::DeribitProductType,
};
use nautilus_live::{SocketReconnectRegistry, SocketReconnectRequestOutcome};
use nautilus_model::{
    data::{BarType, CustomData, Data, DataType, TradeTick},
    enums::BookType,
    identifiers::InstrumentId,
};
use nautilus_network::http::HttpClient;
use nautilus_testkit::events::drain_data_events;
use parking_lot::Mutex;
use rstest::rstest;
use rust_decimal_macros::dec;
use serde_json::{Value, json};
use ustr::Ustr;

const TEST_TIMEOUT: Duration = Duration::from_secs(15);

fn data_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test_data")
}

fn load_json(filename: &str) -> Value {
    let content = std::fs::read_to_string(data_path().join(filename))
        .unwrap_or_else(|_| panic!("failed to read {filename}"));
    serde_json::from_str(&content).expect("invalid json")
}

fn sorted_strings(values: &[&str]) -> Vec<String> {
    let mut values = values
        .iter()
        .map(|value| (*value).to_string())
        .collect::<Vec<_>>();
    values.sort();
    values
}

fn trade_symbol_from_channel(channel: &str) -> Option<&str> {
    channel
        .strip_prefix("trades.")
        .and_then(|suffix| suffix.rsplit_once('.'))
        .map(|(symbol, _)| symbol)
}

async fn subscription_event_trade_symbols(
    state: &TestServerState,
    is_subscribe: bool,
) -> Vec<String> {
    let mut symbols = state
        .subscription_events
        .lock()
        .await
        .iter()
        .filter(|(_, event_is_subscribe)| *event_is_subscribe == is_subscribe)
        .filter_map(|(channel, _)| trade_symbol_from_channel(channel).map(str::to_string))
        .collect::<Vec<_>>();
    symbols.sort();
    symbols
}

async fn collect_trade_ticks(
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<DataEvent>,
    expected_count: usize,
) -> Vec<TradeTick> {
    let mut trades = Vec::new();

    while trades.len() < expected_count {
        let event = tokio::time::timeout(TEST_TIMEOUT, rx.recv())
            .await
            .expect("timeout waiting for trade")
            .expect("channel closed");

        if let DataEvent::Data(Data::Trade(trade)) = event {
            trades.push(trade);
        }
    }

    trades
}

fn instrument_response(events: &[DataEvent]) -> Option<&InstrumentResponse> {
    events.iter().find_map(|event| match event {
        DataEvent::Response(DataResponse::Instrument(response)) => Some(response.as_ref()),
        _ => None,
    })
}

fn trade_payload_for_channel(base_payload: &Value, channel: &str, has_combo_parent: bool) -> Value {
    let symbol = trade_symbol_from_channel(channel)
        .expect("trade channel must use trades.{instrument}.{interval} format");
    let mut payload = base_payload.clone();
    let mut trade = payload["params"]["data"][0].clone();

    payload["params"]["channel"] = json!(channel);
    trade["instrument_name"] = json!(symbol);

    let trade_id = match symbol {
        "BTC-COMBO-1" => "900001",
        "BTC-PERPETUAL" => "900002",
        "BTC-27DEC24" => "900003",
        _ => "900099",
    };
    trade["trade_id"] = json!(trade_id);

    let trade_obj = trade
        .as_object_mut()
        .expect("trade payload must be a JSON object");

    if has_combo_parent && matches!(symbol, "BTC-PERPETUAL" | "BTC-27DEC24") {
        trade_obj.insert("combo_id".to_string(), json!("BTC-COMBO-1"));
        trade_obj.insert("combo_trade_id".to_string(), json!("900001"));
    } else {
        trade_obj.remove("combo_id");
        trade_obj.remove("combo_trade_id");
    }

    payload["params"]["data"] = json!([trade]);
    payload
}

#[derive(Clone, Default)]
struct TestServerState {
    connection_count: Arc<tokio::sync::Mutex<usize>>,
    subscriptions: Arc<tokio::sync::Mutex<Vec<String>>>,
    subscription_events: Arc<tokio::sync::Mutex<Vec<(String, bool)>>>,
    auth_request_count: Arc<AtomicUsize>,
    disconnect_trigger: Arc<AtomicBool>,
    // When true, public/get_instrument responds with a JSON-RPC error,
    // exercising the lazy-load HTTP-failure path.
    fail_get_instrument: Arc<AtomicBool>,
    get_instrument_request_count: Arc<AtomicUsize>,
}

#[derive(Default)]
struct CapturingDebugLogger {
    messages: Mutex<Vec<String>>,
}

impl CapturingDebugLogger {
    fn clear(&self) {
        self.messages.lock().clear();
    }

    fn contains(&self, needle: &str) -> bool {
        self.messages
            .lock()
            .iter()
            .any(|message| message.contains(needle))
    }
}

impl Log for CapturingDebugLogger {
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        metadata.level() <= Level::Debug
    }

    fn log(&self, record: &Record<'_>) {
        if self.enabled(record.metadata()) {
            self.messages.lock().push(record.args().to_string());
        }
    }

    fn flush(&self) {}
}

static CAPTURING_DEBUG_LOGGER: OnceLock<CapturingDebugLogger> = OnceLock::new();
static LOG_CAPTURE_LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();

async fn lock_log_capture() -> tokio::sync::MutexGuard<'static, ()> {
    LOG_CAPTURE_LOCK
        .get_or_init(|| tokio::sync::Mutex::new(()))
        .lock()
        .await
}

fn install_capturing_debug_logger() -> &'static CapturingDebugLogger {
    let logger = CAPTURING_DEBUG_LOGGER.get_or_init(CapturingDebugLogger::default);
    let _ = log::set_logger(logger);
    log::set_max_level(LevelFilter::Debug);
    logger.clear();
    logger
}

async fn handle_jsonrpc_request(
    State(state): State<TestServerState>,
    Json(request): Json<Value>,
) -> impl IntoResponse {
    let method = request.get("method").and_then(|m| m.as_str()).unwrap_or("");
    let id = request.get("id").and_then(|i| i.as_u64()).unwrap_or(0);
    let params = request.get("params").cloned();

    match method {
        "public/get_instruments" => handle_get_instruments(id, params).await,
        "public/get_combos" => handle_get_combos(id).await,
        "public/get_book_summary_by_currency" => {
            let mut data = load_json("http_get_book_summary_by_currency.json");
            data["id"] = json!(id);
            Json(data).into_response()
        }
        "public/get_instrument" => {
            state
                .get_instrument_request_count
                .fetch_add(1, Ordering::Relaxed);

            if state.fail_get_instrument.load(Ordering::Relaxed) {
                return Json(json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "error": {
                        "code": 13020,
                        "message": "Instrument is not available"
                    },
                    "testnet": true
                }))
                .into_response();
            }

            let instrument_name = params
                .as_ref()
                .and_then(|p| p.get("instrument_name"))
                .and_then(|n| n.as_str())
                .unwrap_or("");

            // Route by requested instrument so lazy-load tests get the matching
            // payload rather than always receiving the BTC-PERPETUAL fixture
            if instrument_name == "BTC-COMBO-1" {
                return handle_get_combo_instrument(id).await;
            }

            let fixture =
                if instrument_name.contains('-') && instrument_name.matches('-').count() >= 3 {
                    "http_get_instrument_option.json"
                } else {
                    "http_get_instrument.json"
                };
            let mut data = load_json(fixture);
            data["id"] = json!(id);
            Json(data).into_response()
        }
        _ => Json(json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": {
                "code": -32601,
                "message": "Method not found"
            },
            "testnet": true
        }))
        .into_response(),
    }
}

async fn handle_get_combo_instrument(id: u64) -> Response {
    Json(json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": {
            "kind": "future_combo",
            "instrument_name": "BTC-COMBO-1",
            "max_leverage": 1,
            "maker_commission": 0.0,
            "taker_commission": 0.0,
            "instrument_type": "reversed",
            "creation_timestamp": 1719561600000_i64,
            "is_active": true,
            "tick_size": 0.01,
            "contract_size": 1.0,
            "instrument_id": 456789,
            "min_trade_amount": 1.0,
            "settlement_currency": "BTC",
            "base_currency": "BTC",
            "counter_currency": "USD",
            "quote_currency": "USD",
            "expiration_timestamp": 1767225600000_i64,
            "tick_size_steps": []
        },
        "usIn": 1765308000000000_u64,
        "usOut": 1765308000000500_u64,
        "usDiff": 500,
        "testnet": true
    }))
    .into_response()
}

async fn handle_get_combos(id: u64) -> Response {
    Json(json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": [
            {
                "id": "BTC-COMBO-1",
                "state": "active",
                "legs": [
                    {
                        "amount": -1,
                        "instrument_name": "BTC-PERPETUAL"
                    },
                    {
                        "amount": 1,
                        "instrument_name": "BTC-27DEC24"
                    }
                ],
                "creation_timestamp": 1719561600000_i64,
                "instrument_id": 456789,
                "state_timestamp": 1719561600000_i64
            }
        ],
        "usIn": 1765308000000000_u64,
        "usOut": 1765308000000500_u64,
        "usDiff": 500,
        "testnet": true
    }))
    .into_response()
}

async fn handle_get_instruments(id: u64, params: Option<Value>) -> Response {
    let currency = params
        .as_ref()
        .and_then(|p| p.get("currency"))
        .and_then(|c| c.as_str());

    match currency {
        Some("any" | "BTC") | None => {
            let mut data = load_json("http_get_instruments.json");
            data["id"] = json!(id);

            if let Some(result) = data.get_mut("result")
                && let Some(instruments) = result.as_array_mut()
            {
                for inst in instruments {
                    if inst.get("kind").and_then(|k| k.as_str()) == Some("future_combo")
                        && inst.get("expiration_timestamp").is_none()
                    {
                        inst["expiration_timestamp"] = json!(1_767_225_600_000_i64);
                    }
                }
            }

            if let Some(kind) = params
                .as_ref()
                .and_then(|p| p.get("kind"))
                .and_then(|k| k.as_str())
                && let Some(result) = data.get_mut("result")
                && let Some(instruments) = result.as_array_mut()
            {
                instruments.retain(|inst| {
                    inst.get("kind")
                        .and_then(|k| k.as_str())
                        .is_some_and(|k| k == kind)
                });
            }

            Json(data).into_response()
        }
        _ => Json(json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": [],
            "testnet": true
        }))
        .into_response(),
    }
}

async fn handle_ws_upgrade(ws: WebSocketUpgrade, State(state): State<TestServerState>) -> Response {
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

async fn handle_socket(mut socket: WebSocket, state: TestServerState) {
    {
        let mut count = state.connection_count.lock().await;
        *count += 1;
    }

    let trades_payload = load_json("ws_trades.json");
    let book_snapshot_payload = load_json("ws_book_snapshot.json");
    let quote_payload = load_json("ws_quote.json");
    let ticker_payload = load_json("ws_ticker.json");

    while let Some(message) = socket.recv().await {
        let Ok(message) = message else { break };

        if state.disconnect_trigger.load(Ordering::Relaxed) {
            let _ = socket.send(Message::Close(None)).await;
            break;
        }

        match message {
            Message::Text(text) => {
                let Ok(payload) = serde_json::from_str::<Value>(&text) else {
                    continue;
                };

                let method = payload.get("method").and_then(|m| m.as_str());
                let id = payload.get("id").and_then(|i| i.as_u64());

                match method {
                    Some("public/subscribe" | "private/subscribe") => {
                        if let Some(params) = payload.get("params")
                            && let Some(channels) =
                                params.get("channels").and_then(|c| c.as_array())
                        {
                            let mut subscribed_channels = Vec::new();

                            for channel in channels {
                                if let Some(channel_str) = channel.as_str() {
                                    state
                                        .subscription_events
                                        .lock()
                                        .await
                                        .push((channel_str.to_string(), true));
                                    state
                                        .subscriptions
                                        .lock()
                                        .await
                                        .push(channel_str.to_string());
                                    subscribed_channels.push(channel_str.to_string());
                                }
                            }

                            let response = json!({
                                "jsonrpc": "2.0",
                                "id": id,
                                "result": subscribed_channels,
                                "testnet": true,
                                "usIn": 1699999999000000_u64,
                                "usOut": 1699999999001000_u64,
                                "usDiff": 1000
                            });

                            if socket
                                .send(Message::Text(response.to_string().into()))
                                .await
                                .is_err()
                            {
                                break;
                            }

                            for channel in &subscribed_channels {
                                let payload_owned: Option<Value> = if channel.starts_with("trades.")
                                {
                                    let has_combo_parent =
                                        state.subscriptions.lock().await.iter().any(|channel| {
                                            trade_symbol_from_channel(channel)
                                                == Some("BTC-COMBO-1")
                                        });
                                    Some(trade_payload_for_channel(
                                        &trades_payload,
                                        channel,
                                        has_combo_parent,
                                    ))
                                } else if channel.starts_with("book.") {
                                    Some(book_snapshot_payload.clone())
                                } else if let Some(symbol) = channel.strip_prefix("quote.") {
                                    let mut p = quote_payload.clone();
                                    p["params"]["channel"] = json!(channel);
                                    p["params"]["data"]["instrument_name"] = json!(symbol);
                                    Some(p)
                                } else if channel.starts_with("ticker.") {
                                    Some(ticker_payload.clone())
                                } else {
                                    None
                                };

                                if let Some(payload) = payload_owned
                                    && socket
                                        .send(Message::Text(payload.to_string().into()))
                                        .await
                                        .is_err()
                                {
                                    break;
                                }
                            }
                        }
                    }
                    Some("public/unsubscribe" | "private/unsubscribe") => {
                        if let Some(params) = payload.get("params")
                            && let Some(channels) =
                                params.get("channels").and_then(|c| c.as_array())
                        {
                            let mut unsubscribed = Vec::new();

                            for channel in channels {
                                if let Some(channel_str) = channel.as_str() {
                                    state
                                        .subscription_events
                                        .lock()
                                        .await
                                        .push((channel_str.to_string(), false));
                                    unsubscribed.push(channel_str.to_string());
                                }
                            }

                            let response = json!({
                                "jsonrpc": "2.0",
                                "id": id,
                                "result": unsubscribed,
                                "testnet": true
                            });

                            if socket
                                .send(Message::Text(response.to_string().into()))
                                .await
                                .is_err()
                            {
                                break;
                            }
                        }
                    }
                    Some("public/set_heartbeat") => {
                        let response = json!({
                            "jsonrpc": "2.0",
                            "id": id,
                            "result": "ok",
                            "testnet": true
                        });

                        if socket
                            .send(Message::Text(response.to_string().into()))
                            .await
                            .is_err()
                        {
                            break;
                        }
                    }
                    Some("public/auth") => {
                        state.auth_request_count.fetch_add(1, Ordering::Relaxed);

                        let scope = payload
                            .get("params")
                            .and_then(|p| p.get("scope"))
                            .and_then(|s| s.as_str())
                            .unwrap_or("connection")
                            .to_string();

                        let auth_response = json!({
                            "jsonrpc": "2.0",
                            "id": id,
                            "result": {
                                "access_token": "mock_access_token_12345",
                                "refresh_token": "mock_refresh_token_67890",
                                "expires_in": 900,
                                "scope": scope,
                                "token_type": "bearer",
                                "enabled_features": []
                            },
                            "testnet": true,
                            "usIn": 1699999999000000_u64,
                            "usOut": 1699999999001000_u64,
                            "usDiff": 1000
                        });

                        if socket
                            .send(Message::Text(auth_response.to_string().into()))
                            .await
                            .is_err()
                        {
                            break;
                        }
                    }
                    Some("public/test") => {
                        let response = json!({
                            "jsonrpc": "2.0",
                            "id": id,
                            "result": {
                                "version": "1.2.26"
                            },
                            "testnet": true
                        });

                        if socket
                            .send(Message::Text(response.to_string().into()))
                            .await
                            .is_err()
                        {
                            break;
                        }
                    }
                    _ => {}
                }
            }
            Message::Ping(_) if socket.send(Message::Pong(vec![].into())).await.is_err() => {
                break;
            }
            Message::Close(_) => break,
            _ => {}
        }
    }

    let mut count = state.connection_count.lock().await;
    *count = count.saturating_sub(1);
}

fn create_test_router(state: TestServerState) -> Router {
    Router::new()
        .route("/api/v2", post(handle_jsonrpc_request))
        .route("/ws/api/v2", get(handle_ws_upgrade))
        .route("/health", get(|| async { "OK" }))
        .with_state(state)
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

    let health_url = format!("http://{addr}/health");
    let http_client = HttpClient::builder().build().unwrap();
    wait_until_async(
        || {
            let url = health_url.clone();
            let client = http_client.clone();
            async move { client.get(url, None, None, Some(1), None).await.is_ok() }
        },
        TEST_TIMEOUT,
    )
    .await;

    Ok((addr, state))
}

fn create_test_config(addr: SocketAddr) -> DeribitDataClientConfig {
    DeribitDataClientConfig {
        api_key: None,
        api_secret: None,
        product_types: vec![DeribitProductType::Future],
        base_url_http: Some(format!("http://{addr}/api/v2")),
        base_url_ws: Some(format!("ws://{addr}/ws/api/v2")),
        environment: DeribitEnvironment::Testnet,
        http_timeout_secs: 10,
        max_retries: 1,
        retry_delay_initial_ms: 100,
        retry_delay_max_ms: 1000,
        heartbeat_interval_secs: 30,
        update_instruments_interval_mins: 60,
        auto_load_missing_instruments: false,
        proxy_url: None,
        transport_backend: Default::default(),
        ..Default::default()
    }
}

#[rstest]
#[tokio::test]
async fn test_data_client_request_instrument_refetches_when_cached() {
    let (addr, state) = start_test_server().await.unwrap();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    replace_data_event_sender(tx);

    let config = create_test_config(addr);
    let client = DeribitDataClient::new(*DERIBIT_CLIENT_ID, config).unwrap();
    let instrument_id = InstrumentId::from("BTC-PERPETUAL.DERIBIT");

    let first_request_id = UUID4::new();
    client
        .request_instrument(RequestInstrument::new(
            instrument_id,
            None,
            None,
            Some(*DERIBIT_CLIENT_ID),
            first_request_id,
            UnixNanos::default(),
            None,
        ))
        .expect("first request_instrument");

    wait_until_async(
        || async {
            state.get_instrument_request_count.load(Ordering::Relaxed) >= 1 && !rx.is_empty()
        },
        TEST_TIMEOUT,
    )
    .await;
    let events = drain_data_events(&mut rx, Duration::from_millis(200)).await;
    let response = instrument_response(&events).expect("instrument response");
    assert_eq!(response.correlation_id, first_request_id);
    assert_eq!(response.client_id, *DERIBIT_CLIENT_ID);
    assert_eq!(response.instrument_id, instrument_id);

    state.fail_get_instrument.store(true, Ordering::Relaxed);

    client
        .request_instrument(RequestInstrument::new(
            instrument_id,
            None,
            None,
            Some(*DERIBIT_CLIENT_ID),
            UUID4::new(),
            UnixNanos::default(),
            None,
        ))
        .expect("second request_instrument");

    wait_until_async(
        || async { state.get_instrument_request_count.load(Ordering::Relaxed) >= 2 },
        TEST_TIMEOUT,
    )
    .await;

    let events = drain_data_events(&mut rx, Duration::from_millis(300)).await;
    assert!(
        instrument_response(&events).is_none(),
        "request_instrument must not emit a stale cached response when Deribit returns an error; events were: {events:?}",
    );
}

#[rstest]
#[tokio::test]
async fn test_data_client_connect_disconnect() {
    let (addr, state) = start_test_server().await.unwrap();
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    set_data_event_sender(tx);
    let (system_tx, mut system_rx) = tokio::sync::mpsc::unbounded_channel();
    replace_system_event_sender(system_tx);

    let config = create_test_config(addr);
    let registry = SocketReconnectRegistry::default();
    let mut client = registry
        .scope(|| DeribitDataClient::new(*DERIBIT_CLIENT_ID, config))
        .unwrap();
    assert!(!client.is_connected());

    client.connect().await.unwrap();
    assert!(client.is_connected());

    wait_until_async(
        || async { *state.connection_count.lock().await > 0 },
        TEST_TIMEOUT,
    )
    .await;
    let event = tokio::time::timeout(TEST_TIMEOUT, system_rx.recv())
        .await
        .unwrap()
        .unwrap();
    let SystemEvent::SocketState(change) = event;
    let endpoint = Ustr::from("deribit-data-streams");
    let handle = registry.handle(*DERIBIT_CLIENT_ID, endpoint).unwrap();

    assert_eq!(*state.connection_count.lock().await, 1);
    assert_eq!(change.client_id, *DERIBIT_CLIENT_ID);
    assert_eq!(change.venue, Some(*DERIBIT_VENUE));
    assert_eq!(change.endpoint, endpoint);
    assert_eq!(change.state, SocketState::Connected);
    assert_eq!(
        handle.request_reconnect(),
        SocketReconnectRequestOutcome::Accepted
    );
    let event = tokio::time::timeout(TEST_TIMEOUT, system_rx.recv())
        .await
        .unwrap()
        .unwrap();
    let SystemEvent::SocketState(change) = event;
    assert_eq!(change.endpoint, endpoint);
    assert_eq!(change.state, SocketState::Disconnected);

    client.disconnect().await.unwrap();
    assert!(!client.is_connected());
    assert!(registry.handle(*DERIBIT_CLIENT_ID, endpoint).is_none());
}

#[rstest]
#[tokio::test]
async fn test_data_client_subscribe_trades() {
    let (addr, state) = start_test_server().await.unwrap();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    set_data_event_sender(tx);

    let config = create_test_config(addr);
    let mut client = DeribitDataClient::new(*DERIBIT_CLIENT_ID, config).unwrap();
    client.connect().await.unwrap();

    wait_until_async(
        || async { *state.connection_count.lock().await > 0 },
        TEST_TIMEOUT,
    )
    .await;

    while rx.try_recv().is_ok() {}

    let instrument_id = InstrumentId::from("BTC-PERPETUAL.DERIBIT");
    let cmd = SubscribeTrades::new(
        instrument_id,
        Some(*DERIBIT_CLIENT_ID),
        None,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );
    client.subscribe_trades(cmd).unwrap();

    wait_until_async(
        || async { !state.subscription_events.lock().await.is_empty() },
        TEST_TIMEOUT,
    )
    .await;

    let event = tokio::time::timeout(TEST_TIMEOUT, rx.recv())
        .await
        .expect("timeout waiting for event")
        .expect("channel closed");

    assert!(
        matches!(event, DataEvent::Data(Data::Trade(_))),
        "Expected Trade event, was: {event:?}"
    );

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_data_client_subscribe_combo_legs_expands_trade_channels() {
    let (addr, state) = start_test_server().await.unwrap();
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    set_data_event_sender(tx);

    let mut config = create_test_config(addr);
    config.product_types = vec![DeribitProductType::Future, DeribitProductType::FutureCombo];
    let mut client = DeribitDataClient::new(*DERIBIT_CLIENT_ID, config).unwrap();
    client.connect().await.unwrap();

    wait_until_async(
        || async { *state.connection_count.lock().await > 0 },
        TEST_TIMEOUT,
    )
    .await;

    state.subscription_events.lock().await.clear();

    let instrument_id = InstrumentId::from("BTC-COMBO-1.DERIBIT");
    let mut params = Params::new();
    params.insert("subscribe_combo_legs".to_string(), json!(true));

    let subscribe = SubscribeTrades::new(
        instrument_id,
        Some(*DERIBIT_CLIENT_ID),
        None,
        UUID4::new(),
        UnixNanos::default(),
        None,
        Some(params.clone()),
    );
    client.subscribe_trades(subscribe).unwrap();

    wait_until_async(
        || async {
            subscription_event_trade_symbols(&state, true).await
                == sorted_strings(&["BTC-27DEC24", "BTC-COMBO-1", "BTC-PERPETUAL"])
        },
        TEST_TIMEOUT,
    )
    .await;

    let unsubscribe = UnsubscribeTrades::new(
        instrument_id,
        Some(*DERIBIT_CLIENT_ID),
        None,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );
    client.unsubscribe_trades(&unsubscribe).unwrap();

    wait_until_async(
        || async {
            subscription_event_trade_symbols(&state, false).await
                == sorted_strings(&["BTC-27DEC24", "BTC-COMBO-1", "BTC-PERPETUAL"])
        },
        TEST_TIMEOUT,
    )
    .await;

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_data_client_subscribe_combo_legs_delivers_parent_and_leg_trades() {
    let (addr, state) = start_test_server().await.unwrap();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    set_data_event_sender(tx);

    let mut config = create_test_config(addr);
    config.product_types = vec![DeribitProductType::Future, DeribitProductType::FutureCombo];
    let mut client = DeribitDataClient::new(*DERIBIT_CLIENT_ID, config).unwrap();
    client.connect().await.unwrap();

    wait_until_async(
        || async { *state.connection_count.lock().await > 0 },
        TEST_TIMEOUT,
    )
    .await;

    while rx.try_recv().is_ok() {}
    state.subscription_events.lock().await.clear();

    let instrument_id = InstrumentId::from("BTC-COMBO-1.DERIBIT");
    let mut params = Params::new();
    params.insert("subscribe_combo_legs".to_string(), json!(true));

    let subscribe = SubscribeTrades::new(
        instrument_id,
        Some(*DERIBIT_CLIENT_ID),
        None,
        UUID4::new(),
        UnixNanos::default(),
        None,
        Some(params),
    );
    client.subscribe_trades(subscribe).unwrap();

    wait_until_async(
        || async {
            subscription_event_trade_symbols(&state, true).await
                == sorted_strings(&["BTC-27DEC24", "BTC-COMBO-1", "BTC-PERPETUAL"])
        },
        TEST_TIMEOUT,
    )
    .await;

    let trades = collect_trade_ticks(&mut rx, 3).await;
    let instrument_ids = trades
        .iter()
        .map(|trade| trade.instrument_id)
        .collect::<AHashSet<_>>();
    let expected_instrument_ids = [
        InstrumentId::from("BTC-COMBO-1.DERIBIT"),
        InstrumentId::from("BTC-PERPETUAL.DERIBIT"),
        InstrumentId::from("BTC-27DEC24.DERIBIT"),
    ]
    .into_iter()
    .collect::<AHashSet<_>>();
    assert_eq!(instrument_ids, expected_instrument_ids,);

    let trades_by_instrument = trades
        .into_iter()
        .map(|trade| (trade.instrument_id, trade))
        .collect::<AHashMap<_, _>>();

    assert_eq!(
        trades_by_instrument
            .get(&InstrumentId::from("BTC-COMBO-1.DERIBIT"))
            .unwrap()
            .trade_id
            .to_string(),
        "900001"
    );
    assert_eq!(
        trades_by_instrument
            .get(&InstrumentId::from("BTC-PERPETUAL.DERIBIT"))
            .unwrap()
            .trade_id
            .to_string(),
        "COMBO-900002"
    );
    assert_eq!(
        trades_by_instrument
            .get(&InstrumentId::from("BTC-27DEC24.DERIBIT"))
            .unwrap()
            .trade_id
            .to_string(),
        "COMBO-900003"
    );

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_data_client_subscribe_combo_legs_repeated_unsubscribes_last_reference() {
    let _capture_guard = lock_log_capture().await;
    let logger = install_capturing_debug_logger();
    let (addr, state) = start_test_server().await.unwrap();
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    set_data_event_sender(tx);

    let mut config = create_test_config(addr);
    config.product_types = vec![DeribitProductType::Future, DeribitProductType::FutureCombo];
    let mut client = DeribitDataClient::new(*DERIBIT_CLIENT_ID, config).unwrap();
    client.connect().await.unwrap();

    wait_until_async(
        || async { *state.connection_count.lock().await > 0 },
        TEST_TIMEOUT,
    )
    .await;

    state.subscription_events.lock().await.clear();

    let instrument_id = InstrumentId::from("BTC-COMBO-1.DERIBIT");
    let mut params = Params::new();
    params.insert("subscribe_combo_legs".to_string(), json!(true));

    let first_command_id = UUID4::new();
    let first_subscribe = SubscribeTrades::new(
        instrument_id,
        Some(*DERIBIT_CLIENT_ID),
        None,
        first_command_id,
        UnixNanos::default(),
        None,
        Some(params.clone()),
    );
    logger.clear();
    client.subscribe_trades(first_subscribe).unwrap();

    wait_until_async(
        || async {
            subscription_event_trade_symbols(&state, true).await
                == sorted_strings(&["BTC-27DEC24", "BTC-COMBO-1", "BTC-PERPETUAL"])
        },
        TEST_TIMEOUT,
    )
    .await;
    wait_until_async(
        || async {
            logger.contains(&format!(
                "Processed trade subscription batch: command_id={first_command_id}, \
                 requests=3, instrument=BTC-COMBO-1.DERIBIT"
            ))
        },
        TEST_TIMEOUT,
    )
    .await;

    state.subscription_events.lock().await.clear();

    let second_command_id = UUID4::new();
    let second_subscribe = SubscribeTrades::new(
        instrument_id,
        Some(*DERIBIT_CLIENT_ID),
        None,
        second_command_id,
        UnixNanos::default(),
        None,
        Some(params),
    );
    logger.clear();
    client.subscribe_trades(second_subscribe).unwrap();

    wait_until_async(
        || async {
            logger.contains(&format!(
                "Processed trade subscription batch: command_id={second_command_id}, \
                 requests=3, instrument=BTC-COMBO-1.DERIBIT"
            ))
        },
        TEST_TIMEOUT,
    )
    .await;
    assert!(state.subscription_events.lock().await.is_empty());

    let unsubscribe_command_id = UUID4::new();
    let unsubscribe = UnsubscribeTrades::new(
        instrument_id,
        Some(*DERIBIT_CLIENT_ID),
        None,
        unsubscribe_command_id,
        UnixNanos::default(),
        None,
        None,
    );
    logger.clear();
    client.unsubscribe_trades(&unsubscribe).unwrap();

    wait_until_async(
        || async {
            logger.contains(&format!(
                "Processed trade unsubscription batch: command_id={unsubscribe_command_id}, \
                 requests=3, instrument=BTC-COMBO-1.DERIBIT"
            ))
        },
        TEST_TIMEOUT,
    )
    .await;
    assert!(state.subscription_events.lock().await.is_empty());

    client.unsubscribe_trades(&unsubscribe).unwrap();

    wait_until_async(
        || async {
            subscription_event_trade_symbols(&state, false).await
                == sorted_strings(&["BTC-27DEC24", "BTC-COMBO-1", "BTC-PERPETUAL"])
        },
        TEST_TIMEOUT,
    )
    .await;

    client.disconnect().await.unwrap();
}

#[rstest]
#[case(None)]
#[case(Some(false))]
#[tokio::test]
async fn test_data_client_subscribe_combo_legs_requires_opt_in(#[case] opt_in: Option<bool>) {
    let _capture_guard = lock_log_capture().await;
    let logger = install_capturing_debug_logger();
    let (addr, state) = start_test_server().await.unwrap();
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    set_data_event_sender(tx);

    let mut config = create_test_config(addr);
    config.product_types = vec![DeribitProductType::Future, DeribitProductType::FutureCombo];
    let mut client = DeribitDataClient::new(*DERIBIT_CLIENT_ID, config).unwrap();
    client.connect().await.unwrap();

    wait_until_async(
        || async { *state.connection_count.lock().await > 0 },
        TEST_TIMEOUT,
    )
    .await;

    state.subscription_events.lock().await.clear();

    let instrument_id = InstrumentId::from("BTC-COMBO-1.DERIBIT");
    let params = opt_in.map(|value| {
        let mut params = Params::new();
        params.insert("subscribe_combo_legs".to_string(), json!(value));
        params
    });

    let command_id = UUID4::new();
    let subscribe = SubscribeTrades::new(
        instrument_id,
        Some(*DERIBIT_CLIENT_ID),
        None,
        command_id,
        UnixNanos::default(),
        None,
        params,
    );
    logger.clear();
    client.subscribe_trades(subscribe).unwrap();

    wait_until_async(
        || async {
            let events = state.subscription_events.lock().await;
            events.iter().any(|(channel, is_subscribe)| {
                *is_subscribe && trade_symbol_from_channel(channel) == Some("BTC-COMBO-1")
            })
        },
        TEST_TIMEOUT,
    )
    .await;
    wait_until_async(
        || async {
            logger.contains(&format!(
                "Processed trade subscription batch: command_id={command_id}, \
                 requests=1, instrument=BTC-COMBO-1.DERIBIT"
            ))
        },
        TEST_TIMEOUT,
    )
    .await;
    let events = state.subscription_events.lock().await;
    let subscribed = events
        .iter()
        .filter(|(_, is_subscribe)| *is_subscribe)
        .filter_map(|(channel, _)| trade_symbol_from_channel(channel))
        .collect::<Vec<_>>();

    assert_eq!(subscribed, vec!["BTC-COMBO-1"]);

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_data_client_subscribe_combo_legs_after_lazy_loading_combo() {
    let (addr, state) = start_test_server().await.unwrap();
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    set_data_event_sender(tx);

    let mut config = create_test_config(addr);
    config.auto_load_missing_instruments = true;
    let mut client = DeribitDataClient::new(*DERIBIT_CLIENT_ID, config).unwrap();
    client.connect().await.unwrap();

    wait_until_async(
        || async { *state.connection_count.lock().await > 0 },
        TEST_TIMEOUT,
    )
    .await;

    state.subscription_events.lock().await.clear();

    let instrument_id = InstrumentId::from("BTC-COMBO-1.DERIBIT");
    let mut params = Params::new();
    params.insert("subscribe_combo_legs".to_string(), json!(true));

    let subscribe = SubscribeTrades::new(
        instrument_id,
        Some(*DERIBIT_CLIENT_ID),
        None,
        UUID4::new(),
        UnixNanos::default(),
        None,
        Some(params),
    );
    client.subscribe_trades(subscribe).unwrap();

    wait_until_async(
        || async {
            let subscribed = subscription_event_trade_symbols(&state, true).await;

            subscribed.contains(&"BTC-COMBO-1".to_string())
                && subscribed.contains(&"BTC-PERPETUAL".to_string())
                && subscribed.contains(&"BTC-27DEC24".to_string())
        },
        TEST_TIMEOUT,
    )
    .await;

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_data_client_subscribe_quotes() {
    let (addr, state) = start_test_server().await.unwrap();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    set_data_event_sender(tx);

    let config = create_test_config(addr);
    let mut client = DeribitDataClient::new(*DERIBIT_CLIENT_ID, config).unwrap();
    client.connect().await.unwrap();

    wait_until_async(
        || async { *state.connection_count.lock().await > 0 },
        TEST_TIMEOUT,
    )
    .await;

    while rx.try_recv().is_ok() {}

    let instrument_id = InstrumentId::from("BTC-PERPETUAL.DERIBIT");
    let cmd = SubscribeQuotes::new(
        instrument_id,
        Some(*DERIBIT_CLIENT_ID),
        None,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );
    client.subscribe_quotes(cmd).unwrap();

    wait_until_async(
        || async {
            state
                .subscription_events
                .lock()
                .await
                .iter()
                .any(|(topic, _)| topic.contains("quote."))
        },
        TEST_TIMEOUT,
    )
    .await;

    let event = tokio::time::timeout(TEST_TIMEOUT, rx.recv())
        .await
        .expect("timeout waiting for event")
        .expect("channel closed");

    assert!(
        matches!(event, DataEvent::Data(Data::Quote(_))),
        "Expected Quote event, was: {event:?}"
    );

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_data_client_subscribe_book_deltas() {
    let (addr, state) = start_test_server().await.unwrap();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    set_data_event_sender(tx);

    let config = create_test_config(addr);
    let mut client = DeribitDataClient::new(*DERIBIT_CLIENT_ID, config).unwrap();
    client.connect().await.unwrap();

    wait_until_async(
        || async { *state.connection_count.lock().await > 0 },
        TEST_TIMEOUT,
    )
    .await;

    while rx.try_recv().is_ok() {}

    let instrument_id = InstrumentId::from("BTC-PERPETUAL.DERIBIT");
    let cmd = SubscribeBookDeltas::new(
        instrument_id,
        BookType::L2_MBP,
        Some(*DERIBIT_CLIENT_ID),
        None,
        UUID4::new(),
        UnixNanos::default(),
        None,
        false,
        None,
        None,
    );
    client.subscribe_book_deltas(cmd).unwrap();

    wait_until_async(
        || async {
            state
                .subscription_events
                .lock()
                .await
                .iter()
                .any(|(topic, _)| topic.contains("book."))
        },
        TEST_TIMEOUT,
    )
    .await;

    let event = tokio::time::timeout(TEST_TIMEOUT, rx.recv())
        .await
        .expect("timeout waiting for event")
        .expect("channel closed");

    assert!(
        matches!(event, DataEvent::Data(Data::BookDeltas(_))),
        "Expected Deltas event, was: {event:?}"
    );

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_data_client_reset_clears_state() {
    let (addr, _state) = start_test_server().await.unwrap();
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    set_data_event_sender(tx);

    let config = create_test_config(addr);
    let mut client = DeribitDataClient::new(*DERIBIT_CLIENT_ID, config).unwrap();

    client.reset().unwrap();
    assert!(!client.is_connected());

    client.connect().await.unwrap();
    assert!(client.is_connected());

    client.reset().unwrap();
    assert!(!client.is_connected());
}

#[rstest]
#[tokio::test]
async fn test_subscribe_quotes_uncached_instrument_fails_fast() {
    // Bug #4035: subscribing to an instrument that has not been preloaded must not
    // silently succeed and then have its frames dropped at the WebSocket handler.
    // Default `auto_load_missing_instruments=false` means subscribe should error up front.
    let (addr, _state) = start_test_server().await.unwrap();
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    set_data_event_sender(tx);

    let config = create_test_config(addr); // product_types=[Future] -> option not preloaded
    let mut client = DeribitDataClient::new(*DERIBIT_CLIENT_ID, config).unwrap();
    client.connect().await.unwrap();

    let option_id = InstrumentId::from("BTC-27DEC24-100000-C.DERIBIT");
    let cmd = SubscribeQuotes::new(
        option_id,
        Some(*DERIBIT_CLIENT_ID),
        None,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );

    let err = client
        .subscribe_quotes(cmd)
        .expect_err("expected subscribe to error on uncached instrument");
    let msg = err.to_string();
    assert!(
        msg.contains("auto_load_missing_instruments"),
        "error should reference the config flag, was: {msg}"
    );

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_subscribe_quotes_uncached_instrument_lazy_loads() {
    // Bug #4035: when `auto_load_missing_instruments=true`, subscribe accepts an
    // uncached instrument, fetches it via HTTP, seeds the WebSocket handler cache,
    // and forwards the WS subscribe so subsequent quote frames are emitted as data.
    let (addr, state) = start_test_server().await.unwrap();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    set_data_event_sender(tx);

    let mut config = create_test_config(addr);
    config.auto_load_missing_instruments = true;
    let mut client = DeribitDataClient::new(*DERIBIT_CLIENT_ID, config).unwrap();
    client.connect().await.unwrap();

    wait_until_async(
        || async { *state.connection_count.lock().await > 0 },
        TEST_TIMEOUT,
    )
    .await;

    // Drain instrument-load events from connect()
    while rx.try_recv().is_ok() {}

    let option_id = InstrumentId::from("BTC-27DEC24-100000-C.DERIBIT");
    let cmd = SubscribeQuotes::new(
        option_id,
        Some(*DERIBIT_CLIENT_ID),
        None,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );

    client
        .subscribe_quotes(cmd)
        .expect("subscribe should accept uncached instrument when auto_load is enabled");

    let quote = tokio::time::timeout(TEST_TIMEOUT, async {
        loop {
            let event = rx.recv().await.expect("channel closed");

            if let DataEvent::Data(Data::Quote(quote)) = event
                && quote.instrument_id == option_id
            {
                break quote;
            }
        }
    })
    .await
    .expect("timeout waiting for lazy-loaded option quote");

    assert_eq!(quote.instrument_id, option_id);

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_subscribe_quotes_lazy_load_http_failure_skips_ws_subscribe() {
    let _capture_guard = lock_log_capture().await;
    let logger = install_capturing_debug_logger();
    // Bug #4035: when lazy-load fails (HTTP error), the WS subscribe must be
    // skipped. Otherwise Deribit would ack the subscribe and stream frames the
    // handler cannot match, reintroducing the silent-drop behavior.
    let (addr, state) = start_test_server().await.unwrap();
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    set_data_event_sender(tx);

    let mut config = create_test_config(addr);
    config.auto_load_missing_instruments = true;
    let mut client = DeribitDataClient::new(*DERIBIT_CLIENT_ID, config).unwrap();
    client.connect().await.unwrap();

    wait_until_async(
        || async { *state.connection_count.lock().await > 0 },
        TEST_TIMEOUT,
    )
    .await;

    // Force the next get_instrument to fail
    state.fail_get_instrument.store(true, Ordering::Relaxed);

    let option_id = InstrumentId::from("BTC-27DEC24-100000-C.DERIBIT");
    let command_id = UUID4::new();
    let cmd = SubscribeQuotes::new(
        option_id,
        Some(*DERIBIT_CLIENT_ID),
        None,
        command_id,
        UnixNanos::default(),
        None,
        None,
    );

    logger.clear();
    client
        .subscribe_quotes(cmd)
        .expect("subscribe returns Ok; the failure is logged on the spawned task");

    wait_until_async(
        || async {
            logger.contains(&format!(
                "Lazy-load failed for BTC-27DEC24-100000-C.DERIBIT \
                 (quotes, command_id={command_id}):"
            ))
        },
        TEST_TIMEOUT,
    )
    .await;

    let saw_quote_channel = state
        .subscription_events
        .lock()
        .await
        .iter()
        .any(|(topic, _)| topic.starts_with("quote."));

    assert_eq!(
        state.get_instrument_request_count.load(Ordering::Relaxed),
        1
    );
    assert!(
        !saw_quote_channel,
        "lazy-load HTTP failure must not forward the WebSocket subscribe"
    );

    client.disconnect().await.unwrap();
}

#[derive(Clone, Copy, Debug)]
enum SubscribeKind {
    Quotes,
    Trades,
    BookDeltas,
    BookDepth10,
    MarkPrices,
    IndexPrices,
    Bars,
    FundingRates,
    OptionGreeks,
}

fn dispatch_subscribe(
    client: &mut DeribitDataClient,
    kind: SubscribeKind,
    instrument_id: InstrumentId,
) -> anyhow::Result<()> {
    let client_id = Some(*DERIBIT_CLIENT_ID);
    let cmd_id = UUID4::new();
    let ts = UnixNanos::default();

    match kind {
        SubscribeKind::Quotes => client.subscribe_quotes(SubscribeQuotes::new(
            instrument_id,
            client_id,
            None,
            cmd_id,
            ts,
            None,
            None,
        )),
        SubscribeKind::Trades => client.subscribe_trades(SubscribeTrades::new(
            instrument_id,
            client_id,
            None,
            cmd_id,
            ts,
            None,
            None,
        )),
        SubscribeKind::BookDeltas => client.subscribe_book_deltas(SubscribeBookDeltas::new(
            instrument_id,
            BookType::L2_MBP,
            client_id,
            None,
            cmd_id,
            ts,
            None,
            false,
            None,
            None,
        )),
        SubscribeKind::BookDepth10 => client.subscribe_book_depth10(SubscribeBookDepth10::new(
            instrument_id,
            BookType::L2_MBP,
            client_id,
            None,
            cmd_id,
            ts,
            None,
            false,
            None,
            None,
        )),
        SubscribeKind::MarkPrices => client.subscribe_mark_prices(SubscribeMarkPrices::new(
            instrument_id,
            client_id,
            None,
            cmd_id,
            ts,
            None,
            None,
        )),
        SubscribeKind::IndexPrices => client.subscribe_index_prices(SubscribeIndexPrices::new(
            instrument_id,
            client_id,
            None,
            cmd_id,
            ts,
            None,
            None,
        )),
        SubscribeKind::Bars => {
            let bar_type =
                BarType::from(format!("{instrument_id}-1-MINUTE-LAST-EXTERNAL").as_str());
            client.subscribe_bars(SubscribeBars::new(
                bar_type, client_id, None, cmd_id, ts, None, None,
            ))
        }
        SubscribeKind::FundingRates => client.subscribe_funding_rates(SubscribeFundingRates::new(
            instrument_id,
            client_id,
            None,
            cmd_id,
            ts,
            None,
            None,
        )),
        SubscribeKind::OptionGreeks => client.subscribe_option_greeks(SubscribeOptionGreeks::new(
            instrument_id,
            client_id,
            None,
            cmd_id,
            ts,
            None,
            None,
        )),
    }
}

#[rstest]
#[case::quotes(SubscribeKind::Quotes)]
#[case::trades(SubscribeKind::Trades)]
#[case::book_deltas(SubscribeKind::BookDeltas)]
#[case::book_depth10(SubscribeKind::BookDepth10)]
#[case::mark_prices(SubscribeKind::MarkPrices)]
#[case::index_prices(SubscribeKind::IndexPrices)]
#[case::bars(SubscribeKind::Bars)]
#[case::funding_rates(SubscribeKind::FundingRates)]
#[case::option_greeks(SubscribeKind::OptionGreeks)]
#[tokio::test]
async fn test_subscribe_uncached_instrument_fails_fast(#[case] kind: SubscribeKind) {
    // Bug #4035: every subscribe entry-point shares prepare_subscribe and must
    // fail fast on uncached instruments when auto_load_missing_instruments is off.
    let (addr, _state) = start_test_server().await.unwrap();
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    set_data_event_sender(tx);

    let config = create_test_config(addr); // auto_load=false, product_types=[Future]
    let mut client = DeribitDataClient::new(*DERIBIT_CLIENT_ID, config).unwrap();
    client.connect().await.unwrap();

    let option_id = InstrumentId::from("BTC-27DEC24-100000-C.DERIBIT");
    let err = dispatch_subscribe(&mut client, kind, option_id)
        .expect_err("subscribe must error on uncached instrument");
    let msg = err.to_string();
    assert!(
        msg.contains("auto_load_missing_instruments"),
        "{kind:?} error should reference the config flag, was: {msg}"
    );

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_subscribe_funding_rates_rejects_non_perpetual() {
    let _capture_guard = lock_log_capture().await;
    let logger = install_capturing_debug_logger();
    // Funding rates are perpetual-only; subscribing for a future must log a
    // warning and skip the WS subscribe rather than emit a perpetual.* channel.
    let (addr, state) = start_test_server().await.unwrap();
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    set_data_event_sender(tx);

    let config = create_test_config(addr); // product_types=[Future] preloads BTC-27DEC24
    let mut client = DeribitDataClient::new(*DERIBIT_CLIENT_ID, config).unwrap();
    client.connect().await.unwrap();

    wait_until_async(
        || async { *state.connection_count.lock().await > 0 },
        TEST_TIMEOUT,
    )
    .await;

    let future_id = InstrumentId::from("BTC-27DEC24.DERIBIT");
    let command_id = UUID4::new();
    let cmd = SubscribeFundingRates::new(
        future_id,
        Some(*DERIBIT_CLIENT_ID),
        None,
        command_id,
        UnixNanos::default(),
        None,
        None,
    );

    logger.clear();
    client
        .subscribe_funding_rates(cmd)
        .expect("subscribe returns Ok; rejection is async + logged");

    wait_until_async(
        || async {
            logger.contains(&format!(
                "Funding rates subscription rejected for BTC-27DEC24.DERIBIT \
                 (command_id={command_id}): only available for perpetual instruments"
            ))
        },
        TEST_TIMEOUT,
    )
    .await;

    let saw_perpetual_channel = state
        .subscription_events
        .lock()
        .await
        .iter()
        .any(|(topic, _)| topic.starts_with("perpetual."));
    assert!(
        !saw_perpetual_channel,
        "funding rates subscribe for a non-perpetual must not forward a perpetual.* subscribe"
    );

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_data_client_emits_instruments_on_connect() {
    let (addr, _state) = start_test_server().await.unwrap();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    set_data_event_sender(tx);

    let config = create_test_config(addr);
    let mut client = DeribitDataClient::new(*DERIBIT_CLIENT_ID, config).unwrap();

    client.connect().await.unwrap();

    let instruments_received = Arc::new(AtomicUsize::new(0));
    let counter = instruments_received.clone();

    wait_until_async(
        || {
            while let Ok(event) = rx.try_recv() {
                if matches!(event, DataEvent::Instrument(_)) {
                    counter.fetch_add(1, Ordering::Relaxed);
                }
            }
            let count = counter.load(Ordering::Relaxed);
            async move { count > 0 }
        },
        TEST_TIMEOUT,
    )
    .await;

    assert!(
        instruments_received.load(Ordering::Relaxed) > 0,
        "Expected to receive instrument events on connect"
    );

    client.disconnect().await.unwrap();
}

fn book_summary_data_type(currency: &str, kind: &str) -> DataType {
    let mut metadata = Params::new();
    metadata.insert(
        "currency".to_string(),
        serde_json::Value::String(currency.to_string()),
    );
    metadata.insert(
        "kind".to_string(),
        serde_json::Value::String(kind.to_string()),
    );
    DataType::new(
        "DeribitBookSummary",
        Some(metadata),
        None, // selector-only; adapter must canonicalize identifier
    )
}

#[rstest]
#[tokio::test]
async fn test_data_client_request_book_summaries_round_trip() {
    let (addr, _state) = start_test_server().await.unwrap();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    replace_data_event_sender(tx);

    let config = create_test_config(addr);
    let mut client = DeribitDataClient::new(*DERIBIT_CLIENT_ID, config).unwrap();
    client.connect().await.unwrap();

    while rx.try_recv().is_ok() {}

    let request_data_type = book_summary_data_type("btc", "option");
    assert_eq!(request_data_type.identifier(), None);

    client
        .request_data(RequestCustomData::new(
            *DERIBIT_CLIENT_ID,
            request_data_type.clone(),
            None,
            None,
            None,
            UUID4::new(),
            UnixNanos::default(),
            None,
        ))
        .unwrap();

    let response = loop {
        let event = tokio::time::timeout(TEST_TIMEOUT, rx.recv())
            .await
            .expect("timeout waiting for book summary response")
            .expect("channel closed");

        if let DataEvent::Response(DataResponse::Data(response)) = event {
            break response;
        }
    };

    let items = response
        .data
        .as_ref()
        .downcast_ref::<Vec<CustomData>>()
        .expect("expected Vec<CustomData> payload");

    assert_eq!(items.len(), 2);
    assert_eq!(response.data_type.type_name(), "DeribitBookSummary");
    assert_eq!(
        response.data_type.identifier(),
        Some("BTC:option"),
        "adapter must canonicalize response identity"
    );
    assert_eq!(
        response
            .data_type
            .metadata()
            .and_then(|m| m.get("currency"))
            .and_then(|v| v.as_str()),
        Some("BTC"),
    );
    assert_eq!(
        response
            .data_type
            .metadata()
            .and_then(|m| m.get("kind"))
            .and_then(|v| v.as_str()),
        Some("option"),
    );

    for custom in items {
        assert_eq!(custom.data_type.type_name(), "DeribitBookSummary");
        assert_eq!(custom.data_type.identifier(), Some("BTC:option"));
    }

    let first = items[0]
        .data
        .as_any()
        .downcast_ref::<DeribitBookSummary>()
        .expect("expected DeribitBookSummary");
    assert_eq!(
        first.instrument_id,
        InstrumentId::from("BTC-28MAR25-90000-C.DERIBIT")
    );
    assert_eq!(first.instrument_name, "BTC-28MAR25-90000-C");
    assert_eq!(first.mark_price, Some(dec!(0.042)));
    assert_eq!(first.mark_iv, Some(dec!(55.2)));
    assert_eq!(first.bid_price, Some(dec!(0.040)));
    assert_eq!(first.ask_price, Some(dec!(0.042)));
    assert_eq!(first.open_interest, Some(dec!(123.5)));
    assert_eq!(first.last_price, Some(dec!(0.0415)));
    assert_eq!(first.underlying_price, Some(dec!(95000.5)));
    assert_ne!(first.ts_event, UnixNanos::default());
    assert_eq!(first.ts_event, first.ts_init);

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_data_client_request_book_summaries_rejects_missing_currency() {
    let (addr, _state) = start_test_server().await.unwrap();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    replace_data_event_sender(tx);

    let config = create_test_config(addr);
    let mut client = DeribitDataClient::new(*DERIBIT_CLIENT_ID, config).unwrap();
    client.connect().await.unwrap();

    while rx.try_recv().is_ok() {}

    let data_type = DataType::new("DeribitBookSummary", None, None);
    let result = client.request_data(RequestCustomData::new(
        *DERIBIT_CLIENT_ID,
        data_type,
        None,
        None,
        None,
        UUID4::new(),
        UnixNanos::default(),
        None,
    ));
    assert!(result.is_err(), "missing currency must reject");

    // No response should be emitted for the rejected request.
    assert!(
        tokio::time::timeout(Duration::from_millis(200), rx.recv())
            .await
            .is_err()
    );

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_data_client_request_book_summaries_defaults_kind_and_uppercases_currency() {
    let (addr, _state) = start_test_server().await.unwrap();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    replace_data_event_sender(tx);

    let config = create_test_config(addr);
    let mut client = DeribitDataClient::new(*DERIBIT_CLIENT_ID, config).unwrap();
    client.connect().await.unwrap();

    while rx.try_recv().is_ok() {}

    // Selector-only: lowercase currency, omit kind, no catalog identifier.
    let mut metadata = Params::new();
    metadata.insert(
        "currency".to_string(),
        serde_json::Value::String("eth".to_string()),
    );
    let request_data_type = DataType::new("DeribitBookSummary", Some(metadata), None);

    client
        .request_data(RequestCustomData::new(
            *DERIBIT_CLIENT_ID,
            request_data_type,
            None,
            None,
            None,
            UUID4::new(),
            UnixNanos::default(),
            None,
        ))
        .unwrap();

    let response = loop {
        let event = tokio::time::timeout(TEST_TIMEOUT, rx.recv())
            .await
            .expect("timeout waiting for book summary response")
            .expect("channel closed");

        if let DataEvent::Response(DataResponse::Data(response)) = event {
            break response;
        }
    };

    assert_eq!(response.data_type.type_name(), "DeribitBookSummary");
    assert_eq!(response.data_type.identifier(), Some("ETH:option"));
    assert_eq!(
        response
            .data_type
            .metadata()
            .and_then(|m| m.get("currency"))
            .and_then(|v| v.as_str()),
        Some("ETH"),
    );
    assert_eq!(
        response
            .data_type
            .metadata()
            .and_then(|m| m.get("kind"))
            .and_then(|v| v.as_str()),
        Some("option"),
    );

    let items = response
        .data
        .as_ref()
        .downcast_ref::<Vec<CustomData>>()
        .expect("expected Vec<CustomData> payload");

    for custom in items {
        assert_eq!(custom.data_type.identifier(), Some("ETH:option"));
    }

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_data_client_request_data_ignores_unsupported_custom_type() {
    let (addr, _state) = start_test_server().await.unwrap();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    replace_data_event_sender(tx);

    let config = create_test_config(addr);
    let mut client = DeribitDataClient::new(*DERIBIT_CLIENT_ID, config).unwrap();
    client.connect().await.unwrap();

    while rx.try_recv().is_ok() {}

    let data_type = DataType::new("NotADeribitType", None, None);
    let result = client.request_data(RequestCustomData::new(
        *DERIBIT_CLIENT_ID,
        data_type,
        None,
        None,
        None,
        UUID4::new(),
        UnixNanos::default(),
        None,
    ));
    assert!(result.is_ok(), "unsupported type is soft-ignored");
    assert!(
        tokio::time::timeout(Duration::from_millis(200), rx.recv())
            .await
            .is_err()
    );

    client.disconnect().await.unwrap();
}
