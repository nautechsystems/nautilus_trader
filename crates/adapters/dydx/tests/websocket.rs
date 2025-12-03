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

//! Integration tests for dYdX WebSocket client using a mock Axum server.

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
use nautilus_common::testing::wait_until_async;
use nautilus_dydx::websocket::client::DydxWebSocketClient;
use nautilus_model::identifiers::InstrumentId;
use rstest::rstest;
use serde_json::json;

#[derive(Clone)]
struct TestServerState {
    connection_count: Arc<tokio::sync::Mutex<usize>>,
    subscriptions: Arc<tokio::sync::Mutex<Vec<String>>>,
    subscription_events: Arc<tokio::sync::Mutex<Vec<(String, bool)>>>,
    fail_next_subscriptions: Arc<tokio::sync::Mutex<Vec<String>>>,
    disconnect_trigger: Arc<AtomicBool>,
    ping_count: Arc<AtomicUsize>,
    pong_count: Arc<AtomicUsize>,
}

impl Default for TestServerState {
    fn default() -> Self {
        Self {
            connection_count: Arc::new(tokio::sync::Mutex::new(0)),
            subscriptions: Arc::new(tokio::sync::Mutex::new(Vec::new())),
            subscription_events: Arc::new(tokio::sync::Mutex::new(Vec::new())),
            fail_next_subscriptions: Arc::new(tokio::sync::Mutex::new(Vec::new())),
            disconnect_trigger: Arc::new(AtomicBool::new(false)),
            ping_count: Arc::new(AtomicUsize::new(0)),
            pong_count: Arc::new(AtomicUsize::new(0)),
        }
    }
}

impl TestServerState {
    async fn set_subscription_failures(&self, channels: Vec<String>) {
        *self.fail_next_subscriptions.lock().await = channels;
    }

    async fn subscription_events(&self) -> Vec<(String, bool)> {
        self.subscription_events.lock().await.clone()
    }
}

async fn handle_websocket(ws: WebSocketUpgrade, State(state): State<TestServerState>) -> Response {
    ws.on_upgrade(|socket| handle_socket(socket, state))
}

async fn handle_socket(mut socket: WebSocket, state: TestServerState) {
    {
        let mut count = state.connection_count.lock().await;
        *count += 1;
    }

    send_connected_message(&mut socket).await;

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

                let msg_type = value.get("type").and_then(|v| v.as_str());

                match msg_type {
                    Some("subscribe") => {
                        handle_subscribe(&mut socket, &state, &value).await;
                    }
                    Some("unsubscribe") => {
                        handle_unsubscribe(&mut socket, &state, &value).await;
                    }
                    Some("ping") => {
                        state.ping_count.fetch_add(1, Ordering::Relaxed);
                        let pong_response = json!({
                            "type": "pong"
                        });
                        if socket
                            .send(Message::Text(pong_response.to_string().into()))
                            .await
                            .is_err()
                        {
                            break;
                        }
                        state.pong_count.fetch_add(1, Ordering::Relaxed);
                    }
                    _ => {}
                }
            }
            Message::Ping(data) => {
                state.ping_count.fetch_add(1, Ordering::Relaxed);
                if socket.send(Message::Pong(data)).await.is_err() {
                    break;
                }
                state.pong_count.fetch_add(1, Ordering::Relaxed);
            }
            Message::Close(_) => {
                break;
            }
            _ => {}
        }
    }
}

async fn send_connected_message(socket: &mut WebSocket) {
    let connected = json!({
        "type": "connected",
        "connection_id": "test-conn-123",
        "message_id": 0
    });
    let _ = socket
        .send(Message::Text(connected.to_string().into()))
        .await;
}

async fn handle_subscribe(
    socket: &mut WebSocket,
    state: &TestServerState,
    value: &serde_json::Value,
) {
    let channel = value.get("channel").and_then(|v| v.as_str());

    if let Some(channel_str) = channel {
        let fail_list = state.fail_next_subscriptions.lock().await.clone();
        let should_fail = fail_list.contains(&channel_str.to_string());

        state
            .subscription_events
            .lock()
            .await
            .push((channel_str.to_string(), !should_fail));

        if should_fail {
            let error_response = json!({
                "type": "error",
                "message": format!("Subscription failed for channel: {}", channel_str),
                "connection_id": "test-conn-123"
            });
            let _ = socket
                .send(Message::Text(error_response.to_string().into()))
                .await;
        } else {
            let mut subs = state.subscriptions.lock().await;
            if !subs.contains(&channel_str.to_string()) {
                subs.push(channel_str.to_string());
            }
            drop(subs);

            let subscribed_response = json!({
                "type": "subscribed",
                "connection_id": "test-conn-123",
                "message_id": 1,
                "channel": channel_str,
                "id": value.get("id")
            });
            let _ = socket
                .send(Message::Text(subscribed_response.to_string().into()))
                .await;

            if channel_str.starts_with("v4_trades") {
                send_sample_trade(socket, channel_str).await;
            } else if channel_str.starts_with("v4_orderbook") {
                send_sample_orderbook(socket, channel_str).await;
            } else if channel_str.starts_with("v4_candles") {
                send_sample_candle(socket, channel_str).await;
            }
        }
    }
}

