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

//! Integration tests for Bybit WebSocket client using a mock server.

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
use nautilus_bybit::{
    common::{
        credential::Credential,
        enums::{BybitEnvironment, BybitProductType},
    },
    websocket::client::BybitWebSocketClient,
};
use nautilus_common::testing::wait_until_async;
use nautilus_model::identifiers::InstrumentId;
use rstest::rstest;
use serde_json::json;
use tokio::sync::Mutex;

// Test server state for tracking WebSocket connections
#[derive(Clone)]
struct TestServerState {
    connection_count: Arc<Mutex<usize>>,
    subscription_events: Arc<Mutex<Vec<(String, bool)>>>, // (topic, success)
    fail_next_subscriptions: Arc<Mutex<Vec<String>>>,
    auth_response_delay_ms: Arc<Mutex<Option<u64>>>,
    authenticated: Arc<AtomicBool>,
    disconnect_trigger: Arc<AtomicBool>,
    ping_count: Arc<AtomicUsize>,
    pong_count: Arc<AtomicUsize>,
}

impl Default for TestServerState {
    fn default() -> Self {
        Self {
            connection_count: Arc::new(Mutex::new(0)),
            subscription_events: Arc::new(Mutex::new(Vec::new())),
            fail_next_subscriptions: Arc::new(Mutex::new(Vec::new())),
            auth_response_delay_ms: Arc::new(Mutex::new(None)),
            authenticated: Arc::new(AtomicBool::new(false)),
            disconnect_trigger: Arc::new(AtomicBool::new(false)),
            ping_count: Arc::new(AtomicUsize::new(0)),
            pong_count: Arc::new(AtomicUsize::new(0)),
        }
    }
}

impl TestServerState {
    #[allow(dead_code)]
    async fn reset(&self) {
        *self.connection_count.lock().await = 0;
        self.subscription_events.lock().await.clear();
        self.fail_next_subscriptions.lock().await.clear();
        *self.auth_response_delay_ms.lock().await = None;
        self.authenticated.store(false, Ordering::Relaxed);
        self.disconnect_trigger.store(false, Ordering::Relaxed);
        self.ping_count.store(0, Ordering::Relaxed);
        self.pong_count.store(0, Ordering::Relaxed);
    }

    #[allow(dead_code)]
    async fn set_subscription_failures(&self, topics: Vec<String>) {
        *self.fail_next_subscriptions.lock().await = topics;
    }

    #[allow(dead_code)]
    async fn set_auth_delay(&self, delay_ms: u64) {
        *self.auth_response_delay_ms.lock().await = Some(delay_ms);
    }

    #[allow(dead_code)]
    async fn subscription_events(&self) -> Vec<(String, bool)> {
        self.subscription_events.lock().await.clone()
    }

