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

//! Integration tests for the Polymarket WebSocket client using a mock server.

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
use futures_util::StreamExt;
use nautilus_common::testing::wait_until_async;
use nautilus_polymarket::{
    common::credential::Credential,
    websocket::{client::PolymarketWebSocketClient, messages::PolymarketWsMessage},
};
use rstest::rstest;
use serde_json::{Value, json};

// base64url of b"test_secret_key_32bytes_pad12345"
const TEST_API_SECRET_B64: &str = "dGVzdF9zZWNyZXRfa2V5XzMyYnl0ZXNfcGFkMTIzNDU=";
const TEST_ASSET_ID: &str =
    "71321045679252212594626385532706912750332728571942532289631379312455583992563";
const TEST_ASSET_ID_2: &str =
    "52114319501245915516055106046884209969926127482827954674443846427813813222426";
const TEST_ASSET_ID_3: &str =
    "16678291189211314787145083999015737376658799626183230671758641503291735614088";

#[derive(Clone)]
struct TestServerState {
    connection_count: Arc<tokio::sync::Mutex<usize>>,
    subscribed_assets: Arc<tokio::sync::Mutex<Vec<String>>>,
    received_market_payloads: Arc<tokio::sync::Mutex<Vec<Value>>>,
    received_user_auth: Arc<tokio::sync::Mutex<Option<Value>>>,
    drop_next_connection: Arc<AtomicBool>,
    ping_count: Arc<AtomicUsize>,
}

impl Default for TestServerState {
    fn default() -> Self {
        Self {
            connection_count: Arc::new(tokio::sync::Mutex::new(0)),
            subscribed_assets: Arc::new(tokio::sync::Mutex::new(Vec::new())),
            received_market_payloads: Arc::new(tokio::sync::Mutex::new(Vec::new())),
            received_user_auth: Arc::new(tokio::sync::Mutex::new(None)),
            drop_next_connection: Arc::new(AtomicBool::new(false)),
            ping_count: Arc::new(AtomicUsize::new(0)),
        }
    }
}

fn data_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test_data")
}

fn load_json(filename: &str) -> Value {
    let content = std::fs::read_to_string(data_path().join(filename))
        .unwrap_or_else(|_| panic!("failed to read {filename}"));
    serde_json::from_str(&content).expect("invalid json")
}

fn test_credential() -> Credential {
    Credential::new("test_api_key", TEST_API_SECRET_B64, "test_pass".to_string()).unwrap()
}

async fn handle_market_upgrade(
    ws: WebSocketUpgrade,
    State(state): State<Arc<TestServerState>>,
) -> Response {
    ws.on_upgrade(move |socket| handle_socket(socket, state, false))
}

async fn handle_user_upgrade(
    ws: WebSocketUpgrade,
    State(state): State<Arc<TestServerState>>,
) -> Response {
    ws.on_upgrade(move |socket| handle_socket(socket, state, true))
}

