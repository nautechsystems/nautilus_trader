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

//! Integration tests for the Deribit WebSocket client using a mock Axum server.

use std::{
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
    response::Response,
    routing::get,
};
use futures_util::{StreamExt, pin_mut};
use nautilus_common::testing::wait_until_async;
use nautilus_core::UnixNanos;
use nautilus_deribit::websocket::{client::DeribitWebSocketClient, messages::NautilusWsMessage};
use nautilus_model::{
    identifiers::{InstrumentId, Symbol, Venue},
    instruments::{CryptoPerpetual, InstrumentAny},
    types::{Currency, Price, Quantity},
};
use serde_json::{Value, json};

// ------------------------------------------------------------------------------------------------
// Test Data Helpers
// ------------------------------------------------------------------------------------------------

fn data_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test_data")
}

fn load_json(filename: &str) -> Value {
    let content = std::fs::read_to_string(data_path().join(filename))
        .unwrap_or_else(|_| panic!("failed to read {filename}"));
    serde_json::from_str(&content).expect("invalid json")
}

/// Creates a mock BTC-PERPETUAL instrument for testing.
fn create_btc_perpetual() -> InstrumentAny {
    InstrumentAny::CryptoPerpetual(CryptoPerpetual::new(
        InstrumentId::new(Symbol::from("BTC-PERPETUAL"), Venue::from("DERIBIT")),
        Symbol::from("BTC-PERPETUAL"),
        Currency::BTC(),
        Currency::USD(),
        Currency::BTC(),
        false,
        1, // price_precision
        0, // size_precision
        Price::new(0.5, 1),
        Quantity::new(1.0, 0),
        None, // multiplier
        None, // lot_size
        None, // max_quantity
        None, // min_quantity
        None, // max_notional
        None, // min_notional
        None, // max_price
        None, // min_price
        None, // margin_init
        None, // margin_maint
        None, // maker_fee
        None, // taker_fee
        UnixNanos::default(),
        UnixNanos::default(),
    ))
}

fn load_test_instruments() -> Vec<InstrumentAny> {
    vec![create_btc_perpetual()]
}

// ------------------------------------------------------------------------------------------------
// Test Server State
// ------------------------------------------------------------------------------------------------

#[derive(Clone, Default)]
struct TestServerState {
    connection_count: Arc<tokio::sync::Mutex<usize>>,
    subscriptions: Arc<tokio::sync::Mutex<Vec<String>>>,
    subscription_events: Arc<tokio::sync::Mutex<Vec<(String, bool)>>>,
    unsubscriptions: Arc<tokio::sync::Mutex<Vec<String>>>,
    heartbeat_enabled: Arc<AtomicBool>,
    heartbeat_interval: Arc<tokio::sync::Mutex<Option<u64>>>,
    test_request_count: Arc<AtomicUsize>,
    test_response_count: Arc<AtomicUsize>,
    disconnect_trigger: Arc<AtomicBool>,
    fail_next_subscriptions: Arc<tokio::sync::Mutex<Vec<String>>>,
    drop_next_connection: Arc<AtomicBool>,
    send_test_request: Arc<AtomicBool>,
}

impl TestServerState {
    async fn subscription_events(&self) -> Vec<(String, bool)> {
        self.subscription_events.lock().await.clone()
    }

    async fn clear_subscription_events(&self) {
        self.subscription_events.lock().await.clear();
    }
}

// ------------------------------------------------------------------------------------------------
// Mock WebSocket Handler
// ------------------------------------------------------------------------------------------------