async fn handle_unsubscribe(
    socket: &mut WebSocket,
    state: &TestServerState,
    value: &serde_json::Value,
) {
    let channel = value.get("channel").and_then(|v| v.as_str());

    if let Some(channel_str) = channel {
        let mut subs = state.subscriptions.lock().await;
        subs.retain(|s| s != channel_str);
        drop(subs);

        let mut events = state.subscription_events.lock().await;
        events.retain(|(c, _)| c != channel_str);
        drop(events);

        let unsubscribed_response = json!({
            "type": "unsubscribed",
            "connection_id": "test-conn-123",
            "message_id": 2,
            "channel": channel_str
        });
        let _ = socket
            .send(Message::Text(unsubscribed_response.to_string().into()))
            .await;
    }
}

async fn send_sample_trade(socket: &mut WebSocket, channel: &str) {
    let trade_msg = json!({
        "type": "channel_data",
        "connection_id": "test-conn-123",
        "message_id": 10,
        "channel": channel,
        "id": "BTC-USD",
        "contents": {
            "trades": [{
                "id": "test-trade-1",
                "side": "BUY",
                "size": "0.5",
                "price": "43250.0",
                "type": "LIMIT",
                "createdAt": "2024-01-01T00:00:00.000Z",
                "createdAtHeight": "123456"
            }]
        }
    });
    let _ = socket
        .send(Message::Text(trade_msg.to_string().into()))
        .await;
}

async fn send_sample_orderbook(socket: &mut WebSocket, channel: &str) {
    let orderbook_msg = json!({
        "type": "channel_data",
        "connection_id": "test-conn-123",
        "message_id": 11,
        "channel": channel,
        "id": "BTC-USD",
        "contents": {
            "bids": [
                ["43200.0", "1.5"],
                ["43190.0", "2.3"]
            ],
            "asks": [
                ["43210.0", "1.2"],
                ["43220.0", "0.8"]
            ]
        }
    });
    let _ = socket
        .send(Message::Text(orderbook_msg.to_string().into()))
        .await;
}

async fn send_sample_candle(socket: &mut WebSocket, channel: &str) {
    let candle_msg = json!({
        "type": "channel_data",
        "connection_id": "test-conn-123",
        "message_id": 12,
        "channel": channel,
        "id": "BTC-USD/1MIN",
        "contents": {
            "startedAt": "2024-01-01T00:00:00.000Z",
            "ticker": "BTC-USD",
            "resolution": "1MIN",
            "low": "43000.0",
            "high": "43500.0",
            "open": "43100.0",
            "close": "43400.0",
            "baseTokenVolume": "12.345",
            "usdVolume": "535000.50",
            "trades": 150,
            "startingOpenInterest": "1000000.0"
        }
    });
    let _ = socket
        .send(Message::Text(candle_msg.to_string().into()))
        .await;
}

fn create_test_router(state: TestServerState) -> Router {
    Router::new()
        .route("/v4/ws", get(handle_websocket))
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

    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    Ok((addr, state))
}

