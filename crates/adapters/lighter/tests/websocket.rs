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

//! Integration tests for the Lighter WebSocket client using a mock Axum server.

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
    response::Response,
    routing::get,
};
use futures_util::StreamExt;
use nautilus_common::testing::wait_until_async;
use nautilus_core::{UnixNanos, time::get_atomic_clock_realtime};
use nautilus_lighter::{
    common::LighterNetwork,
    http::models::OrderBooksResponse,
    http::parse::{instruments_from_defs, parse_instrument_defs},
    websocket::{LighterWebSocketClient, NautilusWsMessage},
};
use nautilus_model::instruments::InstrumentAny;
use serde_json::{Value, json};

/// Test server state for tracking connection events and controlling behavior.
#[derive(Clone, Default)]
struct TestServerState {
    connection_count: Arc<AtomicUsize>,
    total_connections: Arc<AtomicUsize>, // Monotonic counter - never decremented
    subscriptions: Arc<tokio::sync::Mutex<Vec<String>>>,
    drop_next_connection: Arc<AtomicBool>,
    messages_sent: Arc<AtomicUsize>,
}

impl TestServerState {
    fn new() -> Self {
        Self::default()
    }

    fn connection_count(&self) -> usize {
        self.connection_count.load(Ordering::SeqCst)
    }

    fn total_connections(&self) -> usize {
        self.total_connections.load(Ordering::SeqCst)
    }

    async fn subscription_count(&self) -> usize {
        self.subscriptions.lock().await.len()
    }
}

fn data_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../tests/test_data/lighter")
}

fn load_json(filename: &str) -> Value {
    let content = std::fs::read_to_string(data_path().join(filename))
        .unwrap_or_else(|_| panic!("failed to read {filename}"));
    serde_json::from_str(&content).expect("invalid json")
}

fn load_instruments() -> (HashMap<u32, InstrumentAny>, UnixNanos) {
    let content = std::fs::read_to_string(data_path().join("http/orderbooks.json"))
        .expect("failed to read orderbooks.json");
    let resp: OrderBooksResponse = serde_json::from_str(&content).expect("invalid orderbooks json");
    let books = resp.into_books();
    let (defs, _) = parse_instrument_defs(&books).expect("failed to parse instrument defs");
    let ts_init = get_atomic_clock_realtime().get_time_ns();
    let instruments = instruments_from_defs(&defs, ts_init).expect("failed to create instruments");

    let mut map = HashMap::new();
    for (i, def) in defs.iter().enumerate() {
        if let Some(inst) = instruments.get(i) {
            map.insert(def.market_index, inst.clone());
        }
    }
    (map, ts_init)
}

