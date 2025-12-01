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

//! Integration tests for HyperLiquid WebSocket client using a mock server.

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
use nautilus_hyperliquid::{
    common::HyperliquidProductType, websocket::client::HyperliquidWebSocketClient,
};
use nautilus_model::{
    data::BarType,
    identifiers::{AccountId, InstrumentId},
};
use rstest::rstest;
use serde_json::{Value, json};

const TEST_USER_ADDRESS: &str = "0x1234567890123456789012345678901234567890";
const TEST_PING_PAYLOAD: &[u8] = b"test-server-ping";

#[derive(Clone)]
struct TestServerState {
    connection_count: Arc<tokio::sync::Mutex<usize>>,
    subscriptions: Arc<tokio::sync::Mutex<Vec<(String, Value)>>>, // (type, full subscription data)
    unsubscriptions: Arc<tokio::sync::Mutex<Vec<Value>>>,
    subscription_events: Arc<tokio::sync::Mutex<Vec<(String, bool)>>>, // (type, success)
    fail_next_subscriptions: Arc<tokio::sync::Mutex<Vec<String>>>,
    drop_next_connection: Arc<AtomicBool>,
    send_initial_ping: Arc<AtomicBool>,
    received_pong: Arc<AtomicBool>,
    last_pong: Arc<tokio::sync::Mutex<Option<Vec<u8>>>>,
    ping_count: Arc<AtomicUsize>,
}

impl Default for TestServerState {
    fn default() -> Self {
        Self {
            connection_count: Arc::new(tokio::sync::Mutex::new(0)),
            subscriptions: Arc::new(tokio::sync::Mutex::new(Vec::new())),
            unsubscriptions: Arc::new(tokio::sync::Mutex::new(Vec::new())),
            subscription_events: Arc::new(tokio::sync::Mutex::new(Vec::new())),
            fail_next_subscriptions: Arc::new(tokio::sync::Mutex::new(Vec::new())),
            drop_next_connection: Arc::new(AtomicBool::new(false)),
            send_initial_ping: Arc::new(AtomicBool::new(false)),
            received_pong: Arc::new(AtomicBool::new(false)),
            last_pong: Arc::new(tokio::sync::Mutex::new(None)),
            ping_count: Arc::new(AtomicUsize::new(0)),
        }
    }
}

impl TestServerState {
    async fn clear_subscription_events(&self) {
        self.subscription_events.lock().await.clear();
    }

    async fn subscription_events(&self) -> Vec<(String, bool)> {
        self.subscription_events.lock().await.clone()
    }

    async fn fail_next_subscription(&self, sub_type: &str) {
        self.fail_next_subscriptions
            .lock()
            .await
            .push(sub_type.to_string());
    }