async fn handle_ws_upgrade(
    ws: WebSocketUpgrade,
    State(state): State<Arc<TestServerState>>,
) -> Response {
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

async fn handle_socket(mut socket: WebSocket, state: Arc<TestServerState>) {
    {
        let mut count = state.connection_count.lock().await;
        *count += 1;
    }

    // Load test data payloads
    let trades_payload = load_json("ws_trades.json");
    let book_snapshot_payload = load_json("ws_book_snapshot.json");
    let book_delta_payload = load_json("ws_book_delta.json");
    let ticker_payload = load_json("ws_ticker.json");
    let quote_payload = load_json("ws_quote.json");

    while let Some(message) = socket.recv().await {
        let Ok(message) = message else { break };

        // Check for disconnect trigger
        if state.disconnect_trigger.load(Ordering::Relaxed) {
            let _ = socket.send(Message::Close(None)).await;
            break;
        }

        match message {
            Message::Text(text) => {
                // Parse JSON-RPC request
                let Ok(payload) = serde_json::from_str::<Value>(&text) else {
                    continue;
                };

                let method = payload.get("method").and_then(|m| m.as_str());
                let id = payload.get("id").and_then(|i| i.as_u64());

                match method {
                    Some("public/subscribe") => {
                        if let Some(params) = payload.get("params")
                            && let Some(channels) =
                                params.get("channels").and_then(|c| c.as_array())
                        {
                            let mut subscribed_channels = Vec::new();
                            let fail_list = state.fail_next_subscriptions.lock().await.clone();

                            for channel in channels {
                                if let Some(channel_str) = channel.as_str() {
                                    let should_fail = fail_list.contains(&channel_str.to_string());

                                    state
                                        .subscription_events
                                        .lock()
                                        .await
                                        .push((channel_str.to_string(), !should_fail));

                                    if !should_fail {
                                        subscribed_channels.push(channel_str.to_string());
                                        state
                                            .subscriptions
                                            .lock()
                                            .await
                                            .push(channel_str.to_string());
                                    }
                                }
                            }

                            // Send subscription response
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

                            // Send test data based on channel type
                            for channel in &subscribed_channels {
                                let data_payload = if channel.starts_with("trades.") {
                                    Some(&trades_payload)
                                } else if channel.starts_with("book.") {
                                    // Send snapshot first, then delta
                                    if socket
                                        .send(Message::Text(
                                            book_snapshot_payload.to_string().into(),
                                        ))
                                        .await
                                        .is_err()
                                    {
                                        break;
                                    }
                                    Some(&book_delta_payload)
                                } else if channel.starts_with("ticker.") {
                                    Some(&ticker_payload)
                                } else if channel.starts_with("quote.") {
                                    Some(&quote_payload)
                                } else {
                                    None
                                };

                                if let Some(payload) = data_payload
                                    && socket
                                        .send(Message::Text(payload.to_string().into()))
                                        .await
                                        .is_err()
                                {
                                    break;
                                }
                            }

                            // Check if we should drop the connection after subscription
                            if state.drop_next_connection.swap(false, Ordering::Relaxed) {
                                let _ = socket.send(Message::Close(None)).await;
                                break;
                            }
                        }
                    }
                    Some("public/unsubscribe") => {
                        if let Some(params) = payload.get("params")
                            && let Some(channels) =
                                params.get("channels").and_then(|c| c.as_array())
                        {
                            let mut unsubscribed = Vec::new();
                            for channel in channels {
                                if let Some(channel_str) = channel.as_str() {
                                    state
                                        .unsubscriptions
                                        .lock()
                                        .await
                                        .push(channel_str.to_string());
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
                        if let Some(params) = payload.get("params")
                            && let Some(interval) = params.get("interval").and_then(|i| i.as_u64())
                        {
                            state.heartbeat_enabled.store(true, Ordering::Relaxed);
                            *state.heartbeat_interval.lock().await = Some(interval);

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

                            // Send test_request if configured
                            if state.send_test_request.load(Ordering::Relaxed) {
                                let test_request = json!({
                                    "jsonrpc": "2.0",
                                    "method": "heartbeat",
                                    "params": {
                                        "type": "test_request"
                                    }
                                });
                                state.test_request_count.fetch_add(1, Ordering::Relaxed);
                                if socket
                                    .send(Message::Text(test_request.to_string().into()))
                                    .await
                                    .is_err()
                                {
                                    break;
                                }
                            }
                        }
                    }
                    Some("public/test") => {
                        // Client responding to test_request
                        state.test_response_count.fetch_add(1, Ordering::Relaxed);

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

                        // Send heartbeat acknowledgment
                        let heartbeat = json!({
                            "jsonrpc": "2.0",
                            "method": "heartbeat",
                            "params": {
                                "type": "heartbeat"
                            }
                        });

                        if socket
                            .send(Message::Text(heartbeat.to_string().into()))
                            .await
                            .is_err()
                        {
                            break;
                        }
                    }
                    _ => {
                        // Unknown method - could send error
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

    // Cleanup on disconnect
    let mut count = state.connection_count.lock().await;
    *count = count.saturating_sub(1);
}

async fn start_ws_server(state: Arc<TestServerState>) -> SocketAddr {
    let router = Router::new()
        .route("/ws/api/v2", get(handle_ws_upgrade))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("failed to bind websocket listener");
    let addr = listener.local_addr().expect("missing local addr");

    tokio::spawn(async move {
        axum::serve(listener, router)
            .await
            .expect("websocket server failed");
    });

    tokio::time::sleep(Duration::from_millis(50)).await;
    addr
}

fn create_test_client(ws_url: &str) -> DeribitWebSocketClient {
    DeribitWebSocketClient::new(
        Some(ws_url.to_string()),
        None,     // api_key
        None,     // api_secret
        Some(30), // heartbeat_interval
        true,     // is_testnet
    )
    .expect("failed to construct deribit websocket client")
}

// ================================================================================================
// Connection Tests
// ================================================================================================

#[tokio::test]
async fn test_websocket_connection() {
    let state = Arc::new(TestServerState::default());
    let addr = start_ws_server(state.clone()).await;
    let ws_url = format!("ws://{addr}/ws/api/v2");

    let instruments = load_test_instruments();

    let mut client = create_test_client(&ws_url);
    client.cache_instruments(instruments);
    client.connect().await.expect("connect failed");

    wait_until_async(
        || {
            let state = state.clone();
            async move { *state.connection_count.lock().await == 1 }
        },
        Duration::from_secs(2),
    )
    .await;

    assert!(client.is_active());

    client.close().await.expect("close failed");

    wait_until_async(
        || {
            let state = state.clone();
            async move { *state.connection_count.lock().await == 0 }
        },
        Duration::from_secs(2),
    )
    .await;
}

#[tokio::test]
async fn test_wait_until_active_timeout() {
    let client = DeribitWebSocketClient::new(
        Some("ws://127.0.0.1:0/ws/api/v2".to_string()),
        None,
        None,
        Some(30),
        true,
    )
    .expect("construct client");

    let result = client.wait_until_active(0.1).await;
    assert!(result.is_err(), "expected timeout error");
}

#[tokio::test]
async fn test_is_active_and_is_closed_states() {
    let state = Arc::new(TestServerState::default());
    let addr = start_ws_server(state.clone()).await;
    let ws_url = format!("ws://{addr}/ws/api/v2");

    let mut client = create_test_client(&ws_url);

    // Before connect
    assert!(!client.is_active());

    // After connect
    client.connect().await.expect("connect failed");
    wait_until_async(
        || {
            let state = state.clone();
            async move { *state.connection_count.lock().await == 1 }
        },
        Duration::from_secs(2),
    )
    .await;

    assert!(client.is_active());
    assert!(!client.is_closed());

    // After close
    client.close().await.expect("close failed");

    wait_until_async(
        || {
            let client = client.clone();
            async move { client.is_closed() }
        },
        Duration::from_secs(2),
    )
    .await;

    assert!(client.is_closed());
}

// ================================================================================================
// Subscription Tests
// ================================================================================================

#[tokio::test]
async fn test_trades_subscription_flow() {
    let state = Arc::new(TestServerState::default());
    let addr = start_ws_server(state.clone()).await;
    let ws_url = format!("ws://{addr}/ws/api/v2");

    let instruments = load_test_instruments();

    let mut client = create_test_client(&ws_url);
    client.cache_instruments(instruments);
    client.connect().await.expect("connect failed");
    client
        .wait_until_active(5.0)
        .await
        .expect("client inactive");

    let instrument_id = InstrumentId::from("BTC-PERPETUAL.DERIBIT");
    client
        .subscribe_trades(instrument_id)
        .await
        .expect("subscribe failed");

    // Verify subscription event recorded
    wait_until_async(
        || {
            let state = state.clone();
            async move {
                state
                    .subscription_events()
                    .await
                    .iter()
                    .any(|(ch, ok)| ch.starts_with("trades.") && *ok)
            }
        },
        Duration::from_secs(2),
    )
    .await;

    // Receive trade data from stream
    let stream = client.stream();
    pin_mut!(stream);
    let message = tokio::time::timeout(Duration::from_secs(2), stream.next())
        .await
        .expect("no message received")
        .expect("stream ended unexpectedly");

    match message {
        NautilusWsMessage::Data(data) => {
            assert!(!data.is_empty(), "expected trade payload");
        }
        other => panic!("unexpected message: {other:?}"),
    }

    client.close().await.expect("close failed");
}

#[tokio::test]
async fn test_book_subscription_snapshot() {
    let state = Arc::new(TestServerState::default());
    let addr = start_ws_server(state.clone()).await;
    let ws_url = format!("ws://{addr}/ws/api/v2");

    let instruments = load_test_instruments();

    let mut client = create_test_client(&ws_url);
    client.cache_instruments(instruments);
    client.connect().await.expect("connect failed");
    client
        .wait_until_active(5.0)
        .await
        .expect("client inactive");

    let instrument_id = InstrumentId::from("BTC-PERPETUAL.DERIBIT");
    client
        .subscribe_book(instrument_id)
        .await
        .expect("subscribe failed");

    // Verify subscription event recorded
    wait_until_async(
        || {
            let state = state.clone();
            async move {
                state
                    .subscription_events()
                    .await
                    .iter()
                    .any(|(ch, ok)| ch.starts_with("book.") && *ok)
            }
        },
        Duration::from_secs(2),
    )
    .await;

    // Receive book data from stream (should receive snapshot first)
    let stream = client.stream();
    pin_mut!(stream);
    let message = tokio::time::timeout(Duration::from_secs(2), stream.next())
        .await
        .expect("no message received")
        .expect("stream ended unexpectedly");

    match message {
        NautilusWsMessage::Deltas(deltas) => {
            assert!(!deltas.deltas.is_empty(), "expected book deltas");
        }
        other => panic!("unexpected message: {other:?}"),
    }

    client.close().await.expect("close failed");
}

#[tokio::test]
async fn test_ticker_subscription_flow() {
    let state = Arc::new(TestServerState::default());
    let addr = start_ws_server(state.clone()).await;
    let ws_url = format!("ws://{addr}/ws/api/v2");

    let instruments = load_test_instruments();

    let mut client = create_test_client(&ws_url);
    client.cache_instruments(instruments);
    client.connect().await.expect("connect failed");
    client
        .wait_until_active(5.0)
        .await
        .expect("client inactive");

    let instrument_id = InstrumentId::from("BTC-PERPETUAL.DERIBIT");
    client
        .subscribe_ticker(instrument_id)
        .await
        .expect("subscribe failed");

    // Verify subscription event recorded
    wait_until_async(
        || {
            let state = state.clone();
            async move {
                state
                    .subscription_events()
                    .await
                    .iter()
                    .any(|(ch, ok)| ch.starts_with("ticker.") && *ok)
            }
        },
        Duration::from_secs(2),
    )
    .await;

    // Receive ticker data from stream
    let stream = client.stream();
    pin_mut!(stream);
    let message = tokio::time::timeout(Duration::from_secs(2), stream.next())
        .await
        .expect("no message received")
        .expect("stream ended unexpectedly");

    match message {
        NautilusWsMessage::Data(data) => {
            assert!(!data.is_empty(), "expected ticker payload");
        }
        other => panic!("unexpected message: {other:?}"),
    }

    client.close().await.expect("close failed");
}

#[tokio::test]
async fn test_quote_subscription_flow() {
    let state = Arc::new(TestServerState::default());
    let addr = start_ws_server(state.clone()).await;
    let ws_url = format!("ws://{addr}/ws/api/v2");

    let instruments = load_test_instruments();

    let mut client = create_test_client(&ws_url);
    client.cache_instruments(instruments);
    client.connect().await.expect("connect failed");
    client
        .wait_until_active(5.0)
        .await
        .expect("client inactive");

    let instrument_id = InstrumentId::from("BTC-PERPETUAL.DERIBIT");
    client
        .subscribe_quotes(instrument_id)
        .await
        .expect("subscribe failed");

    // Verify subscription event recorded
    wait_until_async(
        || {
            let state = state.clone();
            async move {
                state
                    .subscription_events()
                    .await
                    .iter()
                    .any(|(ch, ok)| ch.starts_with("quote.") && *ok)
            }
        },
        Duration::from_secs(2),
    )
    .await;

    // Receive quote data from stream
    let stream = client.stream();
    pin_mut!(stream);
    let message = tokio::time::timeout(Duration::from_secs(2), stream.next())
        .await
        .expect("no message received")
        .expect("stream ended unexpectedly");

    match message {
        NautilusWsMessage::Data(data) => {
            assert!(!data.is_empty(), "expected quote payload");
        }
        other => panic!("unexpected message: {other:?}"),
    }

    client.close().await.expect("close failed");
}

#[tokio::test]
async fn test_multiple_subscriptions() {
    let state = Arc::new(TestServerState::default());
    let addr = start_ws_server(state.clone()).await;
    let ws_url = format!("ws://{addr}/ws/api/v2");

    let instruments = load_test_instruments();

    let mut client = create_test_client(&ws_url);
    client.cache_instruments(instruments);
    client.connect().await.expect("connect failed");
    client
        .wait_until_active(5.0)
        .await
        .expect("client inactive");

    let instrument_id = InstrumentId::from("BTC-PERPETUAL.DERIBIT");

    // Subscribe to multiple channels
    client
        .subscribe_trades(instrument_id)
        .await
        .expect("subscribe trades failed");
    client
        .subscribe_ticker(instrument_id)
        .await
        .expect("subscribe ticker failed");

    // Verify all subscription events recorded
    wait_until_async(
        || {
            let state = state.clone();
            async move {
                let events = state.subscription_events().await;
                events
                    .iter()
                    .any(|(ch, ok)| ch.starts_with("trades.") && *ok)
                    && events
                        .iter()
                        .any(|(ch, ok)| ch.starts_with("ticker.") && *ok)
            }
        },
        Duration::from_secs(2),
    )
    .await;

    client.close().await.expect("close failed");
}

#[tokio::test]
async fn test_unsubscribe() {
    let state = Arc::new(TestServerState::default());
    let addr = start_ws_server(state.clone()).await;
    let ws_url = format!("ws://{addr}/ws/api/v2");

    let instruments = load_test_instruments();

    let mut client = create_test_client(&ws_url);
    client.cache_instruments(instruments);
    client.connect().await.expect("connect failed");
    client
        .wait_until_active(5.0)
        .await
        .expect("client inactive");

    let instrument_id = InstrumentId::from("BTC-PERPETUAL.DERIBIT");

    // Subscribe
    client
        .subscribe_trades(instrument_id)
        .await
        .expect("subscribe failed");

    wait_until_async(
        || {
            let state = state.clone();
            async move {
                state
                    .subscription_events()
                    .await
                    .iter()
                    .any(|(ch, ok)| ch.starts_with("trades.") && *ok)
            }
        },
        Duration::from_secs(2),
    )
    .await;

    // Unsubscribe
    client
        .unsubscribe_trades(instrument_id)
        .await
        .expect("unsubscribe failed");

    // Verify unsubscription recorded
    wait_until_async(
        || {
            let state = state.clone();
            async move {
                let unsubs = state.unsubscriptions.lock().await;
                unsubs.iter().any(|ch| ch.starts_with("trades."))
            }
        },
        Duration::from_secs(2),
    )
    .await;

    client.close().await.expect("close failed");
}

// ================================================================================================
// Heartbeat Tests
// ================================================================================================

#[tokio::test]
async fn test_heartbeat_enable() {
    let state = Arc::new(TestServerState::default());
    let addr = start_ws_server(state.clone()).await;
    let ws_url = format!("ws://{addr}/ws/api/v2");

    let instruments = load_test_instruments();

    let mut client = create_test_client(&ws_url);
    client.cache_instruments(instruments);
    client.connect().await.expect("connect failed");
    client
        .wait_until_active(5.0)
        .await
        .expect("client inactive");

    // Heartbeat should be automatically enabled on connect (configured with 30s interval)
    wait_until_async(
        || {
            let state = state.clone();
            async move { state.heartbeat_enabled.load(Ordering::Relaxed) }
        },
        Duration::from_secs(2),
    )
    .await;

    let interval = state.heartbeat_interval.lock().await;
    assert_eq!(*interval, Some(30));

    client.close().await.expect("close failed");
}

#[tokio::test]
async fn test_heartbeat_test_request_response() {
    let state = Arc::new(TestServerState::default());
    state.send_test_request.store(true, Ordering::Relaxed);

    let addr = start_ws_server(state.clone()).await;
    let ws_url = format!("ws://{addr}/ws/api/v2");

    let instruments = load_test_instruments();

    let mut client = create_test_client(&ws_url);
    client.cache_instruments(instruments);
    client.connect().await.expect("connect failed");
    client
        .wait_until_active(5.0)
        .await
        .expect("client inactive");

    // Heartbeat is automatically enabled on connect (configured with 10s interval)
    // Wait for heartbeat to be enabled
    wait_until_async(
        || {
            let state = state.clone();
            async move { state.heartbeat_enabled.load(Ordering::Relaxed) }
        },
        Duration::from_secs(2),
    )
    .await;

    // Wait for test_request to be sent by server
    wait_until_async(
        || {
            let state = state.clone();
            async move { state.test_request_count.load(Ordering::Relaxed) > 0 }
        },
        Duration::from_secs(2),
    )
    .await;

    // Wait for client to respond with public/test
    wait_until_async(
        || {
            let state = state.clone();
            async move { state.test_response_count.load(Ordering::Relaxed) > 0 }
        },
        Duration::from_secs(2),
    )
    .await;

    assert_eq!(
        state.test_request_count.load(Ordering::Relaxed),
        1,
        "Server should have sent one test_request"
    );
    assert_eq!(
        state.test_response_count.load(Ordering::Relaxed),
        1,
        "Client should have responded to test_request"
    );

    client.close().await.expect("close failed");
}

// ================================================================================================
// Error Handling Tests
// ================================================================================================

#[tokio::test]
async fn test_subscription_failure_handling() {
    let state = Arc::new(TestServerState::default());
    {
        let mut pending = state.fail_next_subscriptions.lock().await;
        pending.push("trades.BTC-PERPETUAL.100ms".to_string());
    }

    let addr = start_ws_server(state.clone()).await;
    let ws_url = format!("ws://{addr}/ws/api/v2");

    let instruments = load_test_instruments();

    let mut client = create_test_client(&ws_url);
    client.cache_instruments(instruments);
    client.connect().await.expect("connect failed");
    client
        .wait_until_active(5.0)
        .await
        .expect("client inactive");

    let instrument_id = InstrumentId::from("BTC-PERPETUAL.DERIBIT");
    client
        .subscribe_trades(instrument_id)
        .await
        .expect("subscribe call should not fail");

    // Verify failure event recorded
    wait_until_async(
        || {
            let state = state.clone();
            async move {
                state
                    .subscription_events()
                    .await
                    .iter()
                    .any(|(ch, ok)| ch.starts_with("trades.") && !ok)
            }
        },
        Duration::from_secs(2),
    )
    .await;

    client.close().await.expect("close failed");
}

// ================================================================================================
// Reconnection Tests
// ================================================================================================

#[tokio::test]
async fn test_reconnection_after_disconnect() {
    let state = Arc::new(TestServerState::default());
    state.drop_next_connection.store(true, Ordering::Relaxed);

    let addr = start_ws_server(state.clone()).await;
    let ws_url = format!("ws://{addr}/ws/api/v2");

    let instruments = load_test_instruments();

    let mut client = create_test_client(&ws_url);
    client.cache_instruments(instruments);
    client.connect().await.expect("connect failed");
    client
        .wait_until_active(5.0)
        .await
        .expect("client inactive");

    let instrument_id = InstrumentId::from("BTC-PERPETUAL.DERIBIT");
    client
        .subscribe_trades(instrument_id)
        .await
        .expect("subscribe failed");

    // Wait for initial subscription
    wait_until_async(
        || {
            let state = state.clone();
            async move {
                state
                    .subscription_events()
                    .await
                    .iter()
                    .any(|(ch, ok)| ch.starts_with("trades.") && *ok)
            }
        },
        Duration::from_secs(2),
    )
    .await;

    state.clear_subscription_events().await;

    // Wait for reconnection and resubscription
    wait_until_async(
        || {
            let state = state.clone();
            async move {
                // Should have at least 2 subscriptions total (initial + reconnect)
                state.subscriptions.lock().await.len() >= 2
            }
        },
        Duration::from_secs(5),
    )
    .await;

    client.close().await.expect("close failed");
}

// ================================================================================================
// Instrument Cache Tests
// ================================================================================================

#[tokio::test]
async fn test_instrument_cache_usage() {
    let state = Arc::new(TestServerState::default());
    let addr = start_ws_server(state.clone()).await;
    let ws_url = format!("ws://{addr}/ws/api/v2");

    let instruments = load_test_instruments();

    let mut client = create_test_client(&ws_url);

    // Cache instruments before connect
    client.cache_instruments(instruments);

    client.connect().await.expect("connect failed");
    client
        .wait_until_active(5.0)
        .await
        .expect("client inactive");

    let instrument_id = InstrumentId::from("BTC-PERPETUAL.DERIBIT");
    client
        .subscribe_trades(instrument_id)
        .await
        .expect("subscribe failed");

    // Receive and verify trade data is properly parsed using cached instrument
    let stream = client.stream();
    pin_mut!(stream);
    let message = tokio::time::timeout(Duration::from_secs(2), stream.next())
        .await
        .expect("no message received")
        .expect("stream ended unexpectedly");

    match message {
        NautilusWsMessage::Data(data) => {
            assert!(!data.is_empty(), "expected parsed trade data");
            // Trades should be parsed correctly with instrument metadata
        }
        other => panic!("unexpected message: {other:?}"),
    }

    client.close().await.expect("close failed");
}

#[tokio::test]
async fn test_cache_instrument_single() {
    let state = Arc::new(TestServerState::default());
    let addr = start_ws_server(state.clone()).await;
    let ws_url = format!("ws://{addr}/ws/api/v2");

    let mut client = create_test_client(&ws_url);

    // Cache single instrument
    let instrument = create_btc_perpetual();
    client.cache_instrument(instrument);

    client.connect().await.expect("connect failed");
    client
        .wait_until_active(5.0)
        .await
        .expect("client inactive");

    let instrument_id = InstrumentId::from("BTC-PERPETUAL.DERIBIT");
    client
        .subscribe_trades(instrument_id)
        .await
        .expect("subscribe failed");

    // Verify trades can be parsed with cached instrument
    let stream = client.stream();
    pin_mut!(stream);
    let message = tokio::time::timeout(Duration::from_secs(2), stream.next())
        .await
        .expect("no message received")
        .expect("stream ended unexpectedly");

    match message {
        NautilusWsMessage::Data(data) => {
            assert!(!data.is_empty(), "expected parsed trade data");
        }
        other => panic!("unexpected message: {other:?}"),
    }

    client.close().await.expect("close failed");
}