async fn handle_ws_upgrade(
    ws: WebSocketUpgrade,
    State(state): State<Arc<TestServerState>>,
) -> Response {
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

async fn handle_socket(mut socket: WebSocket, state: Arc<TestServerState>) {
    state.connection_count.fetch_add(1, Ordering::SeqCst);
    state.total_connections.fetch_add(1, Ordering::SeqCst); // Never decremented

    // Send connected message
    let connected = json!({
        "type": "connected",
        "session_id": "test-session-id"
    });
    if socket
        .send(Message::Text(connected.to_string().into()))
        .await
        .is_err()
    {
        state.connection_count.fetch_sub(1, Ordering::SeqCst);
        return;
    }
    state.messages_sent.fetch_add(1, Ordering::SeqCst);

    // Load fixtures for sending
    let order_book_fixture = load_json("public_order_book_1.json");
    let trade_fixture = load_json("public_trade_1.json");
    let market_stats_fixture = load_json("public_market_stats_1.json");

    while let Some(message) = socket.next().await {
        let Ok(message) = message else { break };

        match message {
            Message::Text(text) => {
                if let Ok(payload) = serde_json::from_str::<Value>(&text) {
                    let msg_type = payload.get("type").and_then(|v| v.as_str());
                    let channel = payload.get("channel").and_then(|v| v.as_str());

                    match msg_type {
                        Some("subscribe") => {
                            if let Some(ch) = channel {
                                // Record subscription
                                {
                                    let mut subs = state.subscriptions.lock().await;
                                    subs.push(ch.to_string());
                                }

                                // Send subscribed acknowledgment
                                let subscribed = json!({
                                    "type": "subscribed",
                                    "channel": ch
                                });
                                if socket
                                    .send(Message::Text(subscribed.to_string().into()))
                                    .await
                                    .is_err()
                                {
                                    break;
                                }
                                state.messages_sent.fetch_add(1, Ordering::SeqCst);

                                // Send fixture data based on channel type
                                if ch.starts_with("order_book") {
                                    // Send the snapshot message from the fixture (index 1)
                                    if let Some(snapshot) =
                                        order_book_fixture.as_array().and_then(|arr| arr.get(1))
                                    {
                                        if socket
                                            .send(Message::Text(snapshot.to_string().into()))
                                            .await
                                            .is_err()
                                        {
                                            break;
                                        }
                                        state.messages_sent.fetch_add(1, Ordering::SeqCst);
                                    }
                                } else if ch.starts_with("trade") {
                                    if let Some(snapshot) =
                                        trade_fixture.as_array().and_then(|arr| arr.get(1))
                                    {
                                        if socket
                                            .send(Message::Text(snapshot.to_string().into()))
                                            .await
                                            .is_err()
                                        {
                                            break;
                                        }
                                        state.messages_sent.fetch_add(1, Ordering::SeqCst);
                                    }
                                } else if ch.starts_with("market_stats")
                                    && let Some(snapshot) =
                                        market_stats_fixture.as_array().and_then(|arr| arr.get(1))
                                {
                                    if socket
                                        .send(Message::Text(snapshot.to_string().into()))
                                        .await
                                        .is_err()
                                    {
                                        break;
                                    }
                                    state.messages_sent.fetch_add(1, Ordering::SeqCst);
                                }

                                // Check if we should drop the connection
                                if state.drop_next_connection.swap(false, Ordering::SeqCst) {
                                    let _ = socket.send(Message::Close(None)).await;
                                    break;
                                }
                            }
                        }
                        Some("unsubscribe") => {
                            if let Some(ch) = channel {
                                let mut subs = state.subscriptions.lock().await;
                                subs.retain(|s| s != ch);

                                let unsubscribed = json!({
                                    "type": "unsubscribed",
                                    "channel": ch
                                });
                                if socket
                                    .send(Message::Text(unsubscribed.to_string().into()))
                                    .await
                                    .is_err()
                                {
                                    break;
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
            Message::Ping(payload) => {
                if socket.send(Message::Pong(payload)).await.is_err() {
                    break;
                }
            }
            Message::Close(_) => break,
            _ => {}
        }
    }

    state.connection_count.fetch_sub(1, Ordering::SeqCst);
}

async fn start_ws_server(state: Arc<TestServerState>) -> SocketAddr {
    let router = Router::new()
        .route("/ws", get(handle_ws_upgrade))
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

fn create_client(
    ws_url: &str,
    instruments: &HashMap<u32, InstrumentAny>,
) -> LighterWebSocketClient {
    let client = LighterWebSocketClient::new(LighterNetwork::Mainnet, Some(ws_url), None);
    for (&market_index, instrument) in instruments {
        client.cache_instrument(instrument.clone(), Some(market_index));
    }
    client
}

// =============================================================================
// TESTS
// =============================================================================

#[tokio::test]
async fn test_connect_and_subscribe() {
    let state = Arc::new(TestServerState::new());
    let addr = start_ws_server(state.clone()).await;
    let ws_url = format!("ws://{addr}/ws");

    let (instruments, _) = load_instruments();
    let mut client = create_client(&ws_url, &instruments);

    client.connect().await.expect("connect failed");
    client
        .wait_until_active(5000)
        .await
        .expect("client inactive");

    assert_eq!(state.connection_count(), 1);

    // Subscribe to order book
    client
        .subscribe_order_book(1)
        .await
        .expect("subscribe failed");

    // Wait for subscription to be recorded
    wait_until_async(
        || {
            let state = state.clone();
            async move { state.subscription_count().await >= 1 }
        },
        Duration::from_secs(2),
    )
    .await;

    assert!(state.subscription_count().await >= 1);

    client.close().await;

    wait_until_async(
        || {
            let state = state.clone();
            async move { state.connection_count() == 0 }
        },
        Duration::from_secs(2),
    )
    .await;
}

#[tokio::test]
async fn test_order_book_message_parsing() {
    let state = Arc::new(TestServerState::new());
    let addr = start_ws_server(state.clone()).await;
    let ws_url = format!("ws://{addr}/ws");

    let (instruments, _) = load_instruments();
    let mut client = create_client(&ws_url, &instruments);

    client.connect().await.expect("connect failed");
    client
        .wait_until_active(5000)
        .await
        .expect("client inactive");

    client
        .subscribe_order_book(1)
        .await
        .expect("subscribe failed");

    // Wait for and receive the order book message
    let event = tokio::time::timeout(Duration::from_secs(2), client.next_event())
        .await
        .expect("timeout waiting for event")
        .expect("no event received");

    match event {
        NautilusWsMessage::Deltas(deltas) => {
            assert!(!deltas.deltas.is_empty(), "expected order book deltas");
            assert_eq!(deltas.deltas[0].sequence, 2760693);
        }
        other => panic!("expected Deltas, got {other:?}"),
    }

    client.close().await;
}

#[tokio::test]
async fn test_trade_message_parsing() {
    let state = Arc::new(TestServerState::new());
    let addr = start_ws_server(state.clone()).await;
    let ws_url = format!("ws://{addr}/ws");

    let (instruments, _) = load_instruments();
    let mut client = create_client(&ws_url, &instruments);

    client.connect().await.expect("connect failed");
    client
        .wait_until_active(5000)
        .await
        .expect("client inactive");

    client.subscribe_trades(1).await.expect("subscribe failed");

    // Wait for and receive the trade message
    let event = tokio::time::timeout(Duration::from_secs(2), client.next_event())
        .await
        .expect("timeout waiting for event")
        .expect("no event received");

    match event {
        NautilusWsMessage::Trades(trades) => {
            assert!(!trades.is_empty(), "expected trade ticks");
        }
        other => panic!("expected Trades, got {other:?}"),
    }

    client.close().await;
}

#[tokio::test]
async fn test_market_stats_parsing() {
    let state = Arc::new(TestServerState::new());
    let addr = start_ws_server(state.clone()).await;
    let ws_url = format!("ws://{addr}/ws");

    let (instruments, _) = load_instruments();
    let mut client = create_client(&ws_url, &instruments);

    client.connect().await.expect("connect failed");
    client
        .wait_until_active(5000)
        .await
        .expect("client inactive");

    client
        .subscribe_market_stats(1)
        .await
        .expect("subscribe failed");

    // Wait for and receive market stats (may be MarkPrice, IndexPrice, or FundingRate)
    let event = tokio::time::timeout(Duration::from_secs(2), client.next_event())
        .await
        .expect("timeout waiting for event")
        .expect("no event received");

    match event {
        NautilusWsMessage::MarkPrice(_) => {}
        NautilusWsMessage::IndexPrice(_) => {}
        NautilusWsMessage::FundingRate(_) => {}
        other => panic!("expected market stats event, got {other:?}"),
    }

    client.close().await;
}

#[tokio::test]
async fn test_reconnect_after_disconnect() {
    let state = Arc::new(TestServerState::new());
    // Set flag to drop connection after first subscription
    state.drop_next_connection.store(true, Ordering::SeqCst);

    let addr = start_ws_server(state.clone()).await;
    let ws_url = format!("ws://{addr}/ws");

    let (instruments, _) = load_instruments();
    let mut client = create_client(&ws_url, &instruments);

    client.connect().await.expect("connect failed");
    client
        .wait_until_active(5000)
        .await
        .expect("client inactive");

    // Subscribe - this will trigger a disconnect
    client
        .subscribe_order_book(1)
        .await
        .expect("subscribe failed");

    // Wait for reconnection - total_connections must reach 2 (initial + reconnect)
    wait_until_async(
        || {
            let state = state.clone();
            async move {
                // total_connections is monotonic (never decremented), so >= 2 means
                // we had at least 2 connections: initial connection + reconnection
                state.total_connections() >= 2
            }
        },
        Duration::from_secs(5),
    )
    .await;

    // Client should still be active after reconnection
    client
        .wait_until_active(5000)
        .await
        .expect("client should reconnect");

    client.close().await;
}

#[tokio::test]
async fn test_resubscribe_after_reconnect() {
    let state = Arc::new(TestServerState::new());
    // Set flag to drop connection after first subscription
    state.drop_next_connection.store(true, Ordering::SeqCst);

    let addr = start_ws_server(state.clone()).await;
    let ws_url = format!("ws://{addr}/ws");

    let (instruments, _) = load_instruments();
    let mut client = create_client(&ws_url, &instruments);

    client.connect().await.expect("connect failed");
    client
        .wait_until_active(5000)
        .await
        .expect("client inactive");

    // Subscribe - this will trigger a disconnect
    client
        .subscribe_order_book(1)
        .await
        .expect("subscribe failed");

    // Wait for reconnection and resubscription
    // After reconnect, the client should automatically resubscribe
    wait_until_async(
        || {
            let state = state.clone();
            async move {
                // We should see at least 2 subscriptions (initial + resub after reconnect)
                state.subscription_count().await >= 2
            }
        },
        Duration::from_secs(5),
    )
    .await;

    // Verify subscriptions were recorded
    let subs = state.subscriptions.lock().await;
    let order_book_subs: Vec<_> = subs
        .iter()
        .filter(|s| s.starts_with("order_book"))
        .collect();
    assert!(
        order_book_subs.len() >= 2,
        "expected at least 2 order_book subscriptions (initial + resub), got {order_book_subs:?}",
    );

    client.close().await;
}

#[tokio::test]
async fn test_subscription_tracking() {
    let state = Arc::new(TestServerState::new());
    let addr = start_ws_server(state.clone()).await;
    let ws_url = format!("ws://{addr}/ws");

    let (instruments, _) = load_instruments();
    let mut client = create_client(&ws_url, &instruments);

    client.connect().await.expect("connect failed");
    client
        .wait_until_active(5000)
        .await
        .expect("client inactive");

    // Subscribe to multiple channels
    client
        .subscribe_order_book(1)
        .await
        .expect("subscribe order_book failed");
    client
        .subscribe_trades(1)
        .await
        .expect("subscribe trades failed");
    client
        .subscribe_market_stats(1)
        .await
        .expect("subscribe market_stats failed");

    // Wait for all subscriptions
    wait_until_async(
        || {
            let state = state.clone();
            async move { state.subscription_count().await >= 3 }
        },
        Duration::from_secs(2),
    )
    .await;

    assert!(state.subscription_count().await >= 3);

    // Unsubscribe from order book
    client
        .unsubscribe_order_book(1)
        .await
        .expect("unsubscribe failed");

    // Give the server time to process
    tokio::time::sleep(Duration::from_millis(100)).await;

    // We should still have trades and market_stats
    let subs = state.subscriptions.lock().await;
    assert!(
        subs.iter().any(|s| s.starts_with("trade")),
        "expected trades subscription to remain"
    );
    assert!(
        subs.iter().any(|s| s.starts_with("market_stats")),
        "expected market_stats subscription to remain"
    );

    client.close().await;
}

#[tokio::test]
async fn test_wait_until_active_timeout() {
    // Create client with invalid URL - should not connect
    let client = LighterWebSocketClient::new(
        LighterNetwork::Mainnet,
        Some("ws://127.0.0.1:1/invalid"),
        None,
    );

    let result = client.wait_until_active(100).await;
    assert!(result.is_err(), "expected timeout error");
}