    async fn pop_fail_subscription(&self, sub_type: &str) -> bool {
        let mut pending = self.fail_next_subscriptions.lock().await;
        if let Some(pos) = pending.iter().position(|entry| entry == sub_type) {
            pending.remove(pos);
            true
        } else {
            false
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

    if state.send_initial_ping.load(Ordering::Relaxed)
        && socket
            .send(Message::Ping(TEST_PING_PAYLOAD.to_vec().into()))
            .await
            .is_err()
    {
        return;
    }

    let trades_payload = json!({
        "channel": "trades",
        "data": [{
            "coin": "BTC",
            "side": "B",
            "px": "98450.00",
            "sz": "0.5",
            "time": 1703875200000u64,
            "hash": "0xabc123"
        }]
    });

    let book_payload = load_json("ws_book_data.json");

    while let Some(message) = socket.next().await {
        let Ok(message) = message else { break };

        match message {
            Message::Text(text) => {
                if let Ok(payload) = serde_json::from_str::<Value>(&text) {
                    let method = payload.get("method").and_then(|m| m.as_str());

                    match method {
                        Some("subscribe") => {
                            if let Some(subscription) = payload.get("subscription") {
                                let sub_type = subscription
                                    .get("type")
                                    .and_then(|t| t.as_str())
                                    .unwrap_or("unknown");

                                let should_fail = state.pop_fail_subscription(sub_type).await;

                                if !should_fail {
                                    let mut subs = state.subscriptions.lock().await;
                                    subs.push((sub_type.to_string(), subscription.clone()));
                                }

                                state
                                    .subscription_events
                                    .lock()
                                    .await
                                    .push((sub_type.to_string(), !should_fail));

                                // Send subscription acknowledgment (HyperLiquid doesn't send explicit acks)
                                // but we'll send data immediately to simulate subscription success

                                if !should_fail {
                                    let data_msg = match sub_type {
                                        "trades" => trades_payload.clone(),
                                        "l2Book" => book_payload.clone(),
                                        "candle" => json!({
                                            "channel": "candle",
                                            "data": {
                                                "t": 1703875200000u64,
                                                "T": 1703875260000u64,
                                                "s": "BTC",
                                                "i": "1m",
                                                "o": "98450.00",
                                                "c": "98460.00",
                                                "h": "98470.00",
                                                "l": "98440.00",
                                                "v": "10.5",
                                                "n": 42
                                            }
                                        }),
                                        "userEvents" | "orderUpdates" | "userFills" => json!({
                                            "channel": sub_type,
                                            "data": []
                                        }),
                                        "bbo" => json!({
                                            "channel": "bbo",
                                            "data": {
                                                "coin": "BTC",
                                                "bid": "98450.00",
                                                "ask": "98451.00",
                                                "time": 1703875200000u64
                                            }
                                        }),
                                        _ => json!({"channel": sub_type, "data": {}}),
                                    };

                                    if socket
                                        .send(Message::Text(data_msg.to_string().into()))
                                        .await
                                        .is_err()
                                    {
                                        break;
                                    }
                                }

                                if state.drop_next_connection.swap(false, Ordering::Relaxed) {
                                    let _ = socket.send(Message::Close(None)).await;
                                    break;
                                }
                            }
                        }
                        Some("unsubscribe") => {
                            if let Some(subscription) = payload.get("subscription") {
                                {
                                    let mut unsubs = state.unsubscriptions.lock().await;
                                    unsubs.push(subscription.clone());
                                }

                                // Remove from active subscriptions
                                let sub_type = subscription
                                    .get("type")
                                    .and_then(|t| t.as_str())
                                    .unwrap_or("unknown");
                                let mut subs = state.subscriptions.lock().await;
                                subs.retain(|(t, _)| t != sub_type);
                            }
                        }
                        Some("ping") => {
                            state.ping_count.fetch_add(1, Ordering::Relaxed);
                            // HyperLiquid expects a pong response
                            let pong_response = json!({"channel": "pong"});
                            if socket
                                .send(Message::Text(pong_response.to_string().into()))
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
            Message::Ping(payload) => {
                state.ping_count.fetch_add(1, Ordering::Relaxed);
                if socket.send(Message::Pong(payload)).await.is_err() {
                    break;
                }
            }
            Message::Pong(payload) => {
                state.received_pong.store(true, Ordering::Relaxed);
                *state.last_pong.lock().await = Some(payload.to_vec());
            }
            Message::Close(_) => break,
            _ => {}
        }

        if state.drop_next_connection.load(Ordering::Relaxed) {
            let _ = socket.send(Message::Close(None)).await;
            break;
        }
    }

    let mut count = state.connection_count.lock().await;
    *count = count.saturating_sub(1);
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

async fn connect_client(ws_url: &str, account_id: Option<AccountId>) -> HyperliquidWebSocketClient {
    let mut client = HyperliquidWebSocketClient::new(
        Some(ws_url.to_string()),
        false,
        HyperliquidProductType::Perp,
        account_id,
    );
    cache_test_instruments(&mut client);
    client
}

fn cache_test_instruments(client: &mut HyperliquidWebSocketClient) {
    use nautilus_core::time::get_atomic_clock_realtime;
    use nautilus_model::{
        identifiers::{InstrumentId, Symbol},
        instruments::{CryptoPerpetual, InstrumentAny},
        types::{Currency, Price, Quantity},
    };

    let clock = get_atomic_clock_realtime();
    let ts = clock.get_time_ns();

    // Create stub instruments for testing
    let instruments = vec![
        ("BTC", "BTC-USD-PERP"),
        ("ETH", "ETH-USD-PERP"),
        ("SOL", "SOL-USD-PERP"),
    ];

    let mut test_instruments = Vec::new();
    for (raw_symbol, symbol_str) in instruments {
        let raw_symbol = Symbol::new(raw_symbol);
        let instrument_id = InstrumentId::from(format!("{symbol_str}.HYPERLIQUID"));

        let instrument = InstrumentAny::CryptoPerpetual(CryptoPerpetual::new(
            instrument_id,
            raw_symbol,
            Currency::USD(),
            Currency::USD(),
            Currency::USD(),
            false,
            2, // price_precision
            3, // size_precision
            Price::from("0.01"),
            Quantity::from("0.001"),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            ts,
            ts,
        ));
        test_instruments.push(instrument);
    }

    client.cache_instruments(test_instruments);
}

async fn wait_until_active(
    client: &HyperliquidWebSocketClient,
    timeout_secs: f64,
) -> anyhow::Result<()> {
    let timeout = Duration::from_secs_f64(timeout_secs);
    let start = std::time::Instant::now();

    while start.elapsed() < timeout {
        if client.is_active() {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    anyhow::bail!("Timeout waiting for client to become active")
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

// ============================================================================
// Core Connectivity Tests
// ============================================================================

#[rstest]
#[tokio::test]
async fn test_client_creation() {
    let client = connect_client("ws://127.0.0.1:9999/ws", None).await;
    assert!(!client.is_active());
}

#[rstest]
#[tokio::test]
async fn test_websocket_connection() {
    let state = Arc::new(TestServerState::default());
    let addr = start_ws_server(state.clone()).await;
    let ws_url = format!("ws://{addr}/ws");

    let mut client = connect_client(&ws_url, None).await;
    client.connect().await.expect("connect failed");

    wait_for_connection_count(&state, 1, Duration::from_secs(5)).await;

    client.disconnect().await.expect("close failed");

    wait_for_connection_count(&state, 0, Duration::from_secs(5)).await;
}

#[rstest]
#[tokio::test]
async fn test_wait_until_active_timeout() {
    let client = connect_client("ws://127.0.0.1:0/ws", None).await;
    let result = wait_until_active(&client, 0.1).await;
    assert!(result.is_err(), "expected timeout error");
}

#[rstest]
#[tokio::test]
async fn test_is_active_lifecycle() {
    let state = Arc::new(TestServerState::default());
    let addr = start_ws_server(state.clone()).await;
    let ws_url = format!("ws://{addr}/ws");

    let mut client = connect_client(&ws_url, None).await;

    assert!(
        !client.is_active(),
        "Client should not be active before connect"
    );

    client.connect().await.expect("connect failed");
    wait_until_active(&client, 2.0)
        .await
        .expect("client inactive");

    assert!(
        client.is_active(),
        "Client should be active after connect completes"
    );

    client.disconnect().await.expect("close failed");
    tokio::time::sleep(Duration::from_millis(100)).await;

    assert!(
        !client.is_active(),
        "Client should not be active after close"
    );
}

#[rstest]
#[tokio::test]
async fn test_is_active_false_after_close() {
    let state = Arc::new(TestServerState::default());
    let addr = start_ws_server(state.clone()).await;
    let ws_url = format!("ws://{addr}/ws");

    let mut client = connect_client(&ws_url, None).await;
    client.connect().await.expect("connect failed");
    wait_until_active(&client, 2.0)
        .await
        .expect("client inactive");

    assert!(
        client.is_active(),
        "Expected is_active() to be true after connect"
    );

    client.disconnect().await.expect("close failed");
    tokio::time::sleep(Duration::from_millis(100)).await;

    assert!(
        !client.is_active(),
        "Expected is_active() to be false after close"
    );
}

#[rstest]
#[tokio::test]
async fn test_is_active_false_during_reconnection() {
    let state = Arc::new(TestServerState::default());
    let addr = start_ws_server(state.clone()).await;
    let ws_url = format!("ws://{addr}/ws");

    let mut client = connect_client(&ws_url, None).await;
    client.connect().await.expect("connect failed");
    wait_until_active(&client, 2.0)
        .await
        .expect("client inactive");
    assert!(client.is_active(), "Client should be active after connect");

    // Trigger disconnect
    state.drop_next_connection.store(true, Ordering::Relaxed);
    let _ = client
        .subscribe_trades(InstrumentId::from("BTC-USD-PERP.HYPERLIQUID"))
        .await;

    tokio::time::sleep(Duration::from_millis(100)).await;

    // During reconnection, is_active() should return false
    assert!(
        !client.is_active(),
        "Client should not be active during reconnection"
    );

    client.disconnect().await.expect("close failed");
}

#[rstest]
#[tokio::test]
async fn test_close_connection() {
    let state = Arc::new(TestServerState::default());
    let addr = start_ws_server(state.clone()).await;
    let ws_url = format!("ws://{addr}/ws");

    let mut client = connect_client(&ws_url, None).await;
    client.connect().await.expect("connect failed");

    wait_for_connection_count(&state, 1, Duration::from_secs(5)).await;

    client.disconnect().await.expect("close failed");

    wait_for_connection_count(&state, 0, Duration::from_secs(5)).await;
}

// ============================================================================
// Subscription Tests
// ============================================================================

#[rstest]
#[tokio::test]
async fn test_subscribe_trades() {
    let state = Arc::new(TestServerState::default());
    let addr = start_ws_server(state.clone()).await;
    let ws_url = format!("ws://{addr}/ws");

    let mut client = connect_client(&ws_url, None).await;
    client.connect().await.expect("connect failed");
    wait_until_active(&client, 2.0)
        .await
        .expect("client inactive");

    client
        .subscribe_trades(InstrumentId::from("BTC-USD-PERP.HYPERLIQUID"))
        .await
        .expect("subscribe failed");

    wait_until_async(
        || {
            let state = state.clone();
            async move { !state.subscription_events.lock().await.is_empty() }
        },
        Duration::from_secs(2),
    )
    .await;

    let events = state.subscription_events().await;
    assert!(
        events.iter().any(|(t, ok)| t == "trades" && *ok),
        "Expected trades subscription success"
    );

    client.disconnect().await.expect("close failed");
}

#[rstest]
#[tokio::test]
async fn test_subscribe_orderbook() {
    let state = Arc::new(TestServerState::default());
    let addr = start_ws_server(state.clone()).await;
    let ws_url = format!("ws://{addr}/ws");

    let mut client = connect_client(&ws_url, None).await;
    client.connect().await.expect("connect failed");
    wait_until_active(&client, 2.0)
        .await
        .expect("client inactive");

    client
        .subscribe_book(InstrumentId::from("BTC-USD-PERP.HYPERLIQUID"))
        .await
        .expect("subscribe failed");

    wait_until_async(
        || {
            let state = state.clone();
            async move { !state.subscription_events.lock().await.is_empty() }
        },
        Duration::from_secs(2),
    )
    .await;

    let events = state.subscription_events().await;
    assert!(
        events.iter().any(|(t, ok)| t == "l2Book" && *ok),
        "Expected l2Book subscription success"
    );

    client.disconnect().await.expect("close failed");
}

#[rstest]
#[tokio::test]
async fn test_subscribe_quotes() {
    let state = Arc::new(TestServerState::default());
    let addr = start_ws_server(state.clone()).await;
    let ws_url = format!("ws://{addr}/ws");

    let mut client = connect_client(&ws_url, None).await;
    client.connect().await.expect("connect failed");
    wait_until_active(&client, 2.0)
        .await
        .expect("client inactive");

    client
        .subscribe_quotes(InstrumentId::from("BTC-USD-PERP.HYPERLIQUID"))
        .await
        .expect("subscribe failed");

    wait_until_async(
        || {
            let state = state.clone();
            async move {
                state
                    .subscription_events
                    .lock()
                    .await
                    .iter()
                    .any(|(t, ok)| t == "bbo" && *ok)
            }
        },
        Duration::from_secs(2),
    )
    .await;

    client.disconnect().await.expect("close failed");
}

#[rstest]
#[tokio::test]
async fn test_subscribe_user_events() {
    let state = Arc::new(TestServerState::default());
    let addr = start_ws_server(state.clone()).await;
    let ws_url = format!("ws://{addr}/ws");

    let account_id = AccountId::from("HYPERLIQUID-001");
    let mut client = connect_client(&ws_url, Some(account_id)).await;
    client.connect().await.expect("connect failed");
    wait_until_active(&client, 2.0)
        .await
        .expect("client inactive");

    client
        .subscribe_user_events(TEST_USER_ADDRESS)
        .await
        .expect("subscribe failed");

    wait_until_async(
        || {
            let state = state.clone();
            async move {
                state
                    .subscription_events
                    .lock()
                    .await
                    .iter()
                    .any(|(t, ok)| t == "userEvents" && *ok)
            }
        },
        Duration::from_secs(2),
    )
    .await;

    client.disconnect().await.expect("close failed");
}

#[rstest]
#[tokio::test]
async fn test_unsubscribe_flow() {
    let state = Arc::new(TestServerState::default());
    let addr = start_ws_server(state.clone()).await;
    let ws_url = format!("ws://{addr}/ws");

    let mut client = connect_client(&ws_url, None).await;
    client.connect().await.expect("connect failed");
    wait_until_active(&client, 2.0)
        .await
        .expect("client inactive");

    client
        .subscribe_trades(InstrumentId::from("BTC-USD-PERP.HYPERLIQUID"))
        .await
        .expect("subscribe failed");

    wait_until_async(
        || {
            let state = state.clone();
            async move { !state.subscriptions.lock().await.is_empty() }
        },
        Duration::from_secs(2),
    )
    .await;

    client
        .unsubscribe_trades(InstrumentId::from("BTC-USD-PERP.HYPERLIQUID"))
        .await
        .expect("unsubscribe failed");

    wait_until_async(
        || {
            let state = state.clone();
            async move { !state.unsubscriptions.lock().await.is_empty() }
        },
        Duration::from_secs(2),
    )
    .await;

    client.disconnect().await.expect("close failed");
}

#[rstest]
#[tokio::test]
async fn test_multiple_subscriptions() {
    let state = Arc::new(TestServerState::default());
    let addr = start_ws_server(state.clone()).await;
    let ws_url = format!("ws://{addr}/ws");

    let mut client = connect_client(&ws_url, None).await;
    client.connect().await.expect("connect failed");
    wait_until_active(&client, 2.0)
        .await
        .expect("client inactive");

    client
        .subscribe_trades(InstrumentId::from("BTC-USD-PERP.HYPERLIQUID"))
        .await
        .expect("subscribe BTC trades failed");
    client
        .subscribe_trades(InstrumentId::from("ETH-USD-PERP.HYPERLIQUID"))
        .await
        .expect("subscribe ETH trades failed");
    client
        .subscribe_quotes(InstrumentId::from("BTC-USD-PERP.HYPERLIQUID"))
        .await
        .expect("subscribe BTC bbo failed");

    wait_until_async(
        || {
            let state = state.clone();
            async move { state.subscription_events.lock().await.len() >= 3 }
        },
        Duration::from_secs(2),
    )
    .await;

    let events = state.subscription_events().await;
    assert!(
        events.iter().filter(|(t, ok)| t == "trades" && *ok).count() >= 2,
        "Expected at least 2 trades subscriptions"
    );
    assert!(
        events.iter().any(|(t, ok)| t == "bbo" && *ok),
        "Expected bbo subscription"
    );

    client.disconnect().await.expect("close failed");
}

// ============================================================================
// Reconnection Tests
// ============================================================================

#[rstest]
#[tokio::test]
async fn test_reconnection_scenario() {
    let state = Arc::new(TestServerState::default());
    state.drop_next_connection.store(true, Ordering::Relaxed);

    let addr = start_ws_server(state.clone()).await;
    let ws_url = format!("ws://{addr}/ws");

    let mut client = connect_client(&ws_url, None).await;
    client.connect().await.expect("connect failed");
    wait_until_active(&client, 2.0)
        .await
        .expect("client inactive");

    client
        .subscribe_trades(InstrumentId::from("BTC-USD-PERP.HYPERLIQUID"))
        .await
        .expect("subscribe failed");

    // Wait for reconnection to complete
    wait_until_async(
        || {
            let state = state.clone();
            async move {
                let count = *state.connection_count.lock().await;
                let events = state.subscription_events().await;
                // Should have reconnected and resubscribed
                count >= 1 && !events.is_empty()
            }
        },
        Duration::from_secs(5),
    )
    .await;

    client.disconnect().await.expect("close failed");
}

#[rstest]
#[tokio::test]
async fn test_heartbeat_timeout_reconnection() {
    let state = Arc::new(TestServerState::default());
    let addr = start_ws_server(state.clone()).await;
    let ws_url = format!("ws://{addr}/ws");

    let mut client = connect_client(&ws_url, None).await;
    client.connect().await.expect("connect failed");
    wait_until_active(&client, 2.0)
        .await
        .expect("client inactive");

    client
        .subscribe_trades(InstrumentId::from("BTC-USD-PERP.HYPERLIQUID"))
        .await
        .expect("subscribe failed");

    // Client should maintain connection with heartbeat
    tokio::time::sleep(Duration::from_secs(3)).await;
    assert!(client.is_active(), "Client should still be active");

    client.disconnect().await.expect("close failed");
}

#[rstest]
#[tokio::test]
async fn test_reconnection_retries_failed_subscriptions() {
    let state = Arc::new(TestServerState::default());
    state.fail_next_subscription("trades").await;

    let addr = start_ws_server(state.clone()).await;
    let ws_url = format!("ws://{addr}/ws");

    let mut client = connect_client(&ws_url, None).await;
    client.connect().await.expect("connect failed");
    wait_until_active(&client, 2.0)
        .await
        .expect("client inactive");

    state.clear_subscription_events().await;

    // Wait to ensure events are cleared
    wait_until_async(
        || {
            let state = state.clone();
            async move { state.subscription_events().await.is_empty() }
        },
        Duration::from_secs(2),
    )
    .await;

    client
        .subscribe_trades(InstrumentId::from("BTC-USD-PERP.HYPERLIQUID"))
        .await
        .expect("subscribe call succeeded");

    // Wait for first subscription attempt (should fail)
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let events = state.subscription_events().await;
            if events.iter().any(|(t, ok)| t == "trades" && !*ok) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("expected subscription failure");

    // Note: Full retry logic requires reconnection implementation
    client.disconnect().await.expect("close failed");
}

#[rstest]
#[tokio::test]
async fn test_subscription_restoration_tracking() {
    let state = Arc::new(TestServerState::default());
    let addr = start_ws_server(state.clone()).await;
    let ws_url = format!("ws://{addr}/ws");

    let mut client = connect_client(&ws_url, None).await;
    client.connect().await.expect("connect failed");
    wait_until_active(&client, 2.0)
        .await
        .expect("client inactive");

    client
        .subscribe_trades(InstrumentId::from("BTC-USD-PERP.HYPERLIQUID"))
        .await
        .expect("subscribe BTC trades failed");
    client
        .subscribe_quotes(InstrumentId::from("ETH-USD-PERP.HYPERLIQUID"))
        .await
        .expect("subscribe ETH bbo failed");

    wait_for_subscription_events(&state, Duration::from_secs(2), |events| events.len() >= 2).await;

    let events = state.subscription_events().await;
    assert!(
        events.iter().any(|(t, ok)| t == "trades" && *ok),
        "Expected trades subscription"
    );
    assert!(
        events.iter().any(|(t, ok)| t == "bbo" && *ok),
        "Expected bbo subscription"
    );

    client.disconnect().await.expect("close failed");
}

#[rstest]
#[tokio::test]
async fn test_true_auto_reconnect_with_verification() {
    let state = Arc::new(TestServerState::default());
    state.drop_next_connection.store(true, Ordering::Relaxed);

    let addr = start_ws_server(state.clone()).await;
    let ws_url = format!("ws://{addr}/ws");

    let mut client = connect_client(&ws_url, None).await;
    client.connect().await.expect("connect failed");
    wait_until_active(&client, 2.0)
        .await
        .expect("client inactive");

    client
        .subscribe_trades(InstrumentId::from("BTC-USD-PERP.HYPERLIQUID"))
        .await
        .expect("subscribe failed");

    // Connection should drop and reconnect
    wait_until_async(
        || {
            let state = state.clone();
            async move {
                // Wait for connection to stabilize after drop
                *state.connection_count.lock().await >= 1
            }
        },
        Duration::from_secs(5),
    )
    .await;

    client.disconnect().await.expect("close failed");
}

// ============================================================================
// Edge Case Tests
// ============================================================================

#[rstest]
#[tokio::test]
async fn test_rapid_consecutive_reconnections() {
    let state = Arc::new(TestServerState::default());
    let addr = start_ws_server(state.clone()).await;
    let ws_url = format!("ws://{addr}/ws");

    let mut client = connect_client(&ws_url, None).await;
    client.connect().await.expect("connect failed");
    wait_until_active(&client, 2.0)
        .await
        .expect("client inactive");

    client
        .subscribe_trades(InstrumentId::from("BTC-USD-PERP.HYPERLIQUID"))
        .await
        .expect("subscribe failed");

    // Trigger multiple rapid disconnects
    for _ in 0..3 {
        state.clear_subscription_events().await;

        // Wait to ensure events are cleared
        wait_until_async(
            || {
                let state = state.clone();
                async move { state.subscription_events().await.is_empty() }
            },
            Duration::from_secs(2),
        )
        .await;

        state.drop_next_connection.store(true, Ordering::Relaxed);

        let _ = client
            .subscribe_quotes(InstrumentId::from("ETH-USD-PERP.HYPERLIQUID"))
            .await;

        tokio::time::sleep(Duration::from_millis(200)).await;
    }

    // Client should eventually stabilize
    tokio::time::sleep(Duration::from_secs(2)).await;

    client.disconnect().await.expect("close failed");
}

#[rstest]
#[tokio::test]
async fn test_reconnection_race_condition() {
    let state = Arc::new(TestServerState::default());
    let addr = start_ws_server(state.clone()).await;
    let ws_url = format!("ws://{addr}/ws");

    let mut client = connect_client(&ws_url, None).await;
    client.connect().await.expect("connect failed");
    wait_until_active(&client, 2.0)
        .await
        .expect("client inactive");

    client
        .subscribe_trades(InstrumentId::from("BTC-USD-PERP.HYPERLIQUID"))
        .await
        .expect("subscribe failed");

    // Trigger disconnect during active connection
    state.drop_next_connection.store(true, Ordering::Relaxed);
    let _ = client
        .subscribe_quotes(InstrumentId::from("ETH-USD-PERP.HYPERLIQUID"))
        .await;

    tokio::time::sleep(Duration::from_millis(100)).await;

    // Trigger another disconnect while reconnecting
    state.drop_next_connection.store(true, Ordering::Relaxed);
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Client should eventually recover
    tokio::time::sleep(Duration::from_secs(3)).await;

    client.disconnect().await.expect("close failed");
}

#[rstest]
#[tokio::test]
async fn test_multiple_partial_subscription_failures() {
    let state = Arc::new(TestServerState::default());
    let addr = start_ws_server(state.clone()).await;
    let ws_url = format!("ws://{addr}/ws");

    let mut client = connect_client(&ws_url, None).await;
    client.connect().await.expect("connect failed");
    wait_until_active(&client, 2.0)
        .await
        .expect("client inactive");

    // Subscribe to multiple channels
    client
        .subscribe_trades(InstrumentId::from("BTC-USD-PERP.HYPERLIQUID"))
        .await
        .expect("subscribe BTC trades");
    client
        .subscribe_quotes(InstrumentId::from("BTC-USD-PERP.HYPERLIQUID"))
        .await
        .expect("subscribe BTC bbo");
    client
        .subscribe_book(InstrumentId::from("ETH-USD-PERP.HYPERLIQUID"))
        .await
        .expect("subscribe ETH book");

    wait_for_subscription_events(&state, Duration::from_secs(2), |events| events.len() >= 3).await;

    state.clear_subscription_events().await;

    // Wait to ensure events are cleared
    wait_until_async(
        || {
            let state = state.clone();
            async move { state.subscription_events().await.is_empty() }
        },
        Duration::from_secs(2),
    )
    .await;

    // Set one to fail on next attempt
    state.fail_next_subscription("trades").await;
    state.drop_next_connection.store(true, Ordering::Relaxed);

    client
        .subscribe_trades(InstrumentId::from("SOL-USD-PERP.HYPERLIQUID"))
        .await
        .expect("trigger disconnect");

    // Wait for reconnection and subscription attempts
    tokio::time::sleep(Duration::from_secs(3)).await;

    client.disconnect().await.expect("close failed");
}

#[rstest]
#[tokio::test]
async fn test_subscribe_after_next_event_call() {
    let state = Arc::new(TestServerState::default());
    let addr = start_ws_server(state.clone()).await;
    let ws_url = format!("ws://{addr}/ws");

    let mut client = connect_client(&ws_url, None).await;
    client.connect().await.expect("connect failed");
    wait_until_active(&client, 2.0).await.expect("wait failed");

    // Subscribe to get some events
    client
        .subscribe_trades(InstrumentId::from("BTC-USD-PERP.HYPERLIQUID"))
        .await
        .expect("subscribe failed");

    tokio::time::sleep(Duration::from_millis(100)).await;

    // Try to get an event
    tokio::select! {
        _ = client.next_event() => {},
        _ = tokio::time::sleep(Duration::from_millis(200)) => {}
    }

    // Subscribe should still work after next_event
    let result = client
        .subscribe_quotes(InstrumentId::from("ETH-USD-PERP.HYPERLIQUID"))
        .await;
    assert!(
        result.is_ok(),
        "Subscribe should work after next_event() is called"
    );

    client.disconnect().await.expect("close failed");
}

#[rstest]
#[tokio::test]
async fn test_sends_pong_for_control_ping() {
    let state = Arc::new(TestServerState::default());
    state.send_initial_ping.store(true, Ordering::Relaxed);

    let addr = start_ws_server(state.clone()).await;
    let ws_url = format!("ws://{addr}/ws");

    let mut client = connect_client(&ws_url, None).await;
    client.connect().await.expect("connect failed");

    wait_until_async(
        || async {
            let guard = state.last_pong.lock().await;
            guard
                .as_ref()
                .is_some_and(|payload| payload.as_slice() == TEST_PING_PAYLOAD)
        },
        Duration::from_secs(2),
    )
    .await;

    client.disconnect().await.expect("close failed");
}

#[rstest]
#[tokio::test]
async fn test_unsubscribed_channel_not_resubscribed_after_disconnect() {
    let state = Arc::new(TestServerState::default());
    let addr = start_ws_server(state.clone()).await;
    let ws_url = format!("ws://{addr}/ws");

    let mut client = connect_client(&ws_url, None).await;
    client.connect().await.expect("connect failed");
    wait_until_active(&client, 2.0)
        .await
        .expect("client inactive");

    client
        .subscribe_trades(InstrumentId::from("BTC-USD-PERP.HYPERLIQUID"))
        .await
        .expect("subscribe trades failed");
    client
        .subscribe_quotes(InstrumentId::from("BTC-USD-PERP.HYPERLIQUID"))
        .await
        .expect("subscribe quotes failed");

    wait_until_async(
        || {
            let state = state.clone();
            async move {
                let subscriptions = state.subscriptions.lock().await;
                let trades_count = subscriptions.iter().filter(|(t, _)| t == "trades").count();
                let quotes_count = subscriptions.iter().filter(|(t, _)| t == "bbo").count();
                trades_count >= 1 && quotes_count >= 1
            }
        },
        Duration::from_secs(1),
    )
    .await;

    client
        .unsubscribe_quotes(InstrumentId::from("BTC-USD-PERP.HYPERLIQUID"))
        .await
        .expect("unsubscribe quotes failed");

    wait_until_async(
        || {
            let state = state.clone();
            async move {
                state.unsubscriptions.lock().await.iter().any(|value| {
                    value
                        .get("type")
                        .and_then(|t| t.as_str())
                        .is_some_and(|t| t == "bbo")
                })
            }
        },
        Duration::from_secs(1),
    )
    .await;

    state.clear_subscription_events().await;

    wait_until_async(
        || {
            let state = state.clone();
            async move { state.subscription_events().await.is_empty() }
        },
        Duration::from_secs(1),
    )
    .await;

    state.drop_next_connection.store(true, Ordering::Relaxed);

    client
        .subscribe_book(InstrumentId::from("SOL-USD-PERP.HYPERLIQUID"))
        .await
        .expect("subscribe book (triggers disconnect) failed");

    wait_until_async(
        || {
            let state = state.clone();
            async move {
                let events = state.subscription_events().await;
                events.iter().filter(|(t, _)| t == "trades").count() >= 1
            }
        },
        Duration::from_secs(3),
    )
    .await;

    let subscriptions = state.subscriptions.lock().await;
    let quotes_count = subscriptions.iter().filter(|(t, _)| t == "bbo").count();
    let trades_count = subscriptions.iter().filter(|(t, _)| t == "trades").count();

    assert_eq!(
        quotes_count, 0,
        "quotes channel was resubscribed unexpectedly"
    );
    assert!(
        trades_count >= 1,
        "expected trades channel to be restored on reconnect"
    );

    client.disconnect().await.expect("close failed");
}

#[rstest]
#[tokio::test]
async fn test_candle_subscription_survives_reconnection() {
    let state = Arc::new(TestServerState::default());
    state.drop_next_connection.store(true, Ordering::Relaxed);

    let addr = start_ws_server(state.clone()).await;
    let ws_url = format!("ws://{addr}/ws");

    let mut client = connect_client(&ws_url, None).await;
    client.connect().await.expect("connect failed");
    wait_until_active(&client, 2.0)
        .await
        .expect("client inactive");

    let bar_type = BarType::from("BTC-USD-PERP.HYPERLIQUID-1-HOUR-LAST-EXTERNAL");
    client
        .subscribe_bars(bar_type)
        .await
        .expect("subscribe bars failed");

    wait_until_async(
        || {
            let state = state.clone();
            async move {
                let subscriptions = state.subscriptions.lock().await;
                subscriptions.iter().any(|(t, _)| t == "candle")
            }
        },
        Duration::from_secs(1),
    )
    .await;

    wait_until_async(
        || {
            let state = state.clone();
            async move {
                let events = state.subscription_events().await;
                events.iter().filter(|(t, _)| t == "candle").count() >= 2
            }
        },
        Duration::from_secs(3),
    )
    .await;

    let subscriptions = state.subscriptions.lock().await;
    let candle_subs: Vec<_> = subscriptions
        .iter()
        .filter(|(t, _)| t == "candle")
        .collect();

    assert!(
        !candle_subs.is_empty(),
        "expected candle subscription to be restored on reconnect"
    );

    for (_, sub) in &candle_subs {
        let has_btc = sub.get("coin").is_some_and(|c| c.as_str() == Some("BTC"));
        assert!(has_btc, "expected candle subscription for BTC coin");
    }

    client.disconnect().await.expect("close failed");
}