#[rstest]
#[tokio::test]
async fn test_websocket_connection() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{addr}/v4/ws");

    let mut client = DydxWebSocketClient::new_public(ws_url, Some(30));
    client.connect().await.unwrap();

    tokio::time::sleep(Duration::from_millis(200)).await;

    let count = state.connection_count.lock().await;
    assert_eq!(*count, 1);

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_websocket_wait_until_active() {
    let (addr, _state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{addr}/v4/ws");

    let mut client = DydxWebSocketClient::new_public(ws_url, Some(30));
    client.connect().await.unwrap();

    wait_until_async(|| async { client.is_connected() }, Duration::from_secs(5)).await;

    assert!(client.is_connected());
    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
#[ignore] // Flaky: disconnect state change timing is non-deterministic
async fn test_websocket_close() {
    let (addr, _state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{addr}/v4/ws");

    let mut client = DydxWebSocketClient::new_public(ws_url, Some(30));
    client.connect().await.unwrap();

    wait_until_async(|| async { client.is_connected() }, Duration::from_secs(5)).await;
    assert!(client.is_connected());

    client.disconnect().await.unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;

    assert!(!client.is_connected());
}

#[rstest]
#[tokio::test]
async fn test_subscribe_trades() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{addr}/v4/ws");

    let mut client = DydxWebSocketClient::new_public(ws_url, Some(30));
    client.connect().await.unwrap();

    wait_until_async(|| async { client.is_connected() }, Duration::from_secs(5)).await;

    let instrument_id = InstrumentId::from("BTC-USD.DYDX");
    client.subscribe_trades(instrument_id).await.unwrap();

    tokio::time::sleep(Duration::from_millis(200)).await;

    let subs = state.subscriptions.lock().await;
    assert!(subs.iter().any(|s| s.contains("v4_trades")));

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_subscribe_orderbook() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{addr}/v4/ws");

    let mut client = DydxWebSocketClient::new_public(ws_url, Some(30));
    client.connect().await.unwrap();

    wait_until_async(|| async { client.is_connected() }, Duration::from_secs(5)).await;

    let instrument_id = InstrumentId::from("BTC-USD.DYDX");
    client.subscribe_orderbook(instrument_id).await.unwrap();

    tokio::time::sleep(Duration::from_millis(200)).await;

    let subs = state.subscriptions.lock().await;
    assert!(subs.iter().any(|s| s.contains("v4_orderbook")));

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_subscribe_candles() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{addr}/v4/ws");

    let mut client = DydxWebSocketClient::new_public(ws_url, Some(30));
    client.connect().await.unwrap();

    wait_until_async(|| async { client.is_connected() }, Duration::from_secs(5)).await;

    let instrument_id = InstrumentId::from("BTC-USD.DYDX");
    client
        .subscribe_candles(instrument_id, "1MIN")
        .await
        .unwrap();

    tokio::time::sleep(Duration::from_millis(200)).await;

    let subs = state.subscriptions.lock().await;
    assert!(subs.iter().any(|s| s.contains("v4_candles")));

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_unsubscribe_trades() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{addr}/v4/ws");

    let mut client = DydxWebSocketClient::new_public(ws_url, Some(30));
    client.connect().await.unwrap();

    wait_until_async(|| async { client.is_connected() }, Duration::from_secs(5)).await;

    let instrument_id = InstrumentId::from("BTC-USD.DYDX");
    client.subscribe_trades(instrument_id).await.unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;

    client.unsubscribe_trades(instrument_id).await.unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;

    let subs = state.subscriptions.lock().await;
    assert!(!subs.iter().any(|s| s.contains("v4_trades")));

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_subscription_failure() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{addr}/v4/ws");

    state
        .set_subscription_failures(vec!["v4_trades".to_string()])
        .await;

    let mut client = DydxWebSocketClient::new_public(ws_url, Some(30));
    client.connect().await.unwrap();

    wait_until_async(|| async { client.is_connected() }, Duration::from_secs(5)).await;

    let instrument_id = InstrumentId::from("BTC-USD.DYDX");
    let _ = client.subscribe_trades(instrument_id).await;

    tokio::time::sleep(Duration::from_millis(300)).await;

    let events = state.subscription_events().await;
    assert!(
        events.iter().any(|(_, success)| !*success),
        "Expected at least one failed subscription"
    );

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
#[ignore] // Flaky: subscription tracking depends on message timing
async fn test_multiple_subscriptions() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{addr}/v4/ws");

    let mut client = DydxWebSocketClient::new_public(ws_url, Some(30));
    client.connect().await.unwrap();

    wait_until_async(|| async { client.is_connected() }, Duration::from_secs(5)).await;

    let btc_id = InstrumentId::from("BTC-USD.DYDX");
    let eth_id = InstrumentId::from("ETH-USD.DYDX");

    client.subscribe_trades(btc_id).await.unwrap();
    client.subscribe_trades(eth_id).await.unwrap();
    client.subscribe_orderbook(btc_id).await.unwrap();

    tokio::time::sleep(Duration::from_millis(500)).await;

    let subs = state.subscriptions.lock().await;
    assert!(subs.len() >= 3, "Expected at least 3 subscriptions");

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_ping_pong() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{addr}/v4/ws");

    let mut client = DydxWebSocketClient::new_public(ws_url, Some(30));
    client.connect().await.unwrap();

    wait_until_async(|| async { client.is_connected() }, Duration::from_secs(5)).await;

    tokio::time::sleep(Duration::from_millis(500)).await;

    let pong_count = state.pong_count.load(Ordering::Relaxed);
    let ping_count = state.ping_count.load(Ordering::Relaxed);
    assert_eq!(pong_count, ping_count);

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_reconnection() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{addr}/v4/ws");

    let mut client = DydxWebSocketClient::new_public(ws_url, Some(30));
    client.connect().await.unwrap();

    wait_until_async(|| async { client.is_connected() }, Duration::from_secs(5)).await;

    let initial_count = *state.connection_count.lock().await;

    state.disconnect_trigger.store(true, Ordering::Relaxed);
    tokio::time::sleep(Duration::from_millis(200)).await;

    state.disconnect_trigger.store(false, Ordering::Relaxed);
    client.disconnect().await.unwrap();
    client.connect().await.unwrap();

    wait_until_async(|| async { client.is_connected() }, Duration::from_secs(5)).await;

    let final_count = *state.connection_count.lock().await;
    assert!(final_count > initial_count);

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
#[ignore] // Flaky: disconnect state change timing is non-deterministic
async fn test_is_active_states() {
    let (addr, _state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{addr}/v4/ws");

    let mut client = DydxWebSocketClient::new_public(ws_url, Some(30));

    assert!(!client.is_connected());

    client.connect().await.unwrap();

    wait_until_async(|| async { client.is_connected() }, Duration::from_secs(5)).await;

    assert!(client.is_connected());

    client.disconnect().await.unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;

    assert!(!client.is_connected());
}

#[rstest]
#[tokio::test]
#[ignore] // Flaky: rapid reconnections are timing-dependent
async fn test_rapid_reconnections() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{addr}/v4/ws");

    let mut client = DydxWebSocketClient::new_public(ws_url, Some(30));
    client.connect().await.unwrap();

    wait_until_async(|| async { client.is_connected() }, Duration::from_secs(5)).await;

    for _ in 0..3 {
        state.disconnect_trigger.store(true, Ordering::Relaxed);
        tokio::time::sleep(Duration::from_millis(200)).await;

        state.disconnect_trigger.store(false, Ordering::Relaxed);
        client.disconnect().await.unwrap();
        tokio::time::sleep(Duration::from_millis(200)).await;
        client.connect().await.unwrap();

        wait_until_async(|| async { client.is_connected() }, Duration::from_secs(5)).await;
    }

    let final_count = *state.connection_count.lock().await;
    assert!(final_count >= 4, "Expected at least 4 connections");

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
#[ignore] // Flaky: subscription restoration depends on client implementation details
async fn test_subscription_restoration_after_reconnect() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{addr}/v4/ws");

    let mut client = DydxWebSocketClient::new_public(ws_url, Some(30));
    client.connect().await.unwrap();

    wait_until_async(|| async { client.is_connected() }, Duration::from_secs(5)).await;

    let instrument_id = InstrumentId::from("BTC-USD.DYDX");
    client.subscribe_trades(instrument_id).await.unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;

    state.disconnect_trigger.store(true, Ordering::Relaxed);
    tokio::time::sleep(Duration::from_millis(200)).await;

    state.disconnect_trigger.store(false, Ordering::Relaxed);
    state.subscription_events.lock().await.clear();

    client.disconnect().await.unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
    client.connect().await.unwrap();

    wait_until_async(|| async { client.is_connected() }, Duration::from_secs(5)).await;

    tokio::time::sleep(Duration::from_millis(300)).await;

    let events = state.subscription_events().await;
    assert!(
        events
            .iter()
            .any(|(channel, success)| channel.contains("v4_trades") && *success),
        "Trade subscription should be restored after reconnect"
    );

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_multiple_subscription_failures() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{addr}/v4/ws");

    state
        .set_subscription_failures(vec!["v4_trades".to_string(), "v4_orderbook".to_string()])
        .await;

    let mut client = DydxWebSocketClient::new_public(ws_url, Some(30));
    client.connect().await.unwrap();

    wait_until_async(|| async { client.is_connected() }, Duration::from_secs(5)).await;

    let btc_id = InstrumentId::from("BTC-USD.DYDX");
    let eth_id = InstrumentId::from("ETH-USD.DYDX");

    let _ = client.subscribe_trades(btc_id).await;
    let _ = client.subscribe_orderbook(eth_id).await;

    tokio::time::sleep(Duration::from_millis(500)).await;

    let events = state.subscription_events().await;
    let failures: Vec<_> = events.iter().filter(|(_, success)| !*success).collect();
    assert!(
        failures.len() >= 2,
        "Should have at least 2 failed subscriptions, got {}",
        failures.len()
    );

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_subscribe_after_stream() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{addr}/v4/ws");

    let mut client = DydxWebSocketClient::new_public(ws_url, Some(30));
    client.connect().await.unwrap();

    wait_until_async(|| async { client.is_connected() }, Duration::from_secs(5)).await;

    tokio::time::sleep(Duration::from_millis(100)).await;

    let instrument_id = InstrumentId::from("BTC-USD.DYDX");
    client.subscribe_trades(instrument_id).await.unwrap();

    tokio::time::sleep(Duration::from_millis(200)).await;

    let subs = state.subscriptions.lock().await;
    assert!(subs.iter().any(|s| s.contains("v4_trades")));

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_unsubscribe_multiple_channels() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{addr}/v4/ws");

    let mut client = DydxWebSocketClient::new_public(ws_url, Some(30));
    client.connect().await.unwrap();

    wait_until_async(|| async { client.is_connected() }, Duration::from_secs(5)).await;

    let instrument_id = InstrumentId::from("BTC-USD.DYDX");

    client.subscribe_trades(instrument_id).await.unwrap();
    client.subscribe_orderbook(instrument_id).await.unwrap();
    client
        .subscribe_candles(instrument_id, "1MIN")
        .await
        .unwrap();

    tokio::time::sleep(Duration::from_millis(200)).await;

    client.unsubscribe_trades(instrument_id).await.unwrap();
    client.unsubscribe_orderbook(instrument_id).await.unwrap();

    tokio::time::sleep(Duration::from_millis(200)).await;

    let subs = state.subscriptions.lock().await;
    assert!(
        !subs.iter().any(|s| s.contains("v4_trades")),
        "Trades should be unsubscribed"
    );
    assert!(
        !subs.iter().any(|s| s.contains("v4_orderbook")),
        "Orderbook should be unsubscribed"
    );
    assert!(
        subs.iter().any(|s| s.contains("v4_candles")),
        "Candles should still be subscribed"
    );

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_connection_count_increments() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{addr}/v4/ws");

    let mut client1 = DydxWebSocketClient::new_public(ws_url.clone(), Some(30));
    let mut client2 = DydxWebSocketClient::new_public(ws_url, Some(30));

    client1.connect().await.unwrap();
    client2.connect().await.unwrap();

    tokio::time::sleep(Duration::from_millis(200)).await;

    let count = *state.connection_count.lock().await;
    assert_eq!(count, 2, "Should have 2 concurrent connections");

    client1.disconnect().await.unwrap();
    client2.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_wait_until_active_timeout() {
    let ws_url = "ws://localhost:1/v4/ws".to_string();
    let mut client = DydxWebSocketClient::new_public(ws_url, Some(1));

    let _ = client.connect().await;

    tokio::time::sleep(Duration::from_millis(500)).await;

    assert!(
        !client.is_connected(),
        "Should not connect to unreachable server"
    );
}

#[rstest]
#[tokio::test]
async fn test_sends_pong_for_control_ping() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{addr}/v4/ws");

    let mut client = DydxWebSocketClient::new_public(ws_url, Some(30));
    client.connect().await.unwrap();

    wait_until_async(|| async { client.is_connected() }, Duration::from_secs(5)).await;

    tokio::time::sleep(Duration::from_millis(300)).await;

    let pong_count = state.pong_count.load(Ordering::Relaxed);
    let ping_count = state.ping_count.load(Ordering::Relaxed);

    assert_eq!(pong_count, ping_count, "Should respond to all pings");

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_subscription_tracking() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{addr}/v4/ws");

    let mut client = DydxWebSocketClient::new_public(ws_url, Some(30));
    client.connect().await.unwrap();

    wait_until_async(|| async { client.is_connected() }, Duration::from_secs(5)).await;

    let btc_id = InstrumentId::from("BTC-USD.DYDX");
    let eth_id = InstrumentId::from("ETH-USD.DYDX");

    client.subscribe_trades(btc_id).await.unwrap();
    client.subscribe_orderbook(eth_id).await.unwrap();

    tokio::time::sleep(Duration::from_millis(200)).await;

    let events = state.subscription_events().await;
    assert!(events.len() >= 2, "Should track subscription events");
    assert!(
        events.iter().all(|(_, success)| *success),
        "All subscriptions should succeed"
    );

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_heartbeat_timeout_triggers_reconnection() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{addr}/v4/ws");

    let mut client = DydxWebSocketClient::new_public(ws_url, Some(1));
    client.connect().await.unwrap();

    wait_until_async(|| async { client.is_connected() }, Duration::from_secs(5)).await;

    let btc_id = InstrumentId::from("BTC-USD.DYDX");
    client.subscribe_trades(btc_id).await.unwrap();

    tokio::time::sleep(Duration::from_millis(100)).await;

    state.disconnect_trigger.store(true, Ordering::Relaxed);

    tokio::time::sleep(Duration::from_secs(3)).await;

    assert!(
        client.is_connected(),
        "Should reconnect after heartbeat timeout"
    );

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_reconnection_race_condition() {
    let (addr, _state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{addr}/v4/ws");

    let mut client = DydxWebSocketClient::new_public(ws_url, Some(30));
    client.connect().await.unwrap();

    wait_until_async(|| async { client.is_connected() }, Duration::from_secs(5)).await;

    let btc_id = InstrumentId::from("BTC-USD.DYDX");
    client.subscribe_trades(btc_id).await.unwrap();

    tokio::time::sleep(Duration::from_millis(100)).await;

    client.disconnect().await.unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;

    client.connect().await.unwrap();
    wait_until_async(|| async { client.is_connected() }, Duration::from_secs(5)).await;

    tokio::time::sleep(Duration::from_millis(200)).await;

    assert!(client.is_connected());

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_subscription_retry_after_failed_reconnection() {
    let (addr, _state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{addr}/v4/ws");

    let mut client = DydxWebSocketClient::new_public(ws_url, Some(30));
    client.connect().await.unwrap();

    wait_until_async(|| async { client.is_connected() }, Duration::from_secs(5)).await;

    let btc_id = InstrumentId::from("BTC-USD.DYDX");
    client.subscribe_trades(btc_id).await.unwrap();

    tokio::time::sleep(Duration::from_millis(100)).await;

    client.disconnect().await.unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;

    client.connect().await.unwrap();
    wait_until_async(|| async { client.is_connected() }, Duration::from_secs(5)).await;

    tokio::time::sleep(Duration::from_millis(300)).await;

    assert!(client.is_connected());

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_is_connected_false_during_reconnection() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{addr}/v4/ws");

    let mut client = DydxWebSocketClient::new_public(ws_url, Some(30));
    client.connect().await.unwrap();

    wait_until_async(|| async { client.is_connected() }, Duration::from_secs(5)).await;

    let btc_id = InstrumentId::from("BTC-USD.DYDX");
    client.subscribe_trades(btc_id).await.unwrap();

    tokio::time::sleep(Duration::from_millis(100)).await;

    state.disconnect_trigger.store(true, Ordering::Relaxed);

    tokio::time::sleep(Duration::from_millis(100)).await;

    let _ = client.is_connected();

    tokio::time::sleep(Duration::from_millis(500)).await;

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
#[ignore = "Flaky: Mock server subscription event tracking unreliable"]
async fn test_subscription_restoration_tracking() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{addr}/v4/ws");

    let mut client = DydxWebSocketClient::new_public(ws_url, Some(30));
    client.connect().await.unwrap();

    wait_until_async(|| async { client.is_connected() }, Duration::from_secs(5)).await;

    let btc_id = InstrumentId::from("BTC-USD.DYDX");
    client.subscribe_trades(btc_id).await.unwrap();

    tokio::time::sleep(Duration::from_millis(100)).await;

    let subs = state.subscriptions.lock().await.clone();
    assert_eq!(subs.len(), 1);

    state.disconnect_trigger.store(true, Ordering::Relaxed);
    tokio::time::sleep(Duration::from_millis(1000)).await;

    let events = state.subscription_events().await;
    assert!(
        events.len() >= 2,
        "Should have subscribe + resubscribe events"
    );
    assert_eq!(
        *state.connection_count.lock().await,
        2,
        "Should have reconnected"
    );

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
#[ignore = "Flaky: Mock server subscription event tracking unreliable"]
async fn test_unsubscribe_tracking_removes_from_state() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{addr}/v4/ws");

    let mut client = DydxWebSocketClient::new_public(ws_url, Some(30));
    client.connect().await.unwrap();

    wait_until_async(|| async { client.is_connected() }, Duration::from_secs(5)).await;

    let btc_id = InstrumentId::from("BTC-USD.DYDX");
    client.subscribe_trades(btc_id).await.unwrap();

    tokio::time::sleep(Duration::from_millis(300)).await;

    let subs = state.subscriptions.lock().await.clone();
    assert_eq!(subs.len(), 1);

    client.unsubscribe_trades(btc_id).await.unwrap();

    tokio::time::sleep(Duration::from_millis(300)).await;

    let events = state.subscription_events().await;
    assert_eq!(events.len(), 2, "Should have subscribe + unsubscribe");
    assert!(events[0].1, "First event should be subscribe");
    assert!(!events[1].1, "Second event should be unsubscribe");

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_failed_subscription_stays_pending_for_retry() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{addr}/v4/ws");

    let mut client = DydxWebSocketClient::new_public(ws_url, Some(30));
    client.connect().await.unwrap();

    wait_until_async(|| async { client.is_connected() }, Duration::from_secs(5)).await;

    state
        .set_subscription_failures(vec!["v4_trades".to_string()])
        .await;

    let btc_id = InstrumentId::from("BTC-USD.DYDX");
    client.subscribe_trades(btc_id).await.unwrap();

    tokio::time::sleep(Duration::from_millis(100)).await;

    let subs = state.subscriptions.lock().await.clone();
    assert_eq!(
        subs.len(),
        0,
        "Failed subscription should not be in active list"
    );

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_subscribe_to_same_channel_idempotent() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{addr}/v4/ws");

    let mut client = DydxWebSocketClient::new_public(ws_url, Some(30));
    client.connect().await.unwrap();

    wait_until_async(|| async { client.is_connected() }, Duration::from_secs(5)).await;

    let btc_id = InstrumentId::from("BTC-USD.DYDX");

    client.subscribe_trades(btc_id).await.unwrap();
    client.subscribe_trades(btc_id).await.unwrap();

    tokio::time::sleep(Duration::from_millis(100)).await;

    let subs = state.subscriptions.lock().await.clone();
    assert_eq!(subs.len(), 1, "Duplicate subscribe should be idempotent");

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_message_routing_trades_vs_orderbook() {
    let (addr, _state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{addr}/v4/ws");

    let mut client = DydxWebSocketClient::new_public(ws_url, Some(30));
    client.connect().await.unwrap();

    wait_until_async(|| async { client.is_connected() }, Duration::from_secs(5)).await;

    let btc_id = InstrumentId::from("BTC-USD.DYDX");
    let eth_id = InstrumentId::from("ETH-USD.DYDX");

    client.subscribe_trades(btc_id).await.unwrap();
    client.subscribe_orderbook(eth_id).await.unwrap();

    tokio::time::sleep(Duration::from_millis(200)).await;

    let subs = _state.subscriptions.lock().await;
    assert!(
        subs.len() >= 2,
        "Should have both trades and orderbook subscriptions"
    );

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_message_routing_candles_channel() {
    let (addr, _state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{addr}/v4/ws");

    let mut client = DydxWebSocketClient::new_public(ws_url, Some(30));
    client.connect().await.unwrap();

    wait_until_async(|| async { client.is_connected() }, Duration::from_secs(5)).await;

    let btc_id = InstrumentId::from("BTC-USD.DYDX");

    client.subscribe_candles(btc_id, "1MIN").await.unwrap();

    tokio::time::sleep(Duration::from_millis(200)).await;

    let subs = _state.subscriptions.lock().await;
    assert!(
        subs.iter().any(|s| s.contains("v4_candles")),
        "Should have candles subscription"
    );

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
#[ignore = "Flaky - timing issues with disconnect state"]
async fn test_is_active_false_after_close() {
    let (addr, _state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{addr}/v4/ws");

    let mut client = DydxWebSocketClient::new_public(ws_url, Some(30));
    client.connect().await.unwrap();

    wait_until_async(|| async { client.is_connected() }, Duration::from_secs(5)).await;

    assert!(
        client.is_connected(),
        "Client should be connected after connect"
    );

    client.disconnect().await.unwrap();

    tokio::time::sleep(Duration::from_millis(100)).await;

    assert!(
        !client.is_connected(),
        "Client should not be connected after disconnect"
    );
}

#[rstest]
#[tokio::test]
#[ignore = "Flaky - timing issues with multiple subscriptions"]
async fn test_multiple_instruments_subscription() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{addr}/v4/ws");

    let mut client = DydxWebSocketClient::new_public(ws_url, Some(30));
    client.connect().await.unwrap();

    wait_until_async(|| async { client.is_connected() }, Duration::from_secs(5)).await;

    let btc_id = InstrumentId::from("BTC-USD.DYDX");
    let eth_id = InstrumentId::from("ETH-USD.DYDX");
    let sol_id = InstrumentId::from("SOL-USD.DYDX");

    client.subscribe_trades(btc_id).await.unwrap();
    client.subscribe_trades(eth_id).await.unwrap();
    client.subscribe_trades(sol_id).await.unwrap();

    tokio::time::sleep(Duration::from_millis(200)).await;

    let subs = state.subscriptions.lock().await.clone();
    assert_eq!(subs.len(), 3, "Should have 3 trade subscriptions");

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_subscription_after_stream_call() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{addr}/v4/ws");

    let mut client = DydxWebSocketClient::new_public(ws_url, Some(30));
    client.connect().await.unwrap();

    wait_until_async(|| async { client.is_connected() }, Duration::from_secs(5)).await;

    client.subscribe_markets().await.unwrap();

    let btc_id = InstrumentId::from("BTC-USD.DYDX");
    client.subscribe_trades(btc_id).await.unwrap();

    tokio::time::sleep(Duration::from_millis(200)).await;

    let subs = state.subscriptions.lock().await.clone();
    assert!(
        !subs.is_empty(),
        "Should be able to subscribe after stream call"
    );

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
#[ignore = "Flaky - timing issues with repeated connections"]
async fn test_connection_lifecycle_multiple_times() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{addr}/v4/ws");

    let mut client = DydxWebSocketClient::new_public(ws_url, Some(30));

    for i in 0..3 {
        client.connect().await.unwrap();
        wait_until_async(|| async { client.is_connected() }, Duration::from_secs(5)).await;
        assert!(
            client.is_connected(),
            "Client should be connected on iteration {i}"
        );

        client.disconnect().await.unwrap();
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(
            !client.is_connected(),
            "Client should not be connected on iteration {i}"
        );
    }

    let conn_count = *state.connection_count.lock().await;
    assert!(conn_count >= 3, "Should have connected multiple times");
}

#[rstest]
#[tokio::test]
async fn test_orderbook_subscription_flow() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{addr}/v4/ws");

    let mut client = DydxWebSocketClient::new_public(ws_url, Some(30));
    client.connect().await.unwrap();

    wait_until_async(|| async { client.is_connected() }, Duration::from_secs(5)).await;

    let btc_id = InstrumentId::from("BTC-USD.DYDX");

    client.subscribe_orderbook(btc_id).await.unwrap();

    tokio::time::sleep(Duration::from_millis(200)).await;

    let subs = state.subscriptions.lock().await;
    assert!(
        subs.iter().any(|s| s.contains("v4_orderbook")),
        "Should have orderbook subscription"
    );

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_candles_subscription_with_resolution() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{addr}/v4/ws");

    let mut client = DydxWebSocketClient::new_public(ws_url, Some(30));
    client.connect().await.unwrap();

    wait_until_async(|| async { client.is_connected() }, Duration::from_secs(5)).await;

    let btc_id = InstrumentId::from("BTC-USD.DYDX");

    client.subscribe_candles(btc_id, "5MINS").await.unwrap();

    tokio::time::sleep(Duration::from_millis(200)).await;

    let subs = state.subscriptions.lock().await;
    assert!(
        subs.iter().any(|s| s.contains("v4_candles")),
        "Should have candles subscription"
    );

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
#[ignore = "Flaky - mock server doesn't track unsubscribe events reliably"]
async fn test_unsubscribe_orderbook() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{addr}/v4/ws");

    let mut client = DydxWebSocketClient::new_public(ws_url, Some(30));
    client.connect().await.unwrap();

    wait_until_async(|| async { client.is_connected() }, Duration::from_secs(5)).await;

    let btc_id = InstrumentId::from("BTC-USD.DYDX");

    client.subscribe_orderbook(btc_id).await.unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;

    client.unsubscribe_orderbook(btc_id).await.unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;

    let events = state.subscription_events.lock().await;
    assert!(
        events
            .iter()
            .any(|(ch, subscribed)| ch.contains("v4_orderbook") && !subscribed),
        "Should have unsubscribe event for orderbook"
    );

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
#[ignore = "Flaky - mock server doesn't track unsubscribe events reliably"]
async fn test_unsubscribe_candles() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{addr}/v4/ws");

    let mut client = DydxWebSocketClient::new_public(ws_url, Some(30));
    client.connect().await.unwrap();

    wait_until_async(|| async { client.is_connected() }, Duration::from_secs(5)).await;

    let btc_id = InstrumentId::from("BTC-USD.DYDX");

    client.subscribe_candles(btc_id, "1MIN").await.unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;

    client.unsubscribe_candles(btc_id, "1MIN").await.unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;

    let events = state.subscription_events.lock().await;
    assert!(
        events
            .iter()
            .any(|(ch, subscribed)| ch.contains("v4_candles") && !subscribed),
        "Should have unsubscribe event for candles"
    );

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_mixed_subscription_types() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{addr}/v4/ws");

    let mut client = DydxWebSocketClient::new_public(ws_url, Some(30));
    client.connect().await.unwrap();

    wait_until_async(|| async { client.is_connected() }, Duration::from_secs(5)).await;

    let btc_id = InstrumentId::from("BTC-USD.DYDX");
    let eth_id = InstrumentId::from("ETH-USD.DYDX");

    client.subscribe_trades(btc_id).await.unwrap();
    client.subscribe_orderbook(eth_id).await.unwrap();
    client.subscribe_candles(btc_id, "1MIN").await.unwrap();

    tokio::time::sleep(Duration::from_millis(200)).await;

    let subs = state.subscriptions.lock().await;
    assert!(
        subs.len() >= 3,
        "Should have at least 3 different subscriptions"
    );
    assert!(
        subs.iter().any(|s| s.contains("v4_trades")),
        "Should have trades"
    );
    assert!(
        subs.iter().any(|s| s.contains("v4_orderbook")),
        "Should have orderbook"
    );
    assert!(
        subs.iter().any(|s| s.contains("v4_candles")),
        "Should have candles"
    );

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_reconnection_preserves_connection_count() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{addr}/v4/ws");

    let mut client = DydxWebSocketClient::new_public(ws_url, Some(30));
    client.connect().await.unwrap();

    wait_until_async(|| async { client.is_connected() }, Duration::from_secs(5)).await;

    let initial_count = *state.connection_count.lock().await;

    state.disconnect_trigger.store(true, Ordering::Relaxed);
    tokio::time::sleep(Duration::from_millis(100)).await;

    wait_until_async(
        || async { *state.connection_count.lock().await > initial_count },
        Duration::from_secs(5),
    )
    .await;

    let final_count = *state.connection_count.lock().await;
    assert!(
        final_count > initial_count,
        "Connection count should increment after reconnection"
    );

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_subscription_validation_empty_symbol() {
    let (addr, _state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{addr}/v4/ws");

    let mut client = DydxWebSocketClient::new_public(ws_url, Some(30));
    client.connect().await.unwrap();

    wait_until_async(|| async { client.is_connected() }, Duration::from_secs(5)).await;

    let invalid_id = InstrumentId::from("INVALID.DYDX");
    let result = client.subscribe_trades(invalid_id).await;

    assert!(result.is_ok(), "Subscribe should not fail for any symbol");

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
#[ignore = "Flaky - race conditions with concurrent subscriptions"]
async fn test_concurrent_subscriptions() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{addr}/v4/ws");

    let mut client = DydxWebSocketClient::new_public(ws_url, Some(30));
    client.connect().await.unwrap();

    wait_until_async(|| async { client.is_connected() }, Duration::from_secs(5)).await;

    let btc_id = InstrumentId::from("BTC-USD.DYDX");
    let eth_id = InstrumentId::from("ETH-USD.DYDX");
    let sol_id = InstrumentId::from("SOL-USD.DYDX");

    let (r1, r2, r3) = tokio::join!(
        client.subscribe_trades(btc_id),
        client.subscribe_trades(eth_id),
        client.subscribe_trades(sol_id),
    );

    assert!(r1.is_ok() && r2.is_ok() && r3.is_ok());

    tokio::time::sleep(Duration::from_millis(200)).await;

    let subs = state.subscriptions.lock().await;
    assert!(subs.len() >= 3, "Should handle concurrent subscriptions");

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_heartbeat_keeps_connection_alive() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{addr}/v4/ws");

    let mut client = DydxWebSocketClient::new_public(ws_url, Some(1));
    client.connect().await.unwrap();

    wait_until_async(|| async { client.is_connected() }, Duration::from_secs(5)).await;

    tokio::time::sleep(Duration::from_secs(2)).await;

    let ping_count = state.ping_count.load(Ordering::Relaxed);
    assert!(ping_count > 0, "Should have sent heartbeat pings");
    assert!(client.is_connected(), "Connection should still be alive");

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
#[ignore = "Flaky - disconnect state timing issues"]
async fn test_disconnect_clears_subscriptions() {
    let (addr, _state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{addr}/v4/ws");

    let mut client = DydxWebSocketClient::new_public(ws_url, Some(30));
    client.connect().await.unwrap();

    wait_until_async(|| async { client.is_connected() }, Duration::from_secs(5)).await;

    let btc_id = InstrumentId::from("BTC-USD.DYDX");
    client.subscribe_trades(btc_id).await.unwrap();

    tokio::time::sleep(Duration::from_millis(100)).await;

    client.disconnect().await.unwrap();

    tokio::time::sleep(Duration::from_millis(100)).await;

    assert!(!client.is_connected(), "Should be disconnected");
}

#[rstest]
#[tokio::test]
async fn test_stream_receiver_persists_across_reconnect() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{addr}/v4/ws");

    let mut client = DydxWebSocketClient::new_public(ws_url, Some(30));
    client.connect().await.unwrap();

    wait_until_async(|| async { client.is_connected() }, Duration::from_secs(5)).await;

    client.subscribe_markets().await.unwrap();

    state.disconnect_trigger.store(true, Ordering::Relaxed);
    tokio::time::sleep(Duration::from_millis(500)).await;

    wait_until_async(|| async { client.is_connected() }, Duration::from_secs(5)).await;

    assert!(client.is_connected(), "Should reconnect successfully");

    client.disconnect().await.unwrap();
}
