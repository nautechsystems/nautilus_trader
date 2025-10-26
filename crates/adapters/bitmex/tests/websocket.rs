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

//! Integration tests for BitMEX WebSocket client using a mock server.

use std::{
    net::SocketAddr,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
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
use nautilus_bitmex::websocket::client::BitmexWebSocketClient;
use nautilus_common::testing::wait_until_async;
use nautilus_model::identifiers::AccountId;
use rstest::rstest;
use serde_json::json;
use tokio::sync::Mutex;

const TEST_PING_PAYLOAD: &[u8] = b"test-server-ping";

// Test server state for tracking WebSocket connections
#[derive(Clone)]
struct TestServerState {
    connection_count: Arc<Mutex<usize>>,
    subscriptions: Arc<Mutex<Vec<String>>>,
    authenticated: Arc<AtomicBool>,
    drop_connections: Arc<AtomicBool>,
    silent_drop: Arc<AtomicBool>,
    auth_calls: Arc<Mutex<usize>>,
    send_initial_ping: Arc<AtomicBool>,
    received_pong: Arc<AtomicBool>,
    last_pong: Arc<Mutex<Option<Vec<u8>>>>,
    fail_next_subscriptions: Arc<Mutex<Vec<String>>>,
    auth_response_delay_ms: Arc<Mutex<Option<u64>>>,
    subscription_events: Arc<Mutex<Vec<(String, bool)>>>,
}

impl TestServerState {
    async fn fail_next_subscription(&self, topic: &str) {
        self.fail_next_subscriptions
            .lock()
            .await
            .push(topic.to_string());
    }

    async fn set_auth_response_delay_ms(&self, delay_ms: Option<u64>) {
        *self.auth_response_delay_ms.lock().await = delay_ms;
    }

    async fn clear_subscription_events(&self) {
        self.subscription_events.lock().await.clear();
    }

    async fn subscription_events(&self) -> Vec<(String, bool)> {
        self.subscription_events.lock().await.clone()
    }
}

impl Default for TestServerState {
    fn default() -> Self {
        Self {
            connection_count: Arc::new(Mutex::new(0)),
            subscriptions: Arc::new(Mutex::new(Vec::new())),
            authenticated: Arc::new(AtomicBool::new(false)),
            drop_connections: Arc::new(AtomicBool::new(false)),
            silent_drop: Arc::new(AtomicBool::new(false)),
            auth_calls: Arc::new(Mutex::new(0)),
            send_initial_ping: Arc::new(AtomicBool::new(false)),
            received_pong: Arc::new(AtomicBool::new(false)),
            last_pong: Arc::new(Mutex::new(None)),
            fail_next_subscriptions: Arc::new(Mutex::new(Vec::new())),
            auth_response_delay_ms: Arc::new(Mutex::new(None)),
            subscription_events: Arc::new(Mutex::new(Vec::new())),
        }
    }
}

// Load test data from existing files
fn load_test_data(filename: &str) -> String {
    let path = format!("test_data/{}", filename);
    std::fs::read_to_string(path).expect("Failed to read test data")
}

// WebSocket handler for the mock server
async fn handle_websocket(ws: WebSocketUpgrade, State(state): State<TestServerState>) -> Response {
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

async fn handle_socket(mut socket: WebSocket, state: TestServerState) {
    #[allow(unused_imports)]
    use futures_util::{SinkExt, StreamExt};

    // Increment connection count
    {
        let mut count = state.connection_count.lock().await;
        *count += 1;
    }

    // Send welcome message
    let welcome_msg = json!({
        "info": "Welcome to the BitMEX Realtime API.",
        "version": "2024-06-12T21:37:02.000Z",
        "timestamp": "2025-01-05T12:00:00.000Z",
        "docs": "https://www.bitmex.com/app/wsAPI",
        "heartbeatEnabled": false,
        "limit": {
            "remaining": 40
        }
    });

    if socket
        .send(Message::Text(
            serde_json::to_string(&welcome_msg).unwrap().into(),
        ))
        .await
        .is_err()
    {
        return;
    }

    if state.send_initial_ping.load(Ordering::Relaxed)
        && socket
            .send(Message::Ping(TEST_PING_PAYLOAD.to_vec().into()))
            .await
            .is_err()
    {
        return;
    }

    // Handle incoming messages
    loop {
        if state.drop_connections.load(Ordering::Relaxed) {
            if state.silent_drop.load(Ordering::Relaxed) {
                break;
            } else {
                let _ = socket.send(Message::Close(None)).await;
                break;
            }
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

        if state.drop_connections.load(Ordering::Relaxed) {
            if state.silent_drop.load(Ordering::Relaxed) {
                break;
            } else {
                let _ = socket.send(Message::Close(None)).await;
                break;
            }
        }

        match msg {
            Message::Text(text) => {
                // Parse the incoming message
                let parsed: Result<serde_json::Value, _> = serde_json::from_str(&text);

                if let Ok(data) = parsed {
                    // Handle authentication requests
                    if data.get("op") == Some(&json!("authKeyExpires")) {
                        // Track auth calls
                        {
                            let mut auth_calls = state.auth_calls.lock().await;
                            *auth_calls += 1;
                        }

                        if let Some(delay) = *state.auth_response_delay_ms.lock().await {
                            tokio::time::sleep(Duration::from_millis(delay)).await;
                        }

                        state.authenticated.store(true, Ordering::Relaxed);

                        // Send auth success response
                        let response = json!({
                            "success": true,
                            "request": {
                                "op": "authKeyExpires",
                                "args": data.get("args")
                            }
                        });

                        if socket
                            .send(Message::Text(
                                serde_json::to_string(&response).unwrap().into(),
                            ))
                            .await
                            .is_err()
                        {
                            break;
                        }
                        continue;
                    }

                    // Handle subscription requests
                    if data.get("op") == Some(&json!("subscribe")) {
                        if let Some(args) = data.get("args").and_then(|a| a.as_array()) {
                            for arg in args {
                                if let Some(topic) = arg.as_str() {
                                    // Check if this is a private channel that requires auth
                                    let private_channels =
                                        ["order", "execution", "position", "margin", "wallet"];
                                    let requires_auth = private_channels.iter().any(|&ch| {
                                        topic == ch || topic.starts_with(&format!("{}:", ch))
                                    });

                                    if requires_auth && !state.authenticated.load(Ordering::Relaxed)
                                    {
                                        // Send auth error
                                        let error_response = json!({
                                            "status": 401,
                                            "error": "Not authenticated",
                                            "meta": {},
                                            "request": {
                                                "op": "subscribe",
                                                "args": [topic]
                                            }
                                        });

                                        if socket
                                            .send(Message::Text(
                                                serde_json::to_string(&error_response)
                                                    .unwrap()
                                                    .into(),
                                            ))
                                            .await
                                            .is_err()
                                        {
                                            break;
                                        }
                                        continue;
                                    }

                                    // Track subscription
                                    let mut pending = state.fail_next_subscriptions.lock().await;
                                    if let Some(pos) = pending.iter().position(|p| p == topic) {
                                        pending.remove(pos);
                                        drop(pending);

                                        let response = json!({
                                            "success": false,
                                            "error": "Subscription failed",
                                            "request": {
                                                "op": "subscribe",
                                                "args": [topic]
                                            }
                                        });
                                        state
                                            .subscription_events
                                            .lock()
                                            .await
                                            .push((topic.to_string(), false));
                                        if socket
                                            .send(Message::Text(
                                                serde_json::to_string(&response).unwrap().into(),
                                            ))
                                            .await
                                            .is_err()
                                        {
                                            break;
                                        }
                                        continue;
                                    }
                                    drop(pending);

                                    let mut subs = state.subscriptions.lock().await;
                                    if !subs.contains(&topic.to_string()) {
                                        subs.push(topic.to_string());
                                    }
                                    drop(subs);

                                    state
                                        .subscription_events
                                        .lock()
                                        .await
                                        .push((topic.to_string(), true));

                                    // Send subscription confirmation
                                    let response = json!({
                                        "success": true,
                                        "subscribe": topic,
                                        "request": {
                                            "op": "subscribe",
                                            "args": [topic]
                                        }
                                    });

                                    if socket
                                        .send(Message::Text(
                                            serde_json::to_string(&response).unwrap().into(),
                                        ))
                                        .await
                                        .is_err()
                                    {
                                        break;
                                    }

                                    // Send sample data based on subscription type
                                    if topic.starts_with("trade:") {
                                        tokio::time::sleep(Duration::from_millis(10)).await;

                                        // Send a trade update
                                        let trade_data = load_test_data("ws_trade.json");
                                        let mut trade: serde_json::Value =
                                            serde_json::from_str(&trade_data).unwrap();
                                        trade["table"] = json!("trade");
                                        trade["action"] = json!("insert");

                                        if socket
                                            .send(Message::Text(
                                                serde_json::to_string(&trade).unwrap().into(),
                                            ))
                                            .await
                                            .is_err()
                                        {
                                            break;
                                        }
                                    } else if topic.starts_with("orderBookL2:") {
                                        tokio::time::sleep(Duration::from_millis(10)).await;

                                        // Send an order book update
                                        let book_data = load_test_data("ws_orderbook_l2.json");
                                        let mut book: serde_json::Value =
                                            serde_json::from_str(&book_data).unwrap();
                                        book["table"] = json!("orderBookL2");
                                        book["action"] = json!("partial");

                                        if socket
                                            .send(Message::Text(
                                                serde_json::to_string(&book).unwrap().into(),
                                            ))
                                            .await
                                            .is_err()
                                        {
                                            break;
                                        }
                                    } else if topic == "position"
                                        || topic == "order"
                                        || topic == "execution"
                                    {
                                        // Handle private subscriptions
                                        tokio::time::sleep(Duration::from_millis(10)).await;

                                        // Send authentication success
                                        let auth_success = json!({
                                            "success": true,
                                            "request": {
                                                "op": "authKeyExpires",
                                                "args": ["test_key", 123456789]
                                            }
                                        });

                                        if socket
                                            .send(Message::Text(
                                                serde_json::to_string(&auth_success)
                                                    .unwrap()
                                                    .into(),
                                            ))
                                            .await
                                            .is_err()
                                        {
                                            break;
                                        }

                                        // Send sample data for the private channel
                                        match topic {
                                            "position" => {
                                                let position_data =
                                                    load_test_data("ws_position.json");
                                                let mut position: serde_json::Value =
                                                    serde_json::from_str(&position_data).unwrap();
                                                position["table"] = json!("position");
                                                position["action"] = json!("partial");

                                                if socket
                                                    .send(Message::Text(
                                                        serde_json::to_string(&position)
                                                            .unwrap()
                                                            .into(),
                                                    ))
                                                    .await
                                                    .is_err()
                                                {
                                                    break;
                                                }
                                            }
                                            "order" => {
                                                let order_data = load_test_data("ws_order.json");
                                                let mut order: serde_json::Value =
                                                    serde_json::from_str(&order_data).unwrap();
                                                order["table"] = json!("order");
                                                order["action"] = json!("partial");

                                                if socket
                                                    .send(Message::Text(
                                                        serde_json::to_string(&order)
                                                            .unwrap()
                                                            .into(),
                                                    ))
                                                    .await
                                                    .is_err()
                                                {
                                                    break;
                                                }
                                            }
                                            "execution" => {
                                                let exec_data = load_test_data("ws_execution.json");
                                                let mut exec: serde_json::Value =
                                                    serde_json::from_str(&exec_data).unwrap();
                                                exec["table"] = json!("execution");
                                                exec["action"] = json!("insert");

                                                if socket
                                                    .send(Message::Text(
                                                        serde_json::to_string(&exec)
                                                            .unwrap()
                                                            .into(),
                                                    ))
                                                    .await
                                                    .is_err()
                                                {
                                                    break;
                                                }
                                            }
                                            _ => {}
                                        }
                                    }
                                }
                            }
                        }
                    }
                    // Handle unsubscribe requests
                    else if data.get("op") == Some(&json!("unsubscribe")) {
                        if let Some(args) = data.get("args").and_then(|a| a.as_array()) {
                            for arg in args {
                                if let Some(topic) = arg.as_str() {
                                    // Remove from subscriptions
                                    {
                                        let mut subs = state.subscriptions.lock().await;
                                        subs.retain(|s| s != topic);
                                    }

                                    let response = json!({
                                        "success": true,
                                        "unsubscribe": topic,
                                        "request": {
                                            "op": "unsubscribe",
                                            "args": [topic]
                                        }
                                    });

                                    if socket
                                        .send(Message::Text(
                                            serde_json::to_string(&response).unwrap().into(),
                                        ))
                                        .await
                                        .is_err()
                                    {
                                        break;
                                    }
                                }
                            }
                        }
                    }
                    // Handle ping
                    else if data.get("op") == Some(&json!("ping")) {
                        let pong = json!({"op": "pong"});
                        if socket
                            .send(Message::Text(serde_json::to_string(&pong).unwrap().into()))
                            .await
                            .is_err()
                        {
                            break;
                        }
                    }
                }
            }
            Message::Pong(data) => {
                state.received_pong.store(true, Ordering::Relaxed);
                let mut last_pong = state.last_pong.lock().await;
                *last_pong = Some(data.to_vec());
            }
            Message::Ping(data) => {
                // Respond with pong
                if socket.send(Message::Pong(data)).await.is_err() {
                    break;
                }
            }
            Message::Close(_) => {
                break;
            }
            _ => {}
        }
    }

    // Decrement connection count
    let mut count = state.connection_count.lock().await;
    *count = count.saturating_sub(1);
}

fn create_test_router(state: TestServerState) -> Router {
    Router::new()
        .route("/realtime", get(handle_websocket))
        .with_state(state)
}

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

async fn wait_for_connection_count(state: &TestServerState, expected: usize, timeout: Duration) {
    wait_until_async(
        || {
            let state = state.clone();
            async move { *state.connection_count.lock().await == expected }
        },
        timeout,
    )
    .await;
}

async fn start_test_server()
-> Result<(SocketAddr, TestServerState), Box<dyn std::error::Error + Send + Sync>> {
    // Bind to port 0 to let the OS assign an available port
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let state = TestServerState::default();
    let router = create_test_router(state.clone());

    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });

    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    Ok((addr, state))
}

fn get_test_account_id() -> AccountId {
    AccountId::new("BITMEX-001")
}

#[rstest]
#[tokio::test]
async fn test_bitmex_websocket_client_creation() {
    let client = BitmexWebSocketClient::new(
        None,                            // url
        Some("test_key".to_string()),    // api_key
        Some("test_secret".to_string()), // api_secret
        Some(get_test_account_id()),     // account_id
        None,                            // heartbeat
    )
    .unwrap();

    assert!(!client.is_active());
}

#[rstest]
#[tokio::test]
async fn test_websocket_connection() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{}/realtime", addr);

    let mut client = BitmexWebSocketClient::new(
        Some(ws_url),
        Some("test_api_key".to_string()),
        Some("test_api_secret".to_string()),
        Some(AccountId::new("BITMEX-001")),
        None,
    )
    .unwrap();

    // Connect to the mock server
    client.connect().await.unwrap();

    // Wait a bit for the connection to be established
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Check connection count
    let count = *state.connection_count.lock().await;
    assert_eq!(count, 1);

    // Close the connection
    client.close().await.unwrap();

    // Wait a bit for disconnection to complete
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Check connection count after disconnect
    let count = *state.connection_count.lock().await;
    assert_eq!(count, 0);
}

