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
    collections::HashMap,
    net::SocketAddr,
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
    response::{IntoResponse, Json, Response},
    routing::{get, post},
};
use nautilus_common::{
    clients::DataClient,
    live::runner::set_data_event_sender,
    messages::{
        DataEvent,
        data::{
            SubscribeBars, SubscribeBookDeltas, SubscribeBookDepth10, SubscribeFundingRates,
            SubscribeIndexPrices, SubscribeMarkPrices, SubscribeOptionGreeks, SubscribeQuotes,
            SubscribeTrades,
        },
    },
    testing::wait_until_async,
};
use nautilus_core::{UUID4, UnixNanos};
use nautilus_deribit::{
    common::{consts::DERIBIT_CLIENT_ID, enums::DeribitEnvironment},
    config::DeribitDataClientConfig,
    data::DeribitDataClient,
    http::models::DeribitProductType,
};
use nautilus_model::{
    data::{BarType, Data},
    enums::BookType,
    identifiers::InstrumentId,
};
use nautilus_network::http::HttpClient;
use rstest::rstest;
use serde_json::{Value, json};

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
    connection_count: Arc<tokio::sync::Mutex<usize>>,
    subscriptions: Arc<tokio::sync::Mutex<Vec<String>>>,
    subscription_events: Arc<tokio::sync::Mutex<Vec<(String, bool)>>>,
    auth_request_count: Arc<AtomicUsize>,
    disconnect_trigger: Arc<AtomicBool>,
    // When true, public/get_instrument responds with a JSON-RPC error,
    // exercising the lazy-load HTTP-failure path.
    fail_get_instrument: Arc<AtomicBool>,
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
        "public/get_instrument" => {
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

async fn handle_get_instruments(id: u64, params: Option<Value>) -> Response {
    let currency = params
        .as_ref()
        .and_then(|p| p.get("currency"))
        .and_then(|c| c.as_str());

    match currency {
        Some("any" | "BTC") | None => {
            let mut data = load_json("http_get_instruments.json");
            data["id"] = json!(id);

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
                                // Send a payload matching the subscribed channel's instrument
                                // so the handler's cache lookup keys to the right instrument
                                let payload_owned: Option<Value> = if channel.starts_with("trades.")
                                {
                                    Some(trades_payload.clone())
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
    }
}

#[rstest]
#[tokio::test]
async fn test_data_client_connect_disconnect() {
    let (addr, state) = start_test_server().await.unwrap();
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    set_data_event_sender(tx);

    let config = create_test_config(addr);
    let mut client = DeribitDataClient::new(*DERIBIT_CLIENT_ID, config).unwrap();
    assert!(!client.is_connected());

    client.connect().await.unwrap();
    assert!(client.is_connected());

    wait_until_async(
        || async { *state.connection_count.lock().await > 0 },
        Duration::from_secs(5),
    )
    .await;
    assert_eq!(*state.connection_count.lock().await, 1);

    client.disconnect().await.unwrap();
    assert!(!client.is_connected());
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
        Duration::from_secs(5),
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
        Duration::from_secs(5),
    )
    .await;

    let event = tokio::time::timeout(Duration::from_secs(5), rx.recv())
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
async fn test_data_client_subscribe_quotes() {
    let (addr, state) = start_test_server().await.unwrap();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    set_data_event_sender(tx);

    let config = create_test_config(addr);
    let mut client = DeribitDataClient::new(*DERIBIT_CLIENT_ID, config).unwrap();
    client.connect().await.unwrap();

    wait_until_async(
        || async { *state.connection_count.lock().await > 0 },
        Duration::from_secs(5),
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
        Duration::from_secs(5),
    )
    .await;

    let event = tokio::time::timeout(Duration::from_secs(5), rx.recv())
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
        Duration::from_secs(5),
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
        Duration::from_secs(5),
    )
    .await;

    let event = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("timeout waiting for event")
        .expect("channel closed");

    assert!(
        matches!(event, DataEvent::Data(Data::Deltas(_))),
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
        Duration::from_secs(5),
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

    // The strongest assertion: a Quote DataEvent for the option arrives. This
    // proves lazy-load fetched the option, seeded the WebSocket handler cache,
    // and the handler matched the inbound frame against the option (not the
    // already-cached BTC-PERPETUAL).
    let mut received_option_quote = false;
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while std::time::Instant::now() < deadline {
        if let Ok(Some(DataEvent::Data(Data::Quote(q)))) =
            tokio::time::timeout(Duration::from_millis(250), rx.recv()).await
            && q.instrument_id == option_id
        {
            received_option_quote = true;
            break;
        }
    }
    assert!(
        received_option_quote,
        "expected a Quote for the lazy-loaded option to flow through the handler"
    );

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_subscribe_quotes_lazy_load_http_failure_skips_ws_subscribe() {
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
        Duration::from_secs(5),
    )
    .await;

    // Force the next get_instrument to fail
    state.fail_get_instrument.store(true, Ordering::Relaxed);

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
        .expect("subscribe returns Ok; the failure is logged on the spawned task");

    // Allow the spawned lazy-load task to run and fail
    tokio::time::sleep(Duration::from_millis(500)).await;

    let saw_quote_channel = state
        .subscription_events
        .lock()
        .await
        .iter()
        .any(|(topic, _)| topic.starts_with("quote."));
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
        Duration::from_secs(5),
    )
    .await;

    let future_id = InstrumentId::from("BTC-27DEC24.DERIBIT");
    let cmd = SubscribeFundingRates::new(
        future_id,
        Some(*DERIBIT_CLIENT_ID),
        None,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );

    client
        .subscribe_funding_rates(cmd)
        .expect("subscribe returns Ok; rejection is async + logged");

    // Allow the spawned task to run, then assert no perpetual channel reached the server
    tokio::time::sleep(Duration::from_millis(500)).await;

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
        Duration::from_secs(5),
    )
    .await;

    assert!(
        instruments_received.load(Ordering::Relaxed) > 0,
        "Expected to receive instrument events on connect"
    );

    client.disconnect().await.unwrap();
}