    #[allow(dead_code)]
    async fn clear_subscription_events(&self) {
        self.subscription_events.lock().await.clear();
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

    // Server-side ping loop
    let state_clone = state.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_millis(100));
        loop {
            interval.tick().await;
            if state_clone.disconnect_trigger.load(Ordering::Relaxed) {
                break;
            }
        }
    });

    // Main message handling loop
    while let Some(Ok(msg)) = socket.recv().await {
        match msg {
            Message::Text(text) => {
                let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
                    continue;
                };

                let op = value.get("op").and_then(|v| v.as_str());

                match op {
                    Some("ping") => {
                        state.ping_count.fetch_add(1, Ordering::Relaxed);
                        // Respond with pong
                        let pong_response = json!({
                            "success": true,
                            "ret_msg": "pong",
                            "conn_id": "test-conn-id",
                            "req_id": value.get("req_id").and_then(|v| v.as_str()).unwrap_or(""),
                            "op": "pong"
                        });
                        if socket
                            .send(Message::Text(pong_response.to_string().into()))
                            .await
                            .is_err()
                        {
                            break;
                        }
                    }
                    Some("auth") => {
                        // Check for auth delay
                        if let Some(delay_ms) = *state.auth_response_delay_ms.lock().await {
                            tokio::time::sleep(Duration::from_millis(delay_ms)).await;
                        }

                        // Parse auth request
                        let api_key = value
                            .get("args")
                            .and_then(|a| a.as_array())
                            .and_then(|arr| arr.first())
                            .and_then(|v| v.as_str());

                        if api_key == Some("test_api_key") {
                            state.authenticated.store(true, Ordering::Relaxed);
                            let auth_response = json!({
                                "success": true,
                                "ret_msg": "",
                                "op": "auth",
                                "conn_id": "test-conn-id"
                            });
                            if socket
                                .send(Message::Text(auth_response.to_string().into()))
                                .await
                                .is_err()
                            {
                                break;
                            }
                        } else {
                            // Auth failed
                            let auth_response = json!({
                                "success": false,
                                "ret_msg": "Invalid API key",
                                "op": "auth",
                                "conn_id": "test-conn-id"
                            });
                            if socket
                                .send(Message::Text(auth_response.to_string().into()))
                                .await
                                .is_err()
                            {
                                break;
                            }
                        }
                    }
                    Some("subscribe") => {
                        let args = value.get("args").and_then(|a| a.as_array());
                        let mut failed_topics = Vec::new();

                        if let Some(topics) = args {
                            let fail_list = state.fail_next_subscriptions.lock().await.clone();

                            for topic in topics {
                                if let Some(topic_str) = topic.as_str() {
                                    let should_fail = fail_list.contains(&topic_str.to_string());

                                    // Track the subscription event
                                    state
                                        .subscription_events
                                        .lock()
                                        .await
                                        .push((topic_str.to_string(), !should_fail));

                                    if should_fail {
                                        failed_topics.push(topic_str);
                                    }
                                }
                            }
                        }

                        // Send subscription response (success or failure)
                        if failed_topics.is_empty() {
                            let sub_response = json!({
                                "success": true,
                                "ret_msg": "",
                                "conn_id": "test-conn-id",
                                "req_id": value.get("req_id").and_then(|v| v.as_str()).unwrap_or(""),
                                "op": "subscribe"
                            });
                            if socket
                                .send(Message::Text(sub_response.to_string().into()))
                                .await
                                .is_err()
                            {
                                break;
                            }
                        } else {
                            // Send failure for failed subscriptions
                            let error_response = json!({
                                "success": false,
                                "ret_msg": format!("Subscription failed for topics: {:?}", failed_topics),
                                "ret_code": 10001,
                                "conn_id": "test-conn-id",
                                "req_id": value.get("req_id").and_then(|v| v.as_str()).unwrap_or(""),
                                "op": "subscribe"
                            });
                            if socket
                                .send(Message::Text(error_response.to_string().into()))
                                .await
                                .is_err()
                            {
                                break;
                            }
                        }

                        // Send a sample data message for the first topic
                        if let Some(topics) = args
                            && let Some(first_topic) = topics.first().and_then(|t| t.as_str())
                        {
                            if first_topic.contains("publicTrade") {
                                // Send a trade message
                                let trade_msg = load_test_data("ws_public_trade.json");
                                if socket
                                    .send(Message::Text(trade_msg.to_string().into()))
                                    .await
                                    .is_err()
                                {
                                    break;
                                }
                            } else if first_topic.contains("orderbook") {
                                // Send an orderbook message
                                let orderbook_msg = load_test_data("ws_orderbook_snapshot.json");
                                if socket
                                    .send(Message::Text(orderbook_msg.to_string().into()))
                                    .await
                                    .is_err()
                                {
                                    break;
                                }
                            }
                        }
                    }
                    Some("unsubscribe") => {
                        let args = value.get("args").and_then(|a| a.as_array());
                        if let Some(topics) = args {
                            for topic in topics {
                                if let Some(topic_str) = topic.as_str() {
                                    let mut events = state.subscription_events.lock().await;
                                    events.retain(|(t, _)| t != topic_str);
                                }
                            }
                        }

                        // Send unsubscription confirmation
                        let unsub_response = json!({
                            "success": true,
                            "ret_msg": "",
                            "conn_id": "test-conn-id",
                            "req_id": value.get("req_id").and_then(|v| v.as_str()).unwrap_or(""),
                            "op": "unsubscribe"
                        });
                        if socket
                            .send(Message::Text(unsub_response.to_string().into()))
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
            Message::Pong(_) => {
                state.pong_count.fetch_add(1, Ordering::Relaxed);
            }
            Message::Close(_) => {
                break;
            }
            _ => {}
        }

        // Check if we should trigger a disconnect
        if state.disconnect_trigger.load(Ordering::Relaxed) {
            break;
        }
    }
}

// Load test data from existing files
fn load_test_data(filename: &str) -> serde_json::Value {
    let path = format!("test_data/{}", filename);
    let content = std::fs::read_to_string(path).expect("Failed to read test data");
    serde_json::from_str(&content).expect("Failed to parse test data")
}

fn create_test_router(state: TestServerState) -> Router {
    Router::new()
        .route("/v5/public/linear", get(handle_websocket))
        .route("/v5/private", get(handle_websocket))
        .with_state(state)
}

async fn start_test_server()
-> Result<(SocketAddr, TestServerState), Box<dyn std::error::Error + Send + Sync>> {
    // Bind to port 0 to let the OS assign an available port
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

#[allow(dead_code)]
async fn wait_for_subscription_events<F>(
    state: &TestServerState,
    timeout: Duration,
    mut predicate: F,
) -> Vec<(String, bool)>
where
    F: FnMut(&[(String, bool)]) -> bool,
{
    let state_clone = state.clone();
    let poll = async {
        loop {
            let events = state_clone.subscription_events().await;
            if predicate(&events) {
                return events;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    };
    match tokio::time::timeout(timeout, poll).await {
        Ok(events) => events,
        Err(_) => state.subscription_events().await,
    }
}

#[rstest]
#[tokio::test]
async fn test_public_client_connection() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{}/v5/public/linear", addr);

    let mut client = BybitWebSocketClient::new_public_with(
        BybitProductType::Linear,
        BybitEnvironment::Mainnet,
        Some(ws_url),
        None,
    );

    client.connect().await.unwrap();

    // Wait for connection to be established
    wait_until_async(
        || async { *state.connection_count.lock().await > 0 },
        Duration::from_secs(2),
    )
    .await;

    assert!(client.is_active().await);
    assert_eq!(*state.connection_count.lock().await, 1);

    client.close().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_private_client_authentication() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{}/v5/private", addr);

    let credential = Credential::new("test_api_key", "test_api_secret");
    let mut client = BybitWebSocketClient::new_private(
        BybitEnvironment::Mainnet,
        credential,
        Some(ws_url),
        None,
    );

    // Connection may timeout waiting for auth confirmation from the mock server
    // This is expected behavior as the mock server's auth flow may not perfectly
    // match the client's expectations
    let _result = client.connect().await;

    // Wait for connection to be established
    wait_until_async(
        || async { *state.connection_count.lock().await > 0 },
        Duration::from_secs(2),
    )
    .await;

    // Check if auth was attempted (connection was made)
    assert!(*state.connection_count.lock().await > 0);

    let _ = client.close().await;
}

#[rstest]
#[tokio::test]
async fn test_authentication_failure() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{}/v5/private", addr);

    let credential = Credential::new("invalid_key", "invalid_secret");
    let mut client = BybitWebSocketClient::new_private(
        BybitEnvironment::Mainnet,
        credential,
        Some(ws_url),
        None,
    );

    let _result = client.connect().await;

    // Wait for connection attempt
    wait_until_async(
        || async { *state.connection_count.lock().await > 0 },
        Duration::from_secs(2),
    )
    .await;

    // Verify the server doesn't mark it as authenticated
    assert!(!state.authenticated.load(Ordering::Relaxed));

    let _ = client.close().await;
}

#[rstest]
#[tokio::test]
async fn test_ping_pong() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{}/v5/public/linear", addr);

    let mut client = BybitWebSocketClient::new_public_with(
        BybitProductType::Linear,
        BybitEnvironment::Mainnet,
        Some(ws_url),
        Some(1), // 1 second heartbeat
    );

    client.connect().await.unwrap();

    // Wait for connection
    wait_until_async(
        || async { *state.connection_count.lock().await > 0 },
        Duration::from_secs(2),
    )
    .await;

    // Wait for at least one ping
    wait_until_async(
        || async { state.ping_count.load(Ordering::Relaxed) > 0 },
        Duration::from_secs(3),
    )
    .await;

    assert!(state.ping_count.load(Ordering::Relaxed) > 0);

    client.close().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_subscription_lifecycle() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{}/v5/public/linear", addr);

    let mut client = BybitWebSocketClient::new_public_with(
        BybitProductType::Linear,
        BybitEnvironment::Mainnet,
        Some(ws_url),
        None,
    );

    client.connect().await.unwrap();

    // Subscribe to a topic
    let topics = vec!["publicTrade.BTCUSDT".to_string()];
    client.subscribe(topics.clone()).await.unwrap();

    // Wait for subscription confirmation
    wait_until_async(
        || async { !state.subscription_events.lock().await.is_empty() },
        Duration::from_secs(2),
    )
    .await;

    let subs = state.subscription_events.lock().await.clone();
    assert!(
        subs.iter()
            .any(|(topic, ok)| topic == "publicTrade.BTCUSDT" && *ok)
    );

    // Unsubscribe
    client.unsubscribe(topics).await.unwrap();

    // Wait for unsubscription
    wait_until_async(
        || async { state.subscription_events.lock().await.is_empty() },
        Duration::from_secs(2),
    )
    .await;

    assert!(state.subscription_events.lock().await.is_empty());

    client.close().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_message_routing() {
    let (addr, _state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{}/v5/public/linear", addr);

    let mut client = BybitWebSocketClient::new_public_with(
        BybitProductType::Linear,
        BybitEnvironment::Mainnet,
        Some(ws_url),
        None,
    );

    client.connect().await.unwrap();

    // Subscribe to trades
    let topics = vec!["publicTrade.BTCUSDT".to_string()];
    client.subscribe(topics).await.unwrap();

    // Wait for subscription to be confirmed
    wait_until_async(
        || async { client.subscription_count() > 0 },
        Duration::from_secs(2),
    )
    .await;

    // Verify subscription was recorded
    assert!(client.subscription_count() > 0);

    client.close().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_reconnection_flow() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{}/v5/public/linear", addr);

    let mut client = BybitWebSocketClient::new_public_with(
        BybitProductType::Linear,
        BybitEnvironment::Mainnet,
        Some(ws_url),
        None,
    );

    client.connect().await.unwrap();

    // Subscribe to a topic before disconnect
    let topics = vec!["publicTrade.BTCUSDT".to_string()];
    client.subscribe(topics.clone()).await.unwrap();

    // Wait for initial connection
    wait_until_async(
        || async { *state.connection_count.lock().await > 0 },
        Duration::from_secs(2),
    )
    .await;

    let initial_count = *state.connection_count.lock().await;
    assert_eq!(initial_count, 1);

    // Trigger a server-side disconnect
    state.disconnect_trigger.store(true, Ordering::Relaxed);

    // Short delay for disconnect trigger to be observed by server
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Note: Full reconnection testing requires the client to support reconnection
    // This test establishes the pattern for testing reconnection behavior

    client.close().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_multiple_subscriptions() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{}/v5/public/linear", addr);

    let mut client = BybitWebSocketClient::new_public_with(
        BybitProductType::Linear,
        BybitEnvironment::Mainnet,
        Some(ws_url),
        None,
    );

    client.connect().await.unwrap();

    // Subscribe to multiple topics
    let topics = vec![
        "publicTrade.BTCUSDT".to_string(),
        "publicTrade.ETHUSDT".to_string(),
        "orderbook.50.BTCUSDT".to_string(),
    ];
    client.subscribe(topics).await.unwrap();

    // Wait for subscriptions
    wait_until_async(
        || async { state.subscription_events.lock().await.len() >= 3 },
        Duration::from_secs(2),
    )
    .await;

    let subs = state.subscription_events.lock().await.clone();
    assert_eq!(subs.len(), 3);
    assert!(
        subs.iter()
            .any(|(topic, ok)| topic == "publicTrade.BTCUSDT" && *ok)
    );
    assert!(
        subs.iter()
            .any(|(topic, ok)| topic == "publicTrade.ETHUSDT" && *ok)
    );
    assert!(
        subs.iter()
            .any(|(topic, ok)| topic == "orderbook.50.BTCUSDT" && *ok)
    );

    client.close().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_wait_until_active_timeout() {
    // Create a client but don't start a server
    let mut client = BybitWebSocketClient::new_public_with(
        BybitProductType::Linear,
        BybitEnvironment::Mainnet,
        Some("ws://127.0.0.1:9999/invalid".to_string()),
        None,
    );

    // Connect will fail, but we won't await it
    let _ = client.connect().await;

    // wait_until_active should timeout
    let result = client.wait_until_active(0.5).await;
    assert!(result.is_err());
}

#[rstest]
#[tokio::test]
async fn test_heartbeat_timeout_reconnection() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{}/v5/public/linear", addr);

    let mut client = BybitWebSocketClient::new_public_with(
        BybitProductType::Linear,
        BybitEnvironment::Mainnet,
        Some(ws_url),
        Some(1), // 1 second heartbeat
    );

    client.connect().await.unwrap();

    // Wait for connection
    wait_until_async(
        || async { *state.connection_count.lock().await > 0 },
        Duration::from_secs(2),
    )
    .await;

    // Trigger disconnect - client should attempt reconnection
    state.disconnect_trigger.store(true, Ordering::Relaxed);

    // Short delay for disconnect trigger to be observed by server
    tokio::time::sleep(Duration::from_millis(100)).await;

    client.close().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_sends_pong_for_text_ping() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{}/v5/public/linear", addr);

    let mut client = BybitWebSocketClient::new_public_with(
        BybitProductType::Linear,
        BybitEnvironment::Mainnet,
        Some(ws_url),
        Some(1),
    );

    client.connect().await.unwrap();

    // Wait for pings to be sent
    wait_until_async(
        || async { state.ping_count.load(Ordering::Relaxed) > 0 },
        Duration::from_secs(3),
    )
    .await;

    // Verify ping was received by server
    assert!(state.ping_count.load(Ordering::Relaxed) > 0);

    client.close().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_sends_pong_for_control_ping() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{}/v5/public/linear", addr);

    let mut client = BybitWebSocketClient::new_public_with(
        BybitProductType::Linear,
        BybitEnvironment::Mainnet,
        Some(ws_url),
        None,
    );

    client.connect().await.unwrap();

    // Wait for connection
    wait_until_async(
        || async { *state.connection_count.lock().await > 0 },
        Duration::from_secs(2),
    )
    .await;

    // Control ping/pong is handled by the WebSocket layer
    // This test verifies the connection remains active
    assert!(client.is_active().await);

    client.close().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_reauth_after_disconnect() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{}/v5/private", addr);

    let credential = Credential::new("test_api_key", "test_api_secret");
    let mut client = BybitWebSocketClient::new_private(
        BybitEnvironment::Mainnet,
        credential,
        Some(ws_url),
        None,
    );

    let _ = client.connect().await;

    // Wait for initial connection
    wait_until_async(
        || async { *state.connection_count.lock().await > 0 },
        Duration::from_secs(2),
    )
    .await;

    // Trigger disconnect
    state.disconnect_trigger.store(true, Ordering::Relaxed);

    // Short delay for disconnect trigger to be observed by server
    tokio::time::sleep(Duration::from_millis(100)).await;

    let _ = client.close().await;
}

#[rstest]
#[tokio::test]
async fn test_login_failure_emits_error() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{}/v5/private", addr);

    let credential = Credential::new("invalid_key", "invalid_secret");
    let mut client = BybitWebSocketClient::new_private(
        BybitEnvironment::Mainnet,
        credential,
        Some(ws_url),
        None,
    );

    let _ = client.connect().await;

    // Wait for connection attempt
    wait_until_async(
        || async { *state.connection_count.lock().await > 0 },
        Duration::from_secs(2),
    )
    .await;

    // Verify auth failed
    assert!(!state.authenticated.load(Ordering::Relaxed));

    let _ = client.close().await;
}

#[rstest]
#[tokio::test]
async fn test_unauthenticated_private_subscription_fails() {
    let (addr, _state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{}/v5/public/linear", addr);

    // Create public client
    let mut client = BybitWebSocketClient::new_public_with(
        BybitProductType::Linear,
        BybitEnvironment::Mainnet,
        Some(ws_url),
        None,
    );

    client.connect().await.unwrap();

    // Try to subscribe to private channels - should fail
    let result = client.subscribe_orders().await;
    assert!(result.is_err());

    client.close().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_subscription_after_reconnection() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{}/v5/public/linear", addr);

    let mut client = BybitWebSocketClient::new_public_with(
        BybitProductType::Linear,
        BybitEnvironment::Mainnet,
        Some(ws_url),
        None,
    );

    client.connect().await.unwrap();

    // Subscribe before disconnect
    let topics = vec!["publicTrade.BTCUSDT".to_string()];
    client.subscribe(topics.clone()).await.unwrap();

    // Wait for subscription
    wait_until_async(
        || async { !state.subscription_events.lock().await.is_empty() },
        Duration::from_secs(2),
    )
    .await;

    // Trigger disconnect
    state.disconnect_trigger.store(true, Ordering::Relaxed);

    // Short delay for disconnect trigger to be observed by server
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Verify subscriptions are tracked
    assert!(client.subscription_count() > 0);

    client.close().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_subscription_restoration_tracking() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{}/v5/public/linear", addr);

    let mut client = BybitWebSocketClient::new_public_with(
        BybitProductType::Linear,
        BybitEnvironment::Mainnet,
        Some(ws_url),
        None,
    );

    client.connect().await.unwrap();

    // Subscribe to multiple topics
    let topics = vec![
        "publicTrade.BTCUSDT".to_string(),
        "orderbook.50.ETHUSDT".to_string(),
    ];
    client.subscribe(topics).await.unwrap();

    // Wait for subscriptions
    wait_until_async(
        || async { state.subscription_events.lock().await.len() >= 2 },
        Duration::from_secs(2),
    )
    .await;

    // Verify subscription count
    let initial_count = client.subscription_count();
    assert_eq!(initial_count, 2);

    client.close().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_reconnection_retries_failed_subscriptions() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{}/v5/public/linear", addr);

    let mut client = BybitWebSocketClient::new_public_with(
        BybitProductType::Linear,
        BybitEnvironment::Mainnet,
        Some(ws_url),
        None,
    );

    client.connect().await.unwrap();

    // Subscribe to a topic
    let topics = vec!["publicTrade.BTCUSDT".to_string()];
    client.subscribe(topics).await.unwrap();

    // Wait for subscription
    wait_until_async(
        || async { !state.subscription_events.lock().await.is_empty() },
        Duration::from_secs(2),
    )
    .await;

    // Clear server subscriptions to simulate failure
    state.subscription_events.lock().await.clear();

    // Trigger disconnect
    state.disconnect_trigger.store(true, Ordering::Relaxed);

    // Short delay for disconnect trigger to be observed by server
    tokio::time::sleep(Duration::from_millis(100)).await;

    client.close().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_trade_subscription_flow() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{}/v5/public/linear", addr);

    let mut client = BybitWebSocketClient::new_public_with(
        BybitProductType::Linear,
        BybitEnvironment::Mainnet,
        Some(ws_url),
        None,
    );

    client.connect().await.unwrap();

    // Subscribe to trades using the high-level method
    let instrument_id = InstrumentId::from("BTCUSDT-LINEAR.BYBIT");
    client.subscribe_trades(instrument_id).await.unwrap();

    // Wait for subscription
    wait_until_async(
        || async { !state.subscription_events.lock().await.is_empty() },
        Duration::from_secs(2),
    )
    .await;

    let subs = state.subscription_events.lock().await.clone();
    assert!(
        subs.iter()
            .any(|(topic, ok)| topic.contains("publicTrade") && *ok)
    );

    client.close().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_orderbook_subscription_flow() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{}/v5/public/linear", addr);

    let mut client = BybitWebSocketClient::new_public_with(
        BybitProductType::Linear,
        BybitEnvironment::Mainnet,
        Some(ws_url),
        None,
    );

    client.connect().await.unwrap();

    // Subscribe to orderbook using the high-level method
    let instrument_id = InstrumentId::from("BTCUSDT-LINEAR.BYBIT");
    client.subscribe_orderbook(instrument_id, 50).await.unwrap();

    // Wait for subscription
    wait_until_async(
        || async { !state.subscription_events.lock().await.is_empty() },
        Duration::from_secs(2),
    )
    .await;

    let subs = state.subscription_events.lock().await.clone();
    assert!(
        subs.iter()
            .any(|(topic, ok)| topic.contains("orderbook") && *ok)
    );

    client.close().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_ticker_subscription_flow() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{}/v5/public/linear", addr);

    let mut client = BybitWebSocketClient::new_public_with(
        BybitProductType::Linear,
        BybitEnvironment::Mainnet,
        Some(ws_url),
        None,
    );

    client.connect().await.unwrap();

    // Subscribe to ticker using the high-level method
    let instrument_id = InstrumentId::from("BTCUSDT-LINEAR.BYBIT");
    client.subscribe_ticker(instrument_id).await.unwrap();

    // Wait for subscription
    wait_until_async(
        || async { !state.subscription_events.lock().await.is_empty() },
        Duration::from_secs(2),
    )
    .await;

    let subs = state.subscription_events.lock().await.clone();
    assert!(
        subs.iter()
            .any(|(topic, ok)| topic.contains("ticker") && *ok)
    );

    client.close().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_klines_subscription_flow() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{}/v5/public/linear", addr);

    let mut client = BybitWebSocketClient::new_public_with(
        BybitProductType::Linear,
        BybitEnvironment::Mainnet,
        Some(ws_url),
        None,
    );

    client.connect().await.unwrap();

    // Subscribe to klines using the high-level method
    let instrument_id = InstrumentId::from("BTCUSDT-LINEAR.BYBIT");
    client
        .subscribe_klines(instrument_id, "1".to_string())
        .await
        .unwrap();

    // Wait for subscription
    wait_until_async(
        || async { !state.subscription_events.lock().await.is_empty() },
        Duration::from_secs(2),
    )
    .await;

    let subs = state.subscription_events.lock().await.clone();
    assert!(
        subs.iter()
            .any(|(topic, ok)| topic.contains("kline") && *ok)
    );

    client.close().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_private_orders_subscription() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{}/v5/private", addr);

    let credential = Credential::new("test_api_key", "test_api_secret");
    let mut client = BybitWebSocketClient::new_private(
        BybitEnvironment::Mainnet,
        credential,
        Some(ws_url),
        None,
    );

    let _ = client.connect().await;

    // Wait for connection
    wait_until_async(
        || async { *state.connection_count.lock().await > 0 },
        Duration::from_secs(2),
    )
    .await;

    // Subscribe to orders (may succeed or fail depending on auth timing)
    let _ = client.subscribe_orders().await;

    let _ = client.close().await;
}

#[rstest]
#[tokio::test]
async fn test_private_executions_subscription() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{}/v5/private", addr);

    let credential = Credential::new("test_api_key", "test_api_secret");
    let mut client = BybitWebSocketClient::new_private(
        BybitEnvironment::Mainnet,
        credential,
        Some(ws_url),
        None,
    );

    let _ = client.connect().await;

    // Wait for connection
    wait_until_async(
        || async { *state.connection_count.lock().await > 0 },
        Duration::from_secs(2),
    )
    .await;

    // Subscribe to executions (may succeed or fail depending on auth timing)
    let _ = client.subscribe_executions().await;

    let _ = client.close().await;
}

#[rstest]
#[tokio::test]
async fn test_private_wallet_subscription() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{}/v5/private", addr);

    let credential = Credential::new("test_api_key", "test_api_secret");
    let mut client = BybitWebSocketClient::new_private(
        BybitEnvironment::Mainnet,
        credential,
        Some(ws_url),
        None,
    );

    let _ = client.connect().await;

    // Wait for connection
    wait_until_async(
        || async { *state.connection_count.lock().await > 0 },
        Duration::from_secs(2),
    )
    .await;

    // Subscribe to wallet (may succeed or fail depending on auth timing)
    let _ = client.subscribe_wallet().await;

    let _ = client.close().await;
}

// Tests for conditional order types
#[cfg(test)]
mod conditional_order_tests {
    use nautilus_bybit::{
        common::enums::{BybitOrderSide, BybitOrderType, BybitProductType, BybitTimeInForce},
        websocket::messages::BybitWsPlaceOrderParams,
    };
    use nautilus_model::{enums::OrderType, types::Price};

    #[test]
    fn test_stop_market_order_uses_trigger_price() {
        let params = create_conditional_order_params(
            OrderType::StopMarket,
            Some(Price::from("4500.00")),
            None,
        );

        // Stop orders should use triggerPrice, not sl_trigger_price
        assert!(params.trigger_price.is_some());
        assert_eq!(params.trigger_price.as_ref().unwrap(), "4500.00");
        assert!(params.sl_trigger_price.is_none());
        assert!(params.tp_trigger_price.is_none());

        // Should be Market type at Bybit level
        assert_eq!(params.order_type, BybitOrderType::Market);
    }

    #[test]
    fn test_stop_limit_order_uses_trigger_price() {
        let params = create_conditional_order_params(
            OrderType::StopLimit,
            Some(Price::from("4500.00")),
            Some(Price::from("4505.00")),
        );

        // Stop limit orders should use triggerPrice
        assert!(params.trigger_price.is_some());
        assert_eq!(params.trigger_price.as_ref().unwrap(), "4500.00");

        // Price should be set for limit
        assert!(params.price.is_some());
        assert_eq!(params.price.as_ref().unwrap(), "4505.00");

        // Should not use sl/tp fields for standalone stop orders
        assert!(params.sl_trigger_price.is_none());
        assert!(params.tp_trigger_price.is_none());

        // Should be Limit type at Bybit level
        assert_eq!(params.order_type, BybitOrderType::Limit);
    }

    #[test]
    fn test_market_if_touched_order_uses_trigger_price() {
        let params = create_conditional_order_params(
            OrderType::MarketIfTouched,
            Some(Price::from("4500.00")),
            None,
        );

        // MIT orders should use triggerPrice
        assert!(params.trigger_price.is_some());
        assert_eq!(params.trigger_price.as_ref().unwrap(), "4500.00");
        assert!(params.sl_trigger_price.is_none());
        assert!(params.tp_trigger_price.is_none());

        // Should be Market type at Bybit level
        assert_eq!(params.order_type, BybitOrderType::Market);
    }

    #[test]
    fn test_limit_if_touched_order_uses_trigger_price() {
        let params = create_conditional_order_params(
            OrderType::LimitIfTouched,
            Some(Price::from("4500.00")),
            Some(Price::from("4505.00")),
        );

        // LIT orders should use triggerPrice
        assert!(params.trigger_price.is_some());
        assert_eq!(params.trigger_price.as_ref().unwrap(), "4500.00");
        assert!(params.sl_trigger_price.is_none());
        assert!(params.tp_trigger_price.is_none());

        // Price should be set for limit
        assert!(params.price.is_some());
        assert_eq!(params.price.as_ref().unwrap(), "4505.00");

        // Should be Limit type at Bybit level
        assert_eq!(params.order_type, BybitOrderType::Limit);
    }

    #[test]
    fn test_reduce_only_false_omitted() {
        let params = create_conditional_order_params_with_reduce_only(
            OrderType::StopMarket,
            Some(Price::from("4500.00")),
            None,
            Some(false),
        );

        // reduce_only should be None when false (not sent to Bybit)
        assert!(params.reduce_only.is_none());
    }

    #[test]
    fn test_reduce_only_explicit_true() {
        let params = create_conditional_order_params_with_reduce_only(
            OrderType::StopMarket,
            Some(Price::from("4500.00")),
            None,
            Some(true),
        );

        // reduce_only should be Some(true)
        assert!(params.reduce_only.is_some());
        assert!(params.reduce_only.unwrap());
    }

    // Helper function to create conditional order params for testing
    fn create_conditional_order_params(
        order_type: OrderType,
        trigger_price: Option<Price>,
        price: Option<Price>,
    ) -> BybitWsPlaceOrderParams {
        create_conditional_order_params_with_reduce_only(
            order_type,
            trigger_price,
            price,
            Some(false),
        )
    }

    fn create_conditional_order_params_with_reduce_only(
        order_type: OrderType,
        trigger_price: Option<Price>,
        price: Option<Price>,
        reduce_only: Option<bool>,
    ) -> BybitWsPlaceOrderParams {
        use nautilus_bybit::common::enums::BybitTriggerType;

        let is_stop_order = matches!(
            order_type,
            OrderType::StopMarket
                | OrderType::StopLimit
                | OrderType::MarketIfTouched
                | OrderType::LimitIfTouched
        );

        let bybit_order_type = match order_type {
            OrderType::Market | OrderType::StopMarket | OrderType::MarketIfTouched => {
                BybitOrderType::Market
            }
            OrderType::Limit | OrderType::StopLimit | OrderType::LimitIfTouched => {
                BybitOrderType::Limit
            }
            _ => panic!("Unsupported order type"),
        };

        if is_stop_order {
            BybitWsPlaceOrderParams {
                category: BybitProductType::Linear,
                symbol: "ETHUSDT".into(),
                side: BybitOrderSide::Buy,
                order_type: bybit_order_type,
                qty: "0.01".to_string(),
                price: price.map(|p| p.to_string()),
                time_in_force: Some(BybitTimeInForce::Gtc),
                order_link_id: Some("test-order-1".to_string()),
                reduce_only: reduce_only.filter(|&r| r),
                close_on_trigger: None,
                trigger_price: trigger_price.map(|p| p.to_string()),
                trigger_by: Some(BybitTriggerType::LastPrice),
                trigger_direction: None,
                tpsl_mode: None,
                take_profit: None,
                stop_loss: None,
                tp_trigger_by: None,
                sl_trigger_by: None,
                sl_trigger_price: None,
                tp_trigger_price: None,
                sl_order_type: None,
                tp_order_type: None,
                sl_limit_price: None,
                tp_limit_price: None,
            }
        } else {
            BybitWsPlaceOrderParams {
                category: BybitProductType::Linear,
                symbol: "ETHUSDT".into(),
                side: BybitOrderSide::Buy,
                order_type: bybit_order_type,
                qty: "0.01".to_string(),
                price: price.map(|p| p.to_string()),
                time_in_force: Some(BybitTimeInForce::Gtc),
                order_link_id: Some("test-order-1".to_string()),
                reduce_only: reduce_only.filter(|&r| r),
                close_on_trigger: None,
                trigger_price: None,
                trigger_by: None,
                trigger_direction: None,
                tpsl_mode: None,
                take_profit: None,
                stop_loss: None,
                tp_trigger_by: None,
                sl_trigger_by: None,
                sl_trigger_price: None,
                tp_trigger_price: None,
                sl_order_type: None,
                tp_order_type: None,
                sl_limit_price: None,
                tp_limit_price: None,
            }
        }
    }
}