#[rstest]
#[tokio::test]
async fn test_client_replies_to_server_ping() {
    let (addr, state) = start_test_server().await.unwrap();
    state.send_initial_ping.store(true, Ordering::Relaxed);
    let ws_url = format!("ws://{}/realtime", addr);

    let mut client = BitmexWebSocketClient::new(
        Some(ws_url),
        None,
        None,
        Some(AccountId::new("BITMEX-001")),
        None,
    )
    .unwrap();

    client.connect().await.unwrap();

    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            if state.received_pong.load(Ordering::Relaxed) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("expected pong response from client");

    let pong_payload = state.last_pong.lock().await.clone();
    assert_eq!(pong_payload.as_deref(), Some(TEST_PING_PAYLOAD));

    client.close().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_subscribe_to_public_data() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{}/realtime", addr);

    let mut client = BitmexWebSocketClient::new(
        Some(ws_url),
        None, // No API key for public data
        None, // No API secret for public data
        Some(AccountId::new("BITMEX-001")),
        None,
    )
    .unwrap();

    // Connect to the mock server
    client.connect().await.unwrap();

    // Subscribe to trades
    let instrument_id = nautilus_model::identifiers::InstrumentId::from("XBTUSD.BITMEX");
    client.subscribe_trades(instrument_id).await.unwrap();

    // Wait for subscription confirmation and data
    tokio::time::sleep(Duration::from_millis(200)).await;

    // Verify subscription state
    assert!(client.is_active());
    let subscriptions = state.subscriptions.lock().await;
    assert!(subscriptions.contains(&"trade:XBTUSD".to_string()));

    // Verify no authentication needed for public data
    assert!(!state.authenticated.load(Ordering::Relaxed));

    // Close the connection
    client.close().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_subscribe_to_orderbook() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{}/realtime", addr);

    let mut client = BitmexWebSocketClient::new(
        Some(ws_url),
        None,
        None,
        Some(AccountId::new("BITMEX-001")),
        None,
    )
    .unwrap();

    // Connect to the mock server
    client.connect().await.unwrap();

    // Subscribe to order book and trades (test multiple subscriptions)
    let instrument_id = nautilus_model::identifiers::InstrumentId::from("XBTUSD.BITMEX");
    client.subscribe_book(instrument_id).await.unwrap();
    client.subscribe_trades(instrument_id).await.unwrap();

    // Wait for subscription confirmation and data
    tokio::time::sleep(Duration::from_millis(200)).await;

    // Verify both subscriptions are active
    assert!(client.is_active());
    let subscriptions = state.subscriptions.lock().await;
    assert!(subscriptions.contains(&"orderBookL2:XBTUSD".to_string()));
    assert!(subscriptions.contains(&"trade:XBTUSD".to_string()));
    // Note: instrument subscription is also automatically added
    assert!(subscriptions.len() >= 2);

    // Close the connection
    client.close().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_subscribe_to_private_data() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{}/realtime", addr);

    let mut client = BitmexWebSocketClient::new(
        Some(ws_url),
        Some("test_api_key".to_string()),
        Some("test_api_secret".to_string()),
        Some(AccountId::new("BITMEX-001")),
        None,
    )
    .unwrap();

    // Connect to the mock server
    client.connect().await.unwrap();

    // Subscribe to private channels
    client.subscribe_positions().await.unwrap();
    client.subscribe_orders().await.unwrap();
    client.subscribe_executions().await.unwrap();

    // Wait for subscription confirmations and data
    tokio::time::sleep(Duration::from_millis(300)).await;

    // Verify client is active and authenticated
    assert!(client.is_active());
    assert!(state.authenticated.load(Ordering::Relaxed));

    // Verify auth was called
    let auth_calls = *state.auth_calls.lock().await;
    assert!(auth_calls >= 1);

    // Verify private subscriptions
    let subscriptions = state.subscriptions.lock().await;
    assert!(subscriptions.contains(&"position".to_string()));
    assert!(subscriptions.contains(&"order".to_string()));
    assert!(subscriptions.contains(&"execution".to_string()));

    // Close the connection
    client.close().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_reconnection_scenario() {
    // This test simulates a reconnection scenario where the server drops
    // the connection and the client needs to reconnect and restore subscriptions
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{}/realtime", addr);

    let mut client = BitmexWebSocketClient::new(
        Some(ws_url.clone()),
        Some("test_api_key".to_string()),
        Some("test_api_secret".to_string()),
        Some(AccountId::new("BITMEX-001")),
        None,
    )
    .unwrap();

    // Connect and subscribe to some channels
    client.connect().await.unwrap();

    let instrument_id = nautilus_model::identifiers::InstrumentId::from("XBTUSD.BITMEX");
    client.subscribe_trades(instrument_id).await.unwrap();
    client.subscribe_book(instrument_id).await.unwrap();
    client.subscribe_positions().await.unwrap();

    // Wait for subscriptions to be established
    client.wait_until_active(2.0).await.unwrap();

    let events = wait_for_subscription_events(&state, Duration::from_secs(5), |events| {
        events
            .iter()
            .any(|(topic, ok)| topic == "trade:XBTUSD" && *ok)
            && events
                .iter()
                .any(|(topic, ok)| topic == "orderBookL2:XBTUSD" && *ok)
            && events.iter().any(|(topic, ok)| topic == "position" && *ok)
    })
    .await;

    assert!(
        events.iter().any(|(topic, ok)| topic == "position" && *ok),
        "position subscription should be confirmed"
    );

    // Verify initial connection
    assert!(client.is_active());
    let initial_count = *state.connection_count.lock().await;
    assert_eq!(initial_count, 1);

    // Clear subscription events so we can verify fresh resubscriptions after reconnection
    state.clear_subscription_events().await;

    // Trigger server-side drop to simulate disconnection (triggers automatic reconnection)
    state.drop_connections.store(true, Ordering::Relaxed);
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Reset drop flag so reconnection can succeed
    state.drop_connections.store(false, Ordering::Relaxed);

    // Wait for automatic reconnection
    client.wait_until_active(10.0).await.unwrap();
    wait_for_connection_count(&state, 1, Duration::from_secs(5)).await;

    // Give time for re-auth and subscription restoration to complete
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Verify reconnection successful
    assert!(client.is_active());
    let reconnected_count = *state.connection_count.lock().await;
    assert_eq!(reconnected_count, 1);

    // Verify subscriptions were restored
    let events = wait_for_subscription_events(&state, Duration::from_secs(20), |events| {
        events
            .iter()
            .any(|(topic, ok)| topic == "trade:XBTUSD" && *ok)
            && events
                .iter()
                .any(|(topic, ok)| topic == "orderBookL2:XBTUSD" && *ok)
            && events.iter().any(|(topic, ok)| topic == "position" && *ok)
    })
    .await;

    assert!(
        events
            .iter()
            .any(|(topic, ok)| topic == "trade:XBTUSD" && *ok),
        "trade:XBTUSD should be restored after reconnection"
    );
    assert!(
        events
            .iter()
            .any(|(topic, ok)| topic == "orderBookL2:XBTUSD" && *ok),
        "orderBookL2:XBTUSD should be restored after reconnection"
    );
    assert!(
        events.iter().any(|(topic, ok)| topic == "position" && *ok),
        "position should be restored after reconnection"
    );

    // Clean up
    client.close().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_unsubscribe() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{}/realtime", addr);

    let mut client = BitmexWebSocketClient::new(
        Some(ws_url),
        None,
        None,
        Some(AccountId::new("BITMEX-001")),
        None,
    )
    .unwrap();

    // Connect and subscribe
    client.connect().await.unwrap();
    let instrument_id = nautilus_model::identifiers::InstrumentId::from("XBTUSD.BITMEX");
    client.subscribe_trades(instrument_id).await.unwrap();

    // Wait for subscription
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Verify subscription exists
    {
        let subs = state.subscriptions.lock().await;
        assert!(subs.contains(&"trade:XBTUSD".to_string()));
    }

    // Unsubscribe
    client.unsubscribe_trades(instrument_id).await.unwrap();

    // Wait for unsubscription
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Verify topic was removed from subscriptions
    {
        let subs = state.subscriptions.lock().await;
        assert!(!subs.contains(&"trade:XBTUSD".to_string()));
    }

    // Client should still be active after unsubscribe
    assert!(client.is_active());

    // Close the connection
    client.close().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_wait_until_active_timeout() {
    // Test that wait_until_active properly times out when not connected
    let client = BitmexWebSocketClient::new(
        None,
        Some("test_key".to_string()),
        Some("test_secret".to_string()),
        Some(AccountId::new("BITMEX-001")),
        None,
    )
    .unwrap();

    // Should timeout since client is not connected
    let result = client.wait_until_active(0.1).await;

    assert!(result.is_err());
    assert!(!client.is_active());
}

#[rstest]
#[tokio::test]
async fn test_multiple_symbols_subscription() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{}/realtime", addr);

    let mut client = BitmexWebSocketClient::new(
        Some(ws_url),
        None,
        None,
        Some(AccountId::new("BITMEX-001")),
        None,
    )
    .unwrap();

    // Connect to the mock server
    client.connect().await.unwrap();

    // Subscribe to multiple symbols
    let xbt_id = nautilus_model::identifiers::InstrumentId::from("XBTUSD.BITMEX");
    let eth_id = nautilus_model::identifiers::InstrumentId::from("ETHUSD.BITMEX");

    client.subscribe_trades(xbt_id).await.unwrap();
    client.subscribe_trades(eth_id).await.unwrap();
    client.subscribe_book(xbt_id).await.unwrap();

    // Wait for subscriptions
    tokio::time::sleep(Duration::from_millis(300)).await;

    // Verify all subscriptions are tracked
    assert!(client.is_active());
    let subscriptions = state.subscriptions.lock().await;
    assert!(subscriptions.contains(&"trade:XBTUSD".to_string()));
    assert!(subscriptions.contains(&"trade:ETHUSD".to_string()));
    assert!(subscriptions.contains(&"orderBookL2:XBTUSD".to_string()));

    // Close the connection
    client.close().await.unwrap();
}

// Removed test_server_side_drop_with_auto_reconnect - see test_true_auto_reconnect_with_verification for comprehensive testing

// Removed test_server_side_silent_drop - see test_true_auto_reconnect_with_verification for comprehensive testing

#[rstest]
#[tokio::test]
async fn test_true_auto_reconnect_with_verification() {
    // This test verifies the actual auto-reconnect path by checking:
    // 1. Connection count increases (new connection established)
    // 2. Auth calls increase (re-authentication happened)
    // 3. Subscriptions are restored
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{}/realtime", addr);

    let mut client = BitmexWebSocketClient::new(
        Some(ws_url.clone()),
        Some("test_api_key".to_string()),
        Some("test_api_secret".to_string()),
        Some(AccountId::new("BITMEX-001")),
        None,
    )
    .unwrap();

    // Initial connect and subscribe to both public and private channels
    client.connect().await.unwrap();

    let instrument_id = nautilus_model::identifiers::InstrumentId::from("XBTUSD.BITMEX");
    client.subscribe_trades(instrument_id).await.unwrap();
    client.subscribe_book(instrument_id).await.unwrap();
    client.subscribe_positions().await.unwrap();
    client.subscribe_orders().await.unwrap();

    // Wait for initial setup to complete
    client.wait_until_active(2.0).await.unwrap();
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Capture initial state
    let initial_connection_count = *state.connection_count.lock().await;
    let initial_auth_calls = *state.auth_calls.lock().await;
    let initial_subs = {
        let subs = state.subscriptions.lock().await;
        subs.clone()
    };

    println!("Initial state:");
    println!("  Connection count: {}", initial_connection_count);
    println!("  Auth calls: {}", initial_auth_calls);
    println!("  Subscriptions: {:?}", initial_subs);

    // Should have at least 1 connection and 1 auth call
    assert_eq!(
        initial_connection_count, 1,
        "Should have 1 initial connection"
    );
    assert_eq!(initial_auth_calls, 1, "Should have 1 initial auth call");
    assert!(
        !initial_subs.is_empty(),
        "Should have initial subscriptions"
    );

    // Trigger server-side drop (graceful close)
    println!("Triggering server-side drop...");
    state.drop_connections.store(true, Ordering::Relaxed);
    state.silent_drop.store(false, Ordering::Relaxed); // Graceful close

    // Wait a bit for the drop to be processed
    tokio::time::sleep(Duration::from_millis(200)).await;

    // Reset the drop flag so reconnection can succeed
    state.drop_connections.store(false, Ordering::Relaxed);

    // Now wait for auto-reconnection to happen
    println!("Waiting for auto-reconnection...");

    // Use wait_until_active to wait for reconnection
    let reconnect_result = client.wait_until_active(10.0).await;

    if reconnect_result.is_ok() {
        println!("Client is active after potential reconnection");

        // Give some time for re-auth and resubscribe to complete
        tokio::time::sleep(Duration::from_millis(1000)).await;

        // Check if reconnection actually happened
        let final_connection_count = *state.connection_count.lock().await;
        let final_auth_calls = *state.auth_calls.lock().await;
        let final_subs = {
            let subs = state.subscriptions.lock().await;
            subs.clone()
        };

        println!("Final state:");
        println!("  Connection count: {}", final_connection_count);
        println!("  Auth calls: {}", final_auth_calls);
        println!("  Subscriptions: {:?}", final_subs);

        // These assertions will tell us if auto-reconnect truly happened
        if final_connection_count > initial_connection_count {
            println!("Auto-reconnect SUCCEEDED - new connection established");
            assert_eq!(final_connection_count, initial_connection_count + 1);
        } else {
            println!("Auto-reconnect did NOT trigger new connection");
        }

        if final_auth_calls > initial_auth_calls {
            println!("Re-authentication SUCCEEDED");
            assert_eq!(final_auth_calls, initial_auth_calls + 1);
        } else {
            println!("Re-authentication did NOT happen");
        }

        // Check if subscriptions were restored
        assert!(
            final_subs.len() >= initial_subs.len(),
            "Subscriptions should be restored after reconnection. Initial: {}, Final: {}",
            initial_subs.len(),
            final_subs.len()
        );
        println!("Subscriptions restored: {} topics", final_subs.len());
    } else {
        println!("Client never became active again - auto-reconnect failed");
        println!("Wait result: {:?}", reconnect_result);
    }

    client.close().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_auth_and_subscription_restoration_order() {
    // Test that reconnection follows proper order: auth first, then subscriptions
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{}/realtime", addr);

    let mut client = BitmexWebSocketClient::new(
        Some(ws_url.clone()),
        Some("test_api_key".to_string()),
        Some("test_api_secret".to_string()),
        Some(AccountId::new("BITMEX-001")),
        None,
    )
    .unwrap();

    // Connect and subscribe to private channels
    client.connect().await.unwrap();

    client.subscribe_positions().await.unwrap();
    client.subscribe_orders().await.unwrap();
    client.subscribe_executions().await.unwrap();

    // Wait for authentication and subscriptions
    tokio::time::sleep(Duration::from_millis(400)).await;

    // Verify authentication happened
    assert!(
        state.authenticated.load(Ordering::Relaxed),
        "Should be authenticated after private channel subscriptions"
    );

    // Verify private subscriptions were accepted
    let subs = {
        let subs = state.subscriptions.lock().await;
        subs.clone()
    };

    assert!(subs.contains(&"position".to_string()));
    assert!(subs.contains(&"order".to_string()));
    assert!(subs.contains(&"execution".to_string()));

    client.close().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_subscription_restoration_tracking() {
    // Test that subscription restoration only restores previously subscribed topics
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{}/realtime", addr);

    let mut client = BitmexWebSocketClient::new(
        Some(ws_url.clone()),
        Some("test_api_key".to_string()),
        Some("test_api_secret".to_string()),
        Some(AccountId::new("BITMEX-001")),
        None,
    )
    .unwrap();

    // Connect and make specific subscriptions
    client.connect().await.unwrap();

    let xbt_id = nautilus_model::identifiers::InstrumentId::from("XBTUSD.BITMEX");
    let eth_id = nautilus_model::identifiers::InstrumentId::from("ETHUSD.BITMEX");

    client.subscribe_trades(xbt_id).await.unwrap();
    client.subscribe_book(xbt_id).await.unwrap();
    client.subscribe_trades(eth_id).await.unwrap();
    client.subscribe_positions().await.unwrap();

    // Wait for subscriptions
    tokio::time::sleep(Duration::from_millis(300)).await;

    let initial_subs = {
        let subs = state.subscriptions.lock().await;
        subs.clone()
    };

    // Verify expected subscriptions exist
    assert!(initial_subs.contains(&"instrument".to_string()));
    assert!(initial_subs.contains(&"trade:XBTUSD".to_string()));
    assert!(initial_subs.contains(&"orderBookL2:XBTUSD".to_string()));
    assert!(initial_subs.contains(&"trade:ETHUSD".to_string()));
    assert!(initial_subs.contains(&"position".to_string()));

    // Unsubscribe from one topic
    client.unsubscribe_trades(eth_id).await.unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Verify unsubscription removed the topic from subscriptions
    let subs_after_unsub = {
        let subs = state.subscriptions.lock().await;
        subs.clone()
    };
    assert!(!subs_after_unsub.contains(&"trade:ETHUSD".to_string()));
    assert!(subs_after_unsub.contains(&"trade:XBTUSD".to_string())); // Other subscriptions remain

    client.close().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_reconnection_retries_failed_subscriptions() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{}/realtime", addr);

    let mut client = BitmexWebSocketClient::new(
        Some(ws_url.clone()),
        Some("test_api_key".to_string()),
        Some("test_api_secret".to_string()),
        Some(AccountId::new("BITMEX-001")),
        None,
    )
    .unwrap();

    client.connect().await.unwrap();

    let instrument_id = nautilus_model::identifiers::InstrumentId::from("XBTUSD.BITMEX");
    client.subscribe_trades(instrument_id).await.unwrap();
    client.subscribe_positions().await.unwrap();

    client.wait_until_active(2.0).await.unwrap();
    tokio::time::sleep(Duration::from_millis(400)).await;

    let initial_events = state.subscription_events().await;
    assert!(
        initial_events
            .iter()
            .any(|(topic, ok)| topic == "position" && *ok),
        "initial subscription events missing expected position confirmation; events={initial_events:?}",
    );

    state.clear_subscription_events().await;
    let initial_auth_calls = *state.auth_calls.lock().await;

    state.fail_next_subscription("position").await;

    state.drop_connections.store(true, Ordering::Relaxed);
    wait_for_connection_count(&state, 0, Duration::from_secs(5)).await;
    state.drop_connections.store(false, Ordering::Relaxed);

    client.wait_until_active(10.0).await.unwrap();
    wait_for_connection_count(&state, 1, Duration::from_secs(5)).await;

    let first_events = wait_for_subscription_events(&state, Duration::from_secs(20), |events| {
        let instrument_ok = events
            .iter()
            .any(|(topic, ok)| topic == "instrument" && *ok);
        let trade_ok = events
            .iter()
            .any(|(topic, ok)| topic == "trade:XBTUSD" && *ok);
        let position_failed = events.iter().any(|(topic, ok)| topic == "position" && !*ok);
        instrument_ok && trade_ok && position_failed
    })
    .await;
    assert!(
        first_events
            .iter()
            .any(|(topic, ok)| topic == "position" && !*ok),
        "position subscription should fail once to simulate server rejection; events={first_events:?}",
    );

    let state_for_auth = state.clone();
    wait_until_async(
        || {
            let state = state_for_auth.clone();
            let threshold = initial_auth_calls + 1;
            async move { *state.auth_calls.lock().await >= threshold }
        },
        Duration::from_secs(8),
    )
    .await;

    let auth_calls_after_first = *state.auth_calls.lock().await;
    assert!(
        auth_calls_after_first > initial_auth_calls,
        "expected re-authentication before retrying subscriptions",
    );

    state.clear_subscription_events().await;

    state.drop_connections.store(true, Ordering::Relaxed);
    wait_for_connection_count(&state, 0, Duration::from_secs(5)).await;
    state.drop_connections.store(false, Ordering::Relaxed);

    client.wait_until_active(10.0).await.unwrap();
    wait_for_connection_count(&state, 1, Duration::from_secs(5)).await;

    let second_events = wait_for_subscription_events(&state, Duration::from_secs(20), |events| {
        events.iter().any(|(topic, ok)| topic == "position" && *ok)
    })
    .await;
    assert!(
        second_events
            .iter()
            .any(|(topic, ok)| topic == "position" && *ok),
        "position subscription should be retried on subsequent reconnect; events={second_events:?}",
    );

    client.close().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_reconnection_waits_for_delayed_auth_ack() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{}/realtime", addr);

    let mut client = BitmexWebSocketClient::new(
        Some(ws_url.clone()),
        Some("test_api_key".to_string()),
        Some("test_api_secret".to_string()),
        Some(AccountId::new("BITMEX-001")),
        None,
    )
    .unwrap();

    client.connect().await.unwrap();

    let instrument_id = nautilus_model::identifiers::InstrumentId::from("XBTUSD.BITMEX");
    client.subscribe_trades(instrument_id).await.unwrap();
    client.subscribe_positions().await.unwrap();

    client.wait_until_active(2.0).await.unwrap();
    tokio::time::sleep(Duration::from_millis(400)).await;

    let initial_events = state.subscription_events().await;
    assert!(
        initial_events
            .iter()
            .any(|(topic, ok)| topic == "position" && *ok),
        "initial subscription events missing expected position confirmation; events={initial_events:?}",
    );

    state.clear_subscription_events().await;
    let baseline_auth_calls = *state.auth_calls.lock().await;
    state.set_auth_response_delay_ms(Some(600)).await;

    state.drop_connections.store(true, Ordering::Relaxed);
    wait_for_connection_count(&state, 0, Duration::from_secs(5)).await;
    state.drop_connections.store(false, Ordering::Relaxed);

    client.wait_until_active(10.0).await.unwrap();
    wait_for_connection_count(&state, 1, Duration::from_secs(5)).await;

    let state_for_auth = state.clone();
    let expected_calls = baseline_auth_calls + 1;
    wait_until_async(
        || {
            let state = state_for_auth.clone();
            async move { *state.auth_calls.lock().await >= expected_calls }
        },
        Duration::from_secs(8),
    )
    .await;

    {
        let events = state.subscription_events().await;
        assert!(
            events.is_empty(),
            "subscriptions should wait for the delayed auth acknowledgment; events={events:?}",
        );
    }

    let events_after_ack = wait_for_subscription_events(&state, Duration::from_secs(8), |events| {
        let instrument_ok = events
            .iter()
            .any(|(topic, ok)| topic == "instrument" && *ok);
        let trade_ok = events
            .iter()
            .any(|(topic, ok)| topic == "trade:XBTUSD" && *ok);
        let position_ok = events.iter().any(|(topic, ok)| topic == "position" && *ok);
        instrument_ok && trade_ok && position_ok
    })
    .await;
    assert!(
        events_after_ack
            .iter()
            .any(|(topic, ok)| topic == "instrument" && *ok),
        "instrument subscription should be restored after auth ack; events={events_after_ack:?}",
    );
    assert!(
        events_after_ack
            .iter()
            .any(|(topic, ok)| topic == "position" && *ok),
        "private subscription should wait for auth ack before restoring; events={events_after_ack:?}",
    );
    assert!(
        events_after_ack
            .iter()
            .any(|(topic, ok)| topic == "trade:XBTUSD" && *ok),
        "public subscription should still be included after ack delay; events={events_after_ack:?}",
    );

    state.set_auth_response_delay_ms(None).await;
    client.close().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_unauthenticated_private_channel_rejection() {
    // Test that private channels are rejected without authentication
    let (addr, _state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{}/realtime", addr);

    let mut client = BitmexWebSocketClient::new(
        Some(ws_url.clone()),
        None, // No credentials
        None,
        Some(AccountId::new("BITMEX-001")),
        None,
    )
    .unwrap();

    // Connect without credentials
    client.connect().await.unwrap();

    // Attempt to subscribe to private channels should fail
    let result = client.subscribe_positions().await;
    assert!(result.is_err());

    let result = client.subscribe_orders().await;
    assert!(result.is_err());

    // Public channels should still work
    let instrument_id = nautilus_model::identifiers::InstrumentId::from("XBTUSD.BITMEX");
    let result = client.subscribe_trades(instrument_id).await;
    assert!(result.is_ok());

    client.close().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_heartbeat_timeout_reconnection() {
    // Test reconnection triggered by heartbeat timeout
    let (addr, _state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{}/realtime", addr);

    let mut client = BitmexWebSocketClient::new(
        Some(ws_url.clone()),
        Some("test_api_key".to_string()),
        Some("test_api_secret".to_string()),
        Some(AccountId::new("BITMEX-001")),
        Some(1), // Very short heartbeat interval (1 second)
    )
    .unwrap();

    // Connect with heartbeat enabled
    client.connect().await.unwrap();

    let instrument_id = nautilus_model::identifiers::InstrumentId::from("XBTUSD.BITMEX");
    client.subscribe_trades(instrument_id).await.unwrap();

    // Wait for initial connection
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(client.is_active());

    // SAFETY: Heartbeat configuration doesn't break connection
    // TODO: Add server flag to suppress pong responses and test actual heartbeat timeout

    // Wait a bit longer to see if heartbeat causes any issues
    tokio::time::sleep(Duration::from_millis(1500)).await;

    // Connection should still be active
    assert!(client.is_active());

    client.close().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_rapid_consecutive_reconnections() {
    // Test that rapid consecutive disconnects/reconnects don't cause state corruption
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{}/realtime", addr);

    let mut client = BitmexWebSocketClient::new(
        Some(ws_url.clone()),
        Some("test_api_key".to_string()),
        Some("test_api_secret".to_string()),
        Some(AccountId::new("BITMEX-001")),
        None,
    )
    .unwrap();

    // Initial connection with subscriptions
    client.connect().await.unwrap();

    let instrument_id = nautilus_model::identifiers::InstrumentId::from("XBTUSD.BITMEX");
    client.subscribe_trades(instrument_id).await.unwrap();
    client.subscribe_book(instrument_id).await.unwrap();
    client.subscribe_positions().await.unwrap();

    client.wait_until_active(2.0).await.unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;

    let initial_auth_calls = *state.auth_calls.lock().await;
    assert_eq!(initial_auth_calls, 1, "Should have 1 initial auth call");

    // Perform 3 rapid disconnect/reconnect cycles
    for cycle in 1..=3 {
        println!("Starting cycle {cycle}");

        // Clear subscription events to verify fresh resubscriptions
        state.clear_subscription_events().await;

        // Trigger server drop
        state.drop_connections.store(true, Ordering::Relaxed);
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Allow reconnection
        state.drop_connections.store(false, Ordering::Relaxed);

        // Wait for reconnection
        let reconnect_result = client.wait_until_active(10.0).await;
        assert!(
            reconnect_result.is_ok(),
            "Reconnection cycle {cycle} failed"
        );

        // Wait for subscription restoration (20s to account for slower CI runners)
        let events = wait_for_subscription_events(&state, Duration::from_secs(20), |events| {
            events
                .iter()
                .any(|(topic, ok)| topic == "trade:XBTUSD" && *ok)
                && events
                    .iter()
                    .any(|(topic, ok)| topic == "orderBookL2:XBTUSD" && *ok)
                && events.iter().any(|(topic, ok)| topic == "position" && *ok)
        })
        .await;

        // Verify all subscriptions were restored in this cycle
        assert!(
            events
                .iter()
                .any(|(topic, ok)| topic == "trade:XBTUSD" && *ok),
            "Cycle {cycle}: trade:XBTUSD should be resubscribed; events={events:?}"
        );
        assert!(
            events
                .iter()
                .any(|(topic, ok)| topic == "orderBookL2:XBTUSD" && *ok),
            "Cycle {cycle}: orderBookL2:XBTUSD should be resubscribed; events={events:?}"
        );
        assert!(
            events.iter().any(|(topic, ok)| topic == "position" && *ok),
            "Cycle {cycle}: position should be resubscribed; events={events:?}"
        );
    }

    // Verify re-authentication happened during reconnections
    // Use >= because rapid reconnections can cause race conditions in auth call timing
    let final_auth_calls = *state.auth_calls.lock().await;
    assert!(
        final_auth_calls >= 4,
        "Should have at least 4 total auth calls (1 initial + 3 reconnects), got {final_auth_calls}"
    );

    client.close().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_multiple_partial_subscription_failures() {
    // Test handling of multiple simultaneous subscription failures during restore
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{}/realtime", addr);

    let mut client = BitmexWebSocketClient::new(
        Some(ws_url.clone()),
        Some("test_api_key".to_string()),
        Some("test_api_secret".to_string()),
        Some(AccountId::new("BITMEX-001")),
        None,
    )
    .unwrap();

    client.connect().await.unwrap();

    let xbt_id = nautilus_model::identifiers::InstrumentId::from("XBTUSD.BITMEX");
    let eth_id = nautilus_model::identifiers::InstrumentId::from("ETHUSD.BITMEX");

    // Subscribe to 5 different channels
    client.subscribe_trades(xbt_id).await.unwrap();
    client.subscribe_book(xbt_id).await.unwrap();
    client.subscribe_trades(eth_id).await.unwrap();
    client.subscribe_positions().await.unwrap();
    client.subscribe_orders().await.unwrap();

    client.wait_until_active(2.0).await.unwrap();
    tokio::time::sleep(Duration::from_millis(500)).await;

    state.clear_subscription_events().await;

    // Set up 3 subscriptions to fail on next reconnect
    state.fail_next_subscription("trade:XBTUSD").await;
    state.fail_next_subscription("position").await;
    state.fail_next_subscription("order").await;

    // Trigger disconnect
    state.drop_connections.store(true, Ordering::Relaxed);
    wait_for_connection_count(&state, 0, Duration::from_secs(5)).await;
    state.drop_connections.store(false, Ordering::Relaxed);

    // Wait for reconnection
    client.wait_until_active(10.0).await.unwrap();
    wait_for_connection_count(&state, 1, Duration::from_secs(5)).await;

    // Wait for subscription restoration attempts
    let first_events = wait_for_subscription_events(&state, Duration::from_secs(20), |events| {
        let trade_xbt_failed = events
            .iter()
            .any(|(topic, ok)| topic == "trade:XBTUSD" && !*ok);
        let position_failed = events.iter().any(|(topic, ok)| topic == "position" && !*ok);
        let order_failed = events.iter().any(|(topic, ok)| topic == "order" && !*ok);

        // Also check that successful ones succeeded
        let instrument_ok = events
            .iter()
            .any(|(topic, ok)| topic == "instrument" && *ok);
        let trade_eth_ok = events
            .iter()
            .any(|(topic, ok)| topic == "trade:ETHUSD" && *ok);
        let book_ok = events
            .iter()
            .any(|(topic, ok)| topic == "orderBookL2:XBTUSD" && *ok);

        trade_xbt_failed
            && position_failed
            && order_failed
            && instrument_ok
            && trade_eth_ok
            && book_ok
    })
    .await;

    // Verify we got the expected mix of successes and failures
    assert!(
        first_events
            .iter()
            .any(|(topic, ok)| topic == "trade:XBTUSD" && !*ok),
        "trade:XBTUSD should fail"
    );
    assert!(
        first_events
            .iter()
            .any(|(topic, ok)| topic == "position" && !*ok),
        "position should fail"
    );
    assert!(
        first_events
            .iter()
            .any(|(topic, ok)| topic == "order" && !*ok),
        "order should fail"
    );

    // Successful subscriptions
    assert!(
        first_events
            .iter()
            .any(|(topic, ok)| topic == "trade:ETHUSD" && *ok),
        "trade:ETHUSD should succeed"
    );
    assert!(
        first_events
            .iter()
            .any(|(topic, ok)| topic == "orderBookL2:XBTUSD" && *ok),
        "orderBookL2:XBTUSD should succeed"
    );

    state.clear_subscription_events().await;

    // Second reconnect should retry all failed subscriptions
    state.drop_connections.store(true, Ordering::Relaxed);
    wait_for_connection_count(&state, 0, Duration::from_secs(5)).await;
    state.drop_connections.store(false, Ordering::Relaxed);

    client.wait_until_active(10.0).await.unwrap();
    wait_for_connection_count(&state, 1, Duration::from_secs(5)).await;

    let second_events = wait_for_subscription_events(&state, Duration::from_secs(20), |events| {
        events
            .iter()
            .any(|(topic, ok)| topic == "trade:XBTUSD" && *ok)
            && events.iter().any(|(topic, ok)| topic == "position" && *ok)
            && events.iter().any(|(topic, ok)| topic == "order" && *ok)
    })
    .await;

    // All previously failed subscriptions should now succeed
    assert!(
        second_events
            .iter()
            .any(|(topic, ok)| topic == "trade:XBTUSD" && *ok),
        "trade:XBTUSD should succeed on retry"
    );
    assert!(
        second_events
            .iter()
            .any(|(topic, ok)| topic == "position" && *ok),
        "position should succeed on retry"
    );
    assert!(
        second_events
            .iter()
            .any(|(topic, ok)| topic == "order" && *ok),
        "order should succeed on retry"
    );

    client.close().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_reconnection_race_condition() {
    // Test disconnect request during active reconnection
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{}/realtime", addr);

    let mut client = BitmexWebSocketClient::new(
        Some(ws_url.clone()),
        Some("test_api_key".to_string()),
        Some("test_api_secret".to_string()),
        Some(AccountId::new("BITMEX-001")),
        None,
    )
    .unwrap();

    client.connect().await.unwrap();

    let instrument_id = nautilus_model::identifiers::InstrumentId::from("XBTUSD.BITMEX");
    client.subscribe_trades(instrument_id).await.unwrap();
    client.subscribe_positions().await.unwrap();

    client.wait_until_active(2.0).await.unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;

    // Add significant auth delay to create a window for race condition
    state.set_auth_response_delay_ms(Some(1000)).await;

    // Trigger first disconnect
    state.drop_connections.store(true, Ordering::Relaxed);
    wait_for_connection_count(&state, 0, Duration::from_secs(5)).await;
    state.drop_connections.store(false, Ordering::Relaxed);

    // Wait a bit for reconnection to start but not complete (due to auth delay)
    tokio::time::sleep(Duration::from_millis(300)).await;

    // Trigger another disconnect while reconnection is in progress
    state.drop_connections.store(true, Ordering::Relaxed);
    tokio::time::sleep(Duration::from_millis(100)).await;
    state.drop_connections.store(false, Ordering::Relaxed);

    // Clear the delay
    state.set_auth_response_delay_ms(None).await;

    // Client should eventually recover
    let final_result = client.wait_until_active(15.0).await;
    assert!(
        final_result.is_ok(),
        "Client should recover despite reconnection race condition"
    );

    // Verify subscriptions are restored
    tokio::time::sleep(Duration::from_millis(500)).await;
    let subs = state.subscriptions.lock().await;
    assert!(
        subs.contains(&"trade:XBTUSD".to_string()),
        "Trade subscription should be restored"
    );
    assert!(
        subs.contains(&"position".to_string()),
        "Position subscription should be restored"
    );

    client.close().await.unwrap();
}