async fn handle_socket(mut socket: WebSocket, state: Arc<TestServerState>, is_user: bool) {
    {
        let mut count = state.connection_count.lock().await;
        *count += 1;
    }

    let book_msg = json!([load_json("ws_market_book_msg.json")]).to_string();
    let user_order_msg = json!([load_json("ws_user_order_msg.json")]).to_string();

    while let Some(result) = socket.next().await {
        let Ok(msg) = result else { break };

        match msg {
            Message::Text(text) => {
                let Ok(payload) = serde_json::from_str::<Value>(&text) else {
                    continue;
                };

                if is_user {
                    if payload.get("type").and_then(Value::as_str) == Some("user") {
                        *state.received_user_auth.lock().await = payload.get("auth").cloned();

                        if socket
                            .send(Message::Text(user_order_msg.clone().into()))
                            .await
                            .is_err()
                        {
                            break;
                        }
                    }
                } else if payload.get("type").and_then(Value::as_str) == Some("market")
                    || payload.get("operation").and_then(Value::as_str).is_some()
                {
                    state
                        .received_market_payloads
                        .lock()
                        .await
                        .push(payload.clone());

                    if let Some(ids) = payload.get("assets_ids").and_then(Value::as_array) {
                        let mut assets = state.subscribed_assets.lock().await;
                        match payload.get("operation").and_then(Value::as_str) {
                            Some("unsubscribe") => {
                                for id in ids {
                                    if let Some(s) = id.as_str() {
                                        assets.retain(|asset| asset != s);
                                    }
                                }
                            }
                            _ => {
                                for id in ids {
                                    if let Some(s) = id.as_str() {
                                        assets.push(s.to_string());
                                    }
                                }
                            }
                        }
                    }

                    if state.drop_next_connection.swap(false, Ordering::Relaxed) {
                        let _ = socket.send(Message::Close(None)).await;
                        break;
                    }

                    if socket
                        .send(Message::Text(book_msg.clone().into()))
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
            }
            Message::Ping(data) => {
                state.ping_count.fetch_add(1, Ordering::Relaxed);

                if socket.send(Message::Pong(data)).await.is_err() {
                    break;
                }
            }
            Message::Close(_) => break,
            _ => {}
        }
    }

    let mut count = state.connection_count.lock().await;
    *count = count.saturating_sub(1);
}

fn create_test_router(state: Arc<TestServerState>) -> Router {
    Router::new()
        .route("/ws/market", get(handle_market_upgrade))
        .route("/ws/user", get(handle_user_upgrade))
        .with_state(state)
}

async fn start_ws_server(state: Arc<TestServerState>) -> SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("failed to bind");
    let addr = listener.local_addr().expect("missing local addr");
    let router = create_test_router(state);

    tokio::spawn(async move {
        axum::serve(listener, router)
            .await
            .expect("ws server failed");
    });

    tokio::time::sleep(Duration::from_millis(50)).await;
    addr
}

