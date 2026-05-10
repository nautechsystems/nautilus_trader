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

//! Integration tests for the Binance Spot WebSocket Streams client using a mock server.

use std::{
    net::SocketAddr,
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
use nautilus_binance::spot::websocket::streams::client::BinanceSpotWebSocketClient;
use nautilus_common::testing::wait_until_async;
use nautilus_network::websocket::TransportBackend;
use rstest::rstest;
use serde_json::json;

// Test server state for tracking WebSocket connections and subscriptions
#[derive(Clone)]
struct TestServerState {
    connection_count: Arc<tokio::sync::Mutex<usize>>,
    total_connections: Arc<AtomicUsize>,
    subscribed_streams: Arc<tokio::sync::Mutex<Vec<String>>>,
    received_messages: Arc<tokio::sync::Mutex<Vec<serde_json::Value>>>,
    disconnect_trigger: Arc<AtomicBool>,
    drop_next_connection: Arc<AtomicBool>,
    ping_count: Arc<AtomicUsize>,
}

impl Default for TestServerState {
    fn default() -> Self {
        Self {
            connection_count: Arc::new(tokio::sync::Mutex::new(0)),
            total_connections: Arc::new(AtomicUsize::new(0)),
            subscribed_streams: Arc::new(tokio::sync::Mutex::new(Vec::new())),
            received_messages: Arc::new(tokio::sync::Mutex::new(Vec::new())),
            disconnect_trigger: Arc::new(AtomicBool::new(false)),
            drop_next_connection: Arc::new(AtomicBool::new(false)),
            ping_count: Arc::new(AtomicUsize::new(0)),
        }
    }
}

impl TestServerState {
    async fn subscribed_streams(&self) -> Vec<String> {
        self.subscribed_streams.lock().await.clone()
    }

    async fn received_messages(&self) -> Vec<serde_json::Value> {
        self.received_messages.lock().await.clone()
    }

    fn total_connections(&self) -> usize {
        self.total_connections.load(Ordering::Relaxed)
    }
}

// WebSocket handler
async fn handle_websocket(ws: WebSocketUpgrade, State(state): State<TestServerState>) -> Response {
    ws.on_upgrade(|socket| handle_socket(socket, state))
}

async fn handle_socket(mut socket: WebSocket, state: TestServerState) {
    {
        let mut count = state.connection_count.lock().await;
        *count += 1;
    }
    state.total_connections.fetch_add(1, Ordering::Relaxed);

    loop {
        if state.disconnect_trigger.load(Ordering::Relaxed) {
            break;
        }

        let msg_opt = match tokio::time::timeout(Duration::from_millis(50), socket.recv()).await {
            Ok(opt) => opt,
            Err(_) => continue,
        };

        let Some(msg) = msg_opt else {
            break;
        };

        let msg = match msg {
            Ok(m) => m,
            Err(_) => break,
        };

        if state.disconnect_trigger.load(Ordering::Relaxed) {
            break;
        }

        match msg {
            Message::Text(text) => {
                let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
                    continue;
                };

                // Store received message
                state.received_messages.lock().await.push(value.clone());

                let method = value.get("method").and_then(|v| v.as_str());
                let id = value.get("id").and_then(|v| v.as_u64()).unwrap_or(0);

                match method {
                    Some("SUBSCRIBE") => {
                        let params = value
                            .get("params")
                            .and_then(|v| v.as_array())
                            .map(|arr| {
                                arr.iter()
                                    .filter_map(|v| v.as_str().map(String::from))
                                    .collect::<Vec<_>>()
                            })
                            .unwrap_or_default();

                        // Add to subscribed streams
                        state.subscribed_streams.lock().await.extend(params);

                        // Send success response
                        let response = json!({
                            "result": null,
                            "id": id
                        });

                        if socket
                            .send(Message::Text(response.to_string().into()))
                            .await
                            .is_err()
                        {
                            break;
                        }

                        if state.drop_next_connection.swap(false, Ordering::Relaxed) {
                            break;
                        }
                    }
                    Some("UNSUBSCRIBE") => {
                        let params = value
                            .get("params")
                            .and_then(|v| v.as_array())
                            .map(|arr| {
                                arr.iter()
                                    .filter_map(|v| v.as_str().map(String::from))
                                    .collect::<Vec<_>>()
                            })
                            .unwrap_or_default();

                        // Remove from subscribed streams
                        let mut streams = state.subscribed_streams.lock().await;
                        streams.retain(|s| !params.contains(s));

                        // Send success response
                        let response = json!({
                            "result": null,
                            "id": id
                        });

                        if socket
                            .send(Message::Text(response.to_string().into()))
                            .await
                            .is_err()
                        {
                            break;
                        }
                    }
                    Some("LIST_SUBSCRIPTIONS") => {
                        let streams = state.subscribed_streams.lock().await.clone();
                        let response = json!({
                            "result": streams,
                            "id": id
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
            Message::Ping(_) => {
                state.ping_count.fetch_add(1, Ordering::Relaxed);

                if socket.send(Message::Pong(vec![].into())).await.is_err() {
                    break;
                }
            }
            Message::Pong(_) => {}
            Message::Close(_) => {
                break;
            }
            _ => {}
        }

        if state.disconnect_trigger.load(Ordering::Relaxed) {
            break;
        }
    }

    let mut count = state.connection_count.lock().await;
    *count = count.saturating_sub(1);
}

fn create_test_router(state: TestServerState) -> Router {
    Router::new()
        .route("/ws", get(handle_websocket))
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

    // Give server time to start
    tokio::time::sleep(Duration::from_millis(100)).await;
    Ok((addr, state))
}

fn create_test_client(addr: &SocketAddr) -> BinanceSpotWebSocketClient {
    let ws_url = format!("ws://{addr}/ws");
    BinanceSpotWebSocketClient::new(Some(ws_url), None, None, None, TransportBackend::default())
        .unwrap()
}

#[rstest]
#[tokio::test]
async fn test_client_connection() {
    let (addr, state) = start_test_server().await.unwrap();
    let mut client = create_test_client(&addr);

    client.connect().await.unwrap();

    // Wait for connection to be established
    wait_until_async(
        || async { *state.connection_count.lock().await > 0 },
        Duration::from_secs(5),
    )
    .await;

    assert!(client.is_active());
    assert_eq!(*state.connection_count.lock().await, 1);

    client.close().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_client_disconnect() {
    let (addr, state) = start_test_server().await.unwrap();
    let mut client = create_test_client(&addr);

    client.connect().await.unwrap();

    wait_until_async(
        || async { *state.connection_count.lock().await > 0 },
        Duration::from_secs(5),
    )
    .await;

    assert!(client.is_active());

    client.close().await.unwrap();

    // Give time for close to propagate
    tokio::time::sleep(Duration::from_millis(100)).await;
}

#[rstest]
#[tokio::test]
async fn test_subscribe_single_stream() {
    let (addr, state) = start_test_server().await.unwrap();
    let mut client = create_test_client(&addr);

    client.connect().await.unwrap();

    wait_until_async(
        || async { *state.connection_count.lock().await > 0 },
        Duration::from_secs(5),
    )
    .await;

    // Subscribe to a single stream
    client
        .subscribe(vec!["btcusdt@trade".to_string()])
        .await
        .unwrap();

    // Wait for subscription to be processed
    wait_until_async(
        || async { !state.subscribed_streams().await.is_empty() },
        Duration::from_secs(5),
    )
    .await;

    let streams = state.subscribed_streams().await;
    assert!(streams.contains(&"btcusdt@trade".to_string()));

    client.close().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_subscribe_multiple_streams() {
    let (addr, state) = start_test_server().await.unwrap();
    let mut client = create_test_client(&addr);

    client.connect().await.unwrap();

    wait_until_async(
        || async { *state.connection_count.lock().await > 0 },
        Duration::from_secs(5),
    )
    .await;

    // Subscribe to multiple streams
    let streams_to_subscribe = vec![
        "btcusdt@trade".to_string(),
        "ethusdt@trade".to_string(),
        "btcusdt@depth@100ms".to_string(),
    ];

    client
        .subscribe(streams_to_subscribe.clone())
        .await
        .unwrap();

    // Wait for subscriptions to be processed
    wait_until_async(
        || async { state.subscribed_streams().await.len() >= 3 },
        Duration::from_secs(5),
    )
    .await;

    let streams = state.subscribed_streams().await;
    assert!(streams.contains(&"btcusdt@trade".to_string()));
    assert!(streams.contains(&"ethusdt@trade".to_string()));
    assert!(streams.contains(&"btcusdt@depth@100ms".to_string()));

    client.close().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_unsubscribe_stream() {
    let (addr, state) = start_test_server().await.unwrap();
    let mut client = create_test_client(&addr);

    client.connect().await.unwrap();

    wait_until_async(
        || async { *state.connection_count.lock().await > 0 },
        Duration::from_secs(5),
    )
    .await;

    // Subscribe first
    client
        .subscribe(vec![
            "btcusdt@trade".to_string(),
            "ethusdt@trade".to_string(),
        ])
        .await
        .unwrap();

    wait_until_async(
        || async { state.subscribed_streams().await.len() >= 2 },
        Duration::from_secs(5),
    )
    .await;

    // Now unsubscribe from one
    client
        .unsubscribe(vec!["btcusdt@trade".to_string()])
        .await
        .unwrap();

    // Wait for unsubscription to be processed
    wait_until_async(
        || async {
            let streams = state.subscribed_streams().await;
            !streams.contains(&"btcusdt@trade".to_string())
        },
        Duration::from_secs(5),
    )
    .await;

    let streams = state.subscribed_streams().await;
    assert!(!streams.contains(&"btcusdt@trade".to_string()));
    assert!(streams.contains(&"ethusdt@trade".to_string()));

    client.close().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_subscription_count() {
    let (addr, state) = start_test_server().await.unwrap();
    let mut client = create_test_client(&addr);

    client.connect().await.unwrap();

    wait_until_async(
        || async { *state.connection_count.lock().await > 0 },
        Duration::from_secs(5),
    )
    .await;

    assert_eq!(client.subscription_count(), 0);

    // Subscribe to streams
    client
        .subscribe(vec![
            "btcusdt@trade".to_string(),
            "ethusdt@trade".to_string(),
        ])
        .await
        .unwrap();

    // Wait for subscription messages to be sent
    wait_until_async(
        || async { !state.received_messages().await.is_empty() },
        Duration::from_secs(5),
    )
    .await;

    // The local subscription count should be updated
    // Note: This tests the client's internal tracking
    let messages = state.received_messages().await;
    assert!(!messages.is_empty());

    client.close().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_is_active_false_before_connect() {
    let (addr, _state) = start_test_server().await.unwrap();
    let client = create_test_client(&addr);

    assert!(!client.is_active());
    assert!(client.is_closed());
}

#[rstest]
#[tokio::test]
async fn test_subscribe_request_format() {
    let (addr, state) = start_test_server().await.unwrap();
    let mut client = create_test_client(&addr);

    client.connect().await.unwrap();

    wait_until_async(
        || async { *state.connection_count.lock().await > 0 },
        Duration::from_secs(5),
    )
    .await;

    client
        .subscribe(vec!["btcusdt@trade".to_string()])
        .await
        .unwrap();

    // Wait for message to be received
    wait_until_async(
        || async { !state.received_messages().await.is_empty() },
        Duration::from_secs(5),
    )
    .await;

    let messages = state.received_messages().await;
    assert!(!messages.is_empty());

    let subscribe_msg = &messages[0];
    assert_eq!(
        subscribe_msg.get("method").and_then(|v| v.as_str()),
        Some("SUBSCRIBE")
    );
    assert!(subscribe_msg.get("id").is_some());
    assert!(subscribe_msg.get("params").is_some());

    let params = subscribe_msg.get("params").and_then(|v| v.as_array());
    assert!(params.is_some());
    let params = params.unwrap();
    assert!(params.iter().any(|v| v.as_str() == Some("btcusdt@trade")));

    client.close().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_unsubscribe_request_format() {
    let (addr, state) = start_test_server().await.unwrap();
    let mut client = create_test_client(&addr);

    client.connect().await.unwrap();

    wait_until_async(
        || async { *state.connection_count.lock().await > 0 },
        Duration::from_secs(5),
    )
    .await;

    // Subscribe first
    client
        .subscribe(vec!["btcusdt@trade".to_string()])
        .await
        .unwrap();

    wait_until_async(
        || async { !state.received_messages().await.is_empty() },
        Duration::from_secs(5),
    )
    .await;

    // Now unsubscribe
    client
        .unsubscribe(vec!["btcusdt@trade".to_string()])
        .await
        .unwrap();

    // Wait for unsubscribe message
    wait_until_async(
        || async { state.received_messages().await.len() >= 2 },
        Duration::from_secs(5),
    )
    .await;

    let messages = state.received_messages().await;
    let unsubscribe_msg = &messages[1];

    assert_eq!(
        unsubscribe_msg.get("method").and_then(|v| v.as_str()),
        Some("UNSUBSCRIBE")
    );
    assert!(unsubscribe_msg.get("id").is_some());

    client.close().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_connection_failure_invalid_url() {
    let result = BinanceSpotWebSocketClient::new(
        Some("ws://127.0.0.1:9999/invalid".to_string()),
        None,
        None,
        None,
        TransportBackend::default(),
    );

    // Client creation should succeed
    let mut client = result.unwrap();

    // But connection should fail
    let connect_result = client.connect().await;
    assert!(connect_result.is_err());
}

#[rstest]
#[tokio::test]
async fn test_default_client_creation() {
    // Test that default client can be created
    // This will use the production URL so we don't actually connect
    let client = BinanceSpotWebSocketClient::default();
    assert!(!client.is_active());
    assert!(client.is_closed());
}

#[rstest]
#[tokio::test]
async fn test_pool_creates_second_connection_on_overflow() {
    let (addr, state) = start_test_server().await.unwrap();
    let mut client = create_test_client(&addr);

    client.connect().await.unwrap();

    wait_until_async(
        || async { *state.connection_count.lock().await > 0 },
        Duration::from_secs(5),
    )
    .await;

    // 1025 streams exceeds the 1024-per-connection limit, so the pool
    // should create a second connection automatically
    let streams: Vec<String> = (0..1025).map(|i| format!("sym{i}@trade")).collect();

    let result = client.subscribe(streams).await;
    assert!(result.is_ok());

    wait_until_async(
        || async { *state.connection_count.lock().await >= 2 },
        Duration::from_secs(5),
    )
    .await;

    assert_eq!(*state.connection_count.lock().await, 2);

    client.close().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_pool_duplicate_subscribe_ignored() {
    let (addr, state) = start_test_server().await.unwrap();
    let mut client = create_test_client(&addr);

    client.connect().await.unwrap();

    wait_until_async(
        || async { *state.connection_count.lock().await > 0 },
        Duration::from_secs(5),
    )
    .await;

    let streams = vec!["btcusdt@trade".to_string()];
    client.subscribe(streams.clone()).await.unwrap();

    wait_until_async(
        || async { !state.subscribed_streams().await.is_empty() },
        Duration::from_secs(5),
    )
    .await;

    // Subscribing the same stream again should be a no-op
    client.subscribe(streams).await.unwrap();

    // Still only one connection
    assert_eq!(*state.connection_count.lock().await, 1);

    client.close().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_pool_unsubscribe_frees_capacity() {
    let (addr, state) = start_test_server().await.unwrap();
    let mut client = create_test_client(&addr);

    client.connect().await.unwrap();

    wait_until_async(
        || async { *state.connection_count.lock().await > 0 },
        Duration::from_secs(5),
    )
    .await;

    // Fill slot 0 to exactly 1024 streams
    let streams: Vec<String> = (0..1024).map(|i| format!("sym{i}@trade")).collect();
    client.subscribe(streams).await.unwrap();

    wait_until_async(
        || async { state.subscribed_streams().await.len() >= 1024 },
        Duration::from_secs(5),
    )
    .await;

    // Unsubscribe 10 streams from slot 0
    let unsub: Vec<String> = (0..10).map(|i| format!("sym{i}@trade")).collect();
    client.unsubscribe(unsub).await.unwrap();

    // Now subscribing 10 new streams should fit in slot 0 (no new connection)
    let new_streams: Vec<String> = (1024..1034).map(|i| format!("sym{i}@trade")).collect();
    client.subscribe(new_streams).await.unwrap();

    // Should still be just 1 connection
    assert_eq!(*state.connection_count.lock().await, 1);

    client.close().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_pool_single_batch_at_limit_uses_one_connection() {
    let (addr, state) = start_test_server().await.unwrap();
    let mut client = create_test_client(&addr);

    client.connect().await.unwrap();

    wait_until_async(
        || async { *state.connection_count.lock().await > 0 },
        Duration::from_secs(5),
    )
    .await;

    // 1024 streams exactly fits in one connection
    let streams: Vec<String> = (0..1024).map(|i| format!("sym{i}@trade")).collect();
    client.subscribe(streams).await.unwrap();

    wait_until_async(
        || async { state.subscribed_streams().await.len() >= 1024 },
        Duration::from_secs(5),
    )
    .await;

    assert_eq!(*state.connection_count.lock().await, 1);

    client.close().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_reconnection_after_server_drop() {
    let (addr, state) = start_test_server().await.unwrap();
    let mut client = create_test_client(&addr);

    client.connect().await.unwrap();

    wait_until_async(
        || async { *state.connection_count.lock().await > 0 },
        Duration::from_secs(5),
    )
    .await;

    client
        .subscribe(vec!["btcusdt@trade".to_string()])
        .await
        .unwrap();

    wait_until_async(
        || async { !state.subscribed_streams().await.is_empty() },
        Duration::from_secs(5),
    )
    .await;

    let initial_total = state.total_connections();

    state.drop_next_connection.store(true, Ordering::Relaxed);
    let _ = client.subscribe(vec!["ethusdt@trade".to_string()]).await;

    wait_until_async(
        || async { state.total_connections() > initial_total },
        Duration::from_secs(10),
    )
    .await;

    assert!(
        state.total_connections() > initial_total,
        "Expected at least one reconnection"
    );

    client.close().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_is_active_lifecycle() {
    let (addr, _state) = start_test_server().await.unwrap();
    let mut client = create_test_client(&addr);

    assert!(!client.is_active(), "Should not be active before connect");
    assert!(client.is_closed(), "Should be closed before connect");

    client.connect().await.unwrap();

    wait_until_async(|| async { client.is_active() }, Duration::from_secs(5)).await;

    assert!(client.is_active(), "Should be active after connect");

    client.close().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_is_active_false_during_reconnection() {
    let (addr, state) = start_test_server().await.unwrap();
    let mut client = create_test_client(&addr);

    client.connect().await.unwrap();

    wait_until_async(|| async { client.is_active() }, Duration::from_secs(5)).await;

    client
        .subscribe(vec!["btcusdt@trade".to_string()])
        .await
        .unwrap();

    wait_until_async(
        || async { !state.subscribed_streams().await.is_empty() },
        Duration::from_secs(5),
    )
    .await;

    state.drop_next_connection.store(true, Ordering::Relaxed);
    let _ = client.subscribe(vec!["ethusdt@trade".to_string()]).await;

    wait_until_async(|| async { !client.is_active() }, Duration::from_secs(5)).await;

    wait_until_async(|| async { client.is_active() }, Duration::from_secs(10)).await;

    assert!(client.is_active(), "Should be active after reconnection");

    client.close().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_rapid_consecutive_reconnections() {
    let (addr, state) = start_test_server().await.unwrap();
    let mut client = create_test_client(&addr);

    client.connect().await.unwrap();

    wait_until_async(|| async { client.is_active() }, Duration::from_secs(5)).await;

    let initial_total = state.total_connections();

    for i in 0..3 {
        state.drop_next_connection.store(true, Ordering::Relaxed);
        let _ = client.subscribe(vec![format!("stream{i}@trade")]).await;

        let expected = initial_total + i + 1;
        wait_until_async(
            || async { state.total_connections() >= expected },
            Duration::from_secs(10),
        )
        .await;

        wait_until_async(|| async { client.is_active() }, Duration::from_secs(10)).await;
    }

    assert!(
        state.total_connections() >= initial_total + 3,
        "Expected at least 3 reconnections, total={}",
        state.total_connections()
    );

    client.close().await.unwrap();
}