async fn wait_until_active(client: &PolymarketWebSocketClient, timeout_secs: f64) {
    wait_until_async(
        || {
            let active = client.is_active();
            async move { active }
        },
        Duration::from_secs_f64(timeout_secs),
    )
    .await;
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

async fn wait_for_market_payload_count(
    state: &TestServerState,
    expected: usize,
    timeout: Duration,
) {
    wait_until_async(
        || {
            let state = state.clone();
            async move { state.received_market_payloads.lock().await.len() >= expected }
        },
        timeout,
    )
    .await;
}

#[rstest]
#[tokio::test]
async fn test_client_not_active_before_connect() {
    let client = PolymarketWebSocketClient::new_market(
        Some("ws://127.0.0.1:9999/ws/market".to_string()),
        true,
    );
    assert!(!client.is_active());
}

#[rstest]
#[tokio::test]
async fn test_market_client_connects_and_disconnects() {
    let state = Arc::new(TestServerState::default());
    let addr = start_ws_server(state.clone()).await;
    let ws_url = format!("ws://{addr}/ws/market");

    let mut client = PolymarketWebSocketClient::new_market(Some(ws_url), true);
    client.connect().await.expect("connect failed");

    wait_for_connection_count(&state, 1, Duration::from_secs(5)).await;

    client.disconnect().await.expect("disconnect failed");

    wait_for_connection_count(&state, 0, Duration::from_secs(5)).await;
}

#[rstest]
#[tokio::test]
async fn test_is_active_lifecycle() {
    let state = Arc::new(TestServerState::default());
    let addr = start_ws_server(state.clone()).await;
    let ws_url = format!("ws://{addr}/ws/market");

    let mut client = PolymarketWebSocketClient::new_market(Some(ws_url), true);

    assert!(!client.is_active(), "should not be active before connect");

    client.connect().await.expect("connect failed");
    wait_until_active(&client, 2.0).await;

    assert!(client.is_active(), "should be active after connect");

    client.disconnect().await.expect("disconnect failed");
    wait_until_async(
        || {
            let active = client.is_active();
            async move { !active }
        },
        Duration::from_secs(2),
    )
    .await;

    assert!(!client.is_active(), "should not be active after disconnect");
}

#[rstest]
#[tokio::test]
async fn test_double_connect_is_idempotent() {
    let state = Arc::new(TestServerState::default());
    let addr = start_ws_server(state.clone()).await;
    let ws_url = format!("ws://{addr}/ws/market");

    let mut client = PolymarketWebSocketClient::new_market(Some(ws_url), true);
    client.connect().await.expect("first connect failed");
    wait_until_active(&client, 2.0).await;

    let result = client.connect().await;
    assert!(result.is_ok(), "second connect should not error");

    wait_for_connection_count(&state, 1, Duration::from_secs(2)).await;

    client.disconnect().await.expect("disconnect failed");
}

#[rstest]
#[tokio::test]
async fn test_url_accessor_returns_configured_url() {
    let url = "ws://127.0.0.1:9999/ws/market";
    let client = PolymarketWebSocketClient::new_market(Some(url.to_string()), true);
    assert_eq!(client.url(), url);
}

#[rstest]
#[tokio::test]
async fn test_subscribe_market_sends_assets_ids() {
    let state = Arc::new(TestServerState::default());
    let addr = start_ws_server(state.clone()).await;
    let ws_url = format!("ws://{addr}/ws/market");

    let mut client = PolymarketWebSocketClient::new_market(Some(ws_url), true);
    client.connect().await.expect("connect failed");
    wait_until_active(&client, 2.0).await;

    client
        .subscribe_market(vec![TEST_ASSET_ID.to_string(), TEST_ASSET_ID_2.to_string()])
        .await
        .expect("subscribe failed");

    wait_until_async(
        || {
            let state = state.clone();
            async move { state.subscribed_assets.lock().await.len() >= 2 }
        },
        Duration::from_secs(2),
    )
    .await;

    let assets = state.subscribed_assets.lock().await;
    assert!(assets.contains(&TEST_ASSET_ID.to_string()));
    assert!(assets.contains(&TEST_ASSET_ID_2.to_string()));

    client.disconnect().await.expect("disconnect failed");
}

#[rstest]
#[tokio::test]
async fn test_subscribe_unsubscribe_subscribe_uses_initial_then_incremental_market_messages() {
    let state = Arc::new(TestServerState::default());
    let addr = start_ws_server(state.clone()).await;
    let ws_url = format!("ws://{addr}/ws/market");

    let mut client = PolymarketWebSocketClient::new_market(Some(ws_url), true);
    client.connect().await.expect("connect failed");
    wait_until_active(&client, 2.0).await;

    client
        .subscribe_market(vec![TEST_ASSET_ID.to_string()])
        .await
        .expect("initial subscribe failed");

    client
        .unsubscribe_market(vec![TEST_ASSET_ID.to_string()])
        .await
        .expect("unsubscribe failed");

    client
        .subscribe_market(vec![TEST_ASSET_ID_2.to_string()])
        .await
        .expect("incremental subscribe failed");

    wait_for_market_payload_count(&state, 3, Duration::from_secs(2)).await;

    let payloads = state.received_market_payloads.lock().await.clone();
    assert_eq!(
        payloads.len(),
        3,
        "expected initial subscribe, unsubscribe, and incremental subscribe payloads"
    );

    assert_eq!(
        payloads[0],
        json!({
            "assets_ids": [TEST_ASSET_ID],
            "type": "market",
            "custom_feature_enabled": true,
        }),
        "first market subscribe should use MarketInitialSubscribeRequest"
    );
    assert_eq!(
        payloads[1],
        json!({
            "assets_ids": [TEST_ASSET_ID],
            "operation": "unsubscribe",
        }),
        "unsubscribe should use MarketUnsubscribeRequest"
    );
    assert_eq!(
        payloads[2],
        json!({
            "assets_ids": [TEST_ASSET_ID_2],
            "operation": "subscribe",
            "custom_feature_enabled": true,
        }),
        "second market subscribe should use MarketSubscribeRequest"
    );

    client.disconnect().await.expect("disconnect failed");
}

#[rstest]
#[tokio::test]
async fn test_subscribe_user_sends_auth_payload() {
    let state = Arc::new(TestServerState::default());
    let addr = start_ws_server(state.clone()).await;
    let ws_url = format!("ws://{addr}/ws/user");

    let mut client = PolymarketWebSocketClient::new_user(Some(ws_url), test_credential());
    client.connect().await.expect("connect failed");
    wait_until_active(&client, 2.0).await;

    client
        .subscribe_user()
        .await
        .expect("subscribe_user failed");

    wait_until_async(
        || {
            let state = state.clone();
            async move { state.received_user_auth.lock().await.is_some() }
        },
        Duration::from_secs(2),
    )
    .await;

    let auth = state.received_user_auth.lock().await;
    let auth = auth.as_ref().unwrap();
    assert!(auth.get("apiKey").is_some(), "auth must contain 'apiKey'");
    assert!(auth.get("secret").is_some(), "auth must contain 'secret'");
    assert!(
        auth.get("passphrase").is_some(),
        "auth must contain 'passphrase'"
    );
    assert_eq!(
        auth.get("apiKey").unwrap().as_str().unwrap(),
        "test_api_key"
    );
    // WebSocket auth sends the raw API secret, not an HMAC signature
    assert_eq!(
        auth.get("secret").unwrap().as_str().unwrap(),
        TEST_API_SECRET_B64
    );
    assert_eq!(
        auth.get("passphrase").unwrap().as_str().unwrap(),
        "test_pass"
    );
    // No timestamp or nonce fields in WebSocket auth
    assert!(
        auth.get("timestamp").is_none(),
        "auth must NOT contain 'timestamp'"
    );
    assert!(auth.get("nonce").is_none(), "auth must NOT contain 'nonce'");

    client.disconnect().await.expect("disconnect failed");
}

#[rstest]
#[tokio::test]
async fn test_next_message_receives_market_book() {
    let state = Arc::new(TestServerState::default());
    let addr = start_ws_server(state.clone()).await;
    let ws_url = format!("ws://{addr}/ws/market");

    let mut client = PolymarketWebSocketClient::new_market(Some(ws_url), true);
    client.connect().await.expect("connect failed");
    wait_until_active(&client, 2.0).await;

    client
        .subscribe_market(vec![TEST_ASSET_ID.to_string()])
        .await
        .expect("subscribe failed");

    // Server sends back a book snapshot after subscribing
    let msg = tokio::time::timeout(Duration::from_secs(3), client.next_message())
        .await
        .expect("timed out waiting for message");

    assert!(
        matches!(msg, Some(PolymarketWsMessage::Market(_))),
        "expected a market message, received: {msg:?}"
    );

    client.disconnect().await.expect("disconnect failed");
}

#[rstest]
#[tokio::test]
async fn test_next_message_receives_user_order() {
    let state = Arc::new(TestServerState::default());
    let addr = start_ws_server(state.clone()).await;
    let ws_url = format!("ws://{addr}/ws/user");

    let mut client = PolymarketWebSocketClient::new_user(Some(ws_url), test_credential());
    client.connect().await.expect("connect failed");
    wait_until_active(&client, 2.0).await;

    client
        .subscribe_user()
        .await
        .expect("subscribe_user failed");

    let msg = tokio::time::timeout(Duration::from_secs(3), client.next_message())
        .await
        .expect("timed out waiting for user message");

    assert!(
        matches!(msg, Some(PolymarketWsMessage::User(_))),
        "expected a user message, received: {msg:?}"
    );

    client.disconnect().await.expect("disconnect failed");
}

#[rstest]
#[tokio::test]
async fn test_subscription_count_is_zero_before_subscribe() {
    let state = Arc::new(TestServerState::default());
    let addr = start_ws_server(state.clone()).await;
    let ws_url = format!("ws://{addr}/ws/market");

    let mut client = PolymarketWebSocketClient::new_market(Some(ws_url), true);
    client.connect().await.expect("connect failed");
    wait_until_active(&client, 2.0).await;

    assert_eq!(client.subscription_count(), 0);

    client.disconnect().await.expect("disconnect failed");
}

#[rstest]
#[tokio::test]
async fn test_subscription_count_increments_after_subscribe() {
    let state = Arc::new(TestServerState::default());
    let addr = start_ws_server(state.clone()).await;
    let ws_url = format!("ws://{addr}/ws/market");

    let mut client = PolymarketWebSocketClient::new_market(Some(ws_url), true);
    client.connect().await.expect("connect failed");
    wait_until_active(&client, 2.0).await;

    client
        .subscribe_market(vec![TEST_ASSET_ID.to_string(), TEST_ASSET_ID_2.to_string()])
        .await
        .expect("subscribe failed");

    wait_until_async(
        || {
            let count = client.subscription_count();
            async move { count >= 2 }
        },
        Duration::from_secs(2),
    )
    .await;

    assert_eq!(client.subscription_count(), 2);

    client.disconnect().await.expect("disconnect failed");
}

#[rstest]
#[tokio::test]
async fn test_subscription_count_decrements_after_unsubscribe() {
    let state = Arc::new(TestServerState::default());
    let addr = start_ws_server(state.clone()).await;
    let ws_url = format!("ws://{addr}/ws/market");

    let mut client = PolymarketWebSocketClient::new_market(Some(ws_url), true);
    client.connect().await.expect("connect failed");
    wait_until_active(&client, 2.0).await;

    client
        .subscribe_market(vec![TEST_ASSET_ID.to_string(), TEST_ASSET_ID_2.to_string()])
        .await
        .expect("subscribe failed");

    wait_until_async(
        || {
            let count = client.subscription_count();
            async move { count >= 2 }
        },
        Duration::from_secs(2),
    )
    .await;

    client
        .unsubscribe_market(vec![TEST_ASSET_ID_2.to_string()])
        .await
        .expect("unsubscribe failed");

    wait_until_async(
        || {
            let count = client.subscription_count();
            async move { count == 1 }
        },
        Duration::from_secs(2),
    )
    .await;

    assert_eq!(client.subscription_count(), 1);

    client.disconnect().await.expect("disconnect failed");
}

#[rstest]
#[tokio::test]
async fn test_subscription_count_multiple_subscribe_calls() {
    let state = Arc::new(TestServerState::default());
    let addr = start_ws_server(state.clone()).await;
    let ws_url = format!("ws://{addr}/ws/market");

    let mut client = PolymarketWebSocketClient::new_market(Some(ws_url), true);
    client.connect().await.expect("connect failed");
    wait_until_active(&client, 2.0).await;

    client
        .subscribe_market(vec![TEST_ASSET_ID.to_string()])
        .await
        .expect("first subscribe failed");
    client
        .subscribe_market(vec![
            TEST_ASSET_ID_2.to_string(),
            TEST_ASSET_ID_3.to_string(),
        ])
        .await
        .expect("second subscribe failed");

    wait_until_async(
        || {
            let count = client.subscription_count();
            async move { count >= 3 }
        },
        Duration::from_secs(2),
    )
    .await;

    assert_eq!(client.subscription_count(), 3);

    client.disconnect().await.expect("disconnect failed");
}

#[rstest]
#[tokio::test]
async fn test_subscription_count_unsubscribe_all() {
    let state = Arc::new(TestServerState::default());
    let addr = start_ws_server(state.clone()).await;
    let ws_url = format!("ws://{addr}/ws/market");

    let mut client = PolymarketWebSocketClient::new_market(Some(ws_url), true);
    client.connect().await.expect("connect failed");
    wait_until_active(&client, 2.0).await;

    client
        .subscribe_market(vec![TEST_ASSET_ID.to_string(), TEST_ASSET_ID_2.to_string()])
        .await
        .expect("subscribe failed");

    wait_until_async(
        || {
            let count = client.subscription_count();
            async move { count >= 2 }
        },
        Duration::from_secs(2),
    )
    .await;

    client
        .unsubscribe_market(vec![TEST_ASSET_ID.to_string(), TEST_ASSET_ID_2.to_string()])
        .await
        .expect("unsubscribe failed");

    wait_until_async(
        || {
            let count = client.subscription_count();
            async move { count == 0 }
        },
        Duration::from_secs(2),
    )
    .await;

    assert_eq!(client.subscription_count(), 0);

    client.disconnect().await.expect("disconnect failed");
}

#[rstest]
#[tokio::test]
async fn test_unsubscribe_market_removes_assets_from_reconnect_set() {
    let state = Arc::new(TestServerState::default());
    let addr = start_ws_server(state.clone()).await;
    let ws_url = format!("ws://{addr}/ws/market");

    let mut client = PolymarketWebSocketClient::new_market(Some(ws_url), true);
    client.connect().await.expect("connect failed");
    wait_until_active(&client, 2.0).await;

    client
        .subscribe_market(vec![TEST_ASSET_ID.to_string(), TEST_ASSET_ID_2.to_string()])
        .await
        .expect("subscribe failed");

    wait_until_async(
        || {
            let state = state.clone();
            async move { state.subscribed_assets.lock().await.len() >= 2 }
        },
        Duration::from_secs(2),
    )
    .await;

    client
        .unsubscribe_market(vec![TEST_ASSET_ID_2.to_string()])
        .await
        .expect("unsubscribe failed");

    // Clear server state to verify reconnect re-subscribes only the remaining asset
    state.subscribed_assets.lock().await.clear();

    // Trigger a reconnect by dropping and re-connecting
    state.drop_next_connection.store(true, Ordering::Relaxed);
    let _ = client
        .subscribe_market(vec![TEST_ASSET_ID.to_string()])
        .await;

    wait_until_async(
        || {
            let state = state.clone();
            // After reconnect, resubscribe_all fires; verify assets repopulated
            async move { !state.subscribed_assets.lock().await.is_empty() }
        },
        Duration::from_secs(5),
    )
    .await;

    let assets = state.subscribed_assets.lock().await.clone();

    assert!(
        assets.contains(&TEST_ASSET_ID.to_string()),
        "asset_id should be re-subscribed after reconnect"
    );
    assert!(
        !assets.contains(&TEST_ASSET_ID_2.to_string()),
        "unsubscribed asset_id_2 must not appear after reconnect"
    );

    client.disconnect().await.expect("disconnect failed");
}

#[rstest]
#[tokio::test]
async fn test_reconnect_resubscribes_all_market_assets() {
    let state = Arc::new(TestServerState::default());
    let addr = start_ws_server(state.clone()).await;
    let ws_url = format!("ws://{addr}/ws/market");

    let mut client = PolymarketWebSocketClient::new_market(Some(ws_url), true);
    client.connect().await.expect("connect failed");
    wait_until_active(&client, 2.0).await;

    client
        .subscribe_market(vec![TEST_ASSET_ID.to_string(), TEST_ASSET_ID_2.to_string()])
        .await
        .expect("subscribe failed");

    wait_until_async(
        || {
            let state = state.clone();
            async move { state.subscribed_assets.lock().await.len() >= 2 }
        },
        Duration::from_secs(2),
    )
    .await;

    // Clear server-side subscriptions so we can verify they are restored
    state.subscribed_assets.lock().await.clear();

    // Trigger reconnect: server drops on next subscribe
    state.drop_next_connection.store(true, Ordering::Relaxed);
    let _ = client
        .subscribe_market(vec![TEST_ASSET_ID.to_string()])
        .await;

    // Wait for resubscription after reconnect (handler calls resubscribe_all)
    wait_until_async(
        || {
            let state = state.clone();
            async move { state.subscribed_assets.lock().await.len() >= 2 }
        },
        Duration::from_secs(5),
    )
    .await;

    let assets = state.subscribed_assets.lock().await.clone();
    assert!(
        assets.contains(&TEST_ASSET_ID.to_string()),
        "asset_id must be resubscribed after reconnect"
    );
    assert!(
        assets.contains(&TEST_ASSET_ID_2.to_string()),
        "asset_id_2 must be resubscribed after reconnect"
    );

    client.disconnect().await.expect("disconnect failed");
}

#[rstest]
#[tokio::test]
async fn test_is_authenticated_false_before_connect() {
    let client = PolymarketWebSocketClient::new_user(
        Some("ws://127.0.0.1:9999/ws/user".to_string()),
        test_credential(),
    );
    assert!(!client.is_authenticated());
}

#[rstest]
#[tokio::test]
async fn test_is_authenticated_false_after_connect_before_subscribe_user() {
    let state = Arc::new(TestServerState::default());
    let addr = start_ws_server(state.clone()).await;
    let ws_url = format!("ws://{addr}/ws/user");

    let mut client = PolymarketWebSocketClient::new_user(Some(ws_url), test_credential());
    client.connect().await.expect("connect failed");
    wait_until_active(&client, 2.0).await;

    assert!(!client.is_authenticated());

    client.disconnect().await.expect("disconnect failed");
}

#[rstest]
#[tokio::test]
async fn test_is_authenticated_true_after_subscribe_user() {
    let state = Arc::new(TestServerState::default());
    let addr = start_ws_server(state.clone()).await;
    let ws_url = format!("ws://{addr}/ws/user");

    let mut client = PolymarketWebSocketClient::new_user(Some(ws_url), test_credential());
    client.connect().await.expect("connect failed");
    wait_until_active(&client, 2.0).await;

    client
        .subscribe_user()
        .await
        .expect("subscribe_user failed");

    wait_until_async(
        || {
            let authenticated = client.is_authenticated();
            async move { authenticated }
        },
        Duration::from_secs(2),
    )
    .await;

    assert!(client.is_authenticated());

    client.disconnect().await.expect("disconnect failed");
}

#[rstest]
#[tokio::test]
async fn test_is_authenticated_false_after_disconnect() {
    let state = Arc::new(TestServerState::default());
    let addr = start_ws_server(state.clone()).await;
    let ws_url = format!("ws://{addr}/ws/user");

    let mut client = PolymarketWebSocketClient::new_user(Some(ws_url), test_credential());
    client.connect().await.expect("connect failed");
    wait_until_active(&client, 2.0).await;

    client
        .subscribe_user()
        .await
        .expect("subscribe_user failed");

    wait_until_async(
        || {
            let authenticated = client.is_authenticated();
            async move { authenticated }
        },
        Duration::from_secs(2),
    )
    .await;

    assert!(
        client.is_authenticated(),
        "should be authenticated before disconnect"
    );

    client.disconnect().await.expect("disconnect failed");

    // disconnect() calls auth_tracker.invalidate()
    assert!(
        !client.is_authenticated(),
        "should not be authenticated after disconnect"
    );
}

#[rstest]
#[tokio::test]
async fn test_market_client_is_never_authenticated() {
    let state = Arc::new(TestServerState::default());
    let addr = start_ws_server(state.clone()).await;
    let ws_url = format!("ws://{addr}/ws/market");

    let mut client = PolymarketWebSocketClient::new_market(Some(ws_url), true);
    client.connect().await.expect("connect failed");
    wait_until_active(&client, 2.0).await;

    client
        .subscribe_market(vec![TEST_ASSET_ID.to_string()])
        .await
        .expect("subscribe failed");

    wait_until_async(
        || {
            let count = client.subscription_count();
            async move { count >= 1 }
        },
        Duration::from_secs(2),
    )
    .await;

    // Market channel does not use auth tracker
    assert!(!client.is_authenticated());

    client.disconnect().await.expect("disconnect failed");
}
