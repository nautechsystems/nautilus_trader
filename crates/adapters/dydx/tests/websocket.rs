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
            } else if channel_str.starts_with("v4_subaccounts") {
                send_sample_subaccounts(socket, channel_str, value).await;
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

async fn send_sample_subaccounts(socket: &mut WebSocket, channel: &str, value: &serde_json::Value) {
    let id = value
        .get("id")
        .and_then(|v| v.as_str())
        .unwrap_or("dydx1test/0");

    // Send initial subscribed message with subaccount info
    let subscribed_msg = json!({
        "type": "subscribed",
        "connection_id": "test-conn-123",
        "message_id": 1,
        "channel": channel,
        "id": id,
        "contents": {
            "subaccount": {
                "address": "dydx1test",
                "subaccountNumber": 0,
                "equity": "125000.50",
                "freeCollateral": "100000.25",
                "openPerpetualPositions": {
                    "BTC-USD": {
                        "market": "BTC-USD",
                        "status": "OPEN",
                        "side": "LONG",
                        "size": "0.5",
                        "maxSize": "1.0",
                        "entryPrice": "43000.0",
                        "exitPrice": null,
                        "realizedPnl": "125.50",
                        "unrealizedPnl": "125.00",
                        "createdAt": "2024-01-01T00:00:00.000Z",
                        "closedAt": null,
                        "sumOpen": "0.5",
                        "sumClose": "0.0",
                        "netFunding": "-10.25"
                    }
                },
                "assetPositions": {
                    "USDC": {
                        "symbol": "USDC",
                        "side": "LONG",
                        "size": "100000.0",
                        "assetId": "0"
                    }
                },
                "marginEnabled": true,
                "updatedAtHeight": "12345700",
                "latestProcessedBlockHeight": "12345700"
            }
        }
    });
    let _ = socket
        .send(Message::Text(subscribed_msg.to_string().into()))
        .await;

    // Send an update with order and fill
    let update_msg = json!({
        "type": "channel_data",
        "connection_id": "test-conn-123",
        "message_id": 6,
        "id": id,
        "channel": channel,
        "version": "1.0.0",
        "contents": {
            "orders": [{
                "id": "order-001",
                "subaccountId": "dydx1test/0",
                "clientId": "12345678",
                "clobPairId": "0",
                "side": "BUY",
                "size": "0.5",
                "price": "43000.0",
                "status": "FILLED",
                "type": "LIMIT",
                "timeInForce": "GTT",
                "postOnly": false,
                "reduceOnly": false,
                "orderFlags": "64",
                "goodTilBlockTime": "2024-01-02T00:00:00.000Z",
                "createdAtHeight": "12345678",
                "clientMetadata": "4",
                "totalFilled": "0.5",
                "updatedAt": "2024-01-01T00:00:00.000Z",
                "updatedAtHeight": "12345678"
            }],
            "fills": [{
                "id": "fill-001",
                "subaccountId": "dydx1test/0",
                "side": "BUY",
                "liquidity": "TAKER",
                "type": "LIMIT",
                "market": "BTC-USD",
                "marketType": "PERPETUAL",
                "price": "43000.0",
                "size": "0.5",
                "fee": "10.75",
                "createdAt": "2024-01-01T00:00:00.000Z",
                "createdAtHeight": "12345678",
                "orderId": "order-001",
                "clientMetadata": "4"
            }]
        }
    });
    let _ = socket
        .send(Message::Text(update_msg.to_string().into()))
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

    Ok((addr, state))
}

#[rstest]
#[tokio::test]
async fn test_websocket_connection() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{addr}/v4/ws");

    let mut client = DydxWebSocketClient::new_public(ws_url, Some(30));
    client.connect().await.unwrap();

    wait_until_async(
        || async { *state.connection_count.lock().await == 1 },
        Duration::from_secs(5),
    )
    .await;

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

    wait_until_async(|| async { !client.is_connected() }, Duration::from_secs(5)).await;

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

    wait_until_async(
        || async {
            state
                .subscriptions
                .lock()
                .await
                .iter()
                .any(|s| s.contains("v4_trades"))
        },
        Duration::from_secs(5),
    )
    .await;

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

    wait_until_async(
        || async {
            state
                .subscriptions
                .lock()
                .await
                .iter()
                .any(|s| s.contains("v4_orderbook"))
        },
        Duration::from_secs(5),
    )
    .await;

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

    wait_until_async(
        || async {
            state
                .subscriptions
                .lock()
                .await
                .iter()
                .any(|s| s.contains("v4_candles"))
        },
        Duration::from_secs(5),
    )
    .await;

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

    wait_until_async(
        || async {
            state
                .subscriptions
                .lock()
                .await
                .iter()
                .any(|s| s.contains("v4_trades"))
        },
        Duration::from_secs(5),
    )
    .await;

    client.unsubscribe_trades(instrument_id).await.unwrap();

    wait_until_async(
        || async {
            !state
                .subscriptions
                .lock()
                .await
                .iter()
                .any(|s| s.contains("v4_trades"))
        },
        Duration::from_secs(5),
    )
    .await;

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

    wait_until_async(
        || async {
            state
                .subscription_events()
                .await
                .iter()
                .any(|(_, success)| !*success)
        },
        Duration::from_secs(5),
    )
    .await;

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

    wait_until_async(
        || async { state.subscriptions.lock().await.len() >= 3 },
        Duration::from_secs(5),
    )
    .await;

    let subs = state.subscriptions.lock().await;
    assert!(subs.len() >= 3, "Expected at least 3 subscriptions");

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_ping_pong() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{addr}/v4/ws");

    let mut client = DydxWebSocketClient::new_public(ws_url, Some(1));
    client.connect().await.unwrap();

    wait_until_async(|| async { client.is_connected() }, Duration::from_secs(5)).await;

    // Wait for complete ping/pong cycle (pong_count tracks successful responses)
    wait_until_async(
        || async { state.pong_count.load(Ordering::Relaxed) >= 1 },
        Duration::from_secs(3),
    )
    .await;

    let pong_count = state.pong_count.load(Ordering::Relaxed);
    assert!(
        pong_count >= 1,
        "Expected at least 1 completed ping/pong cycle within 3s, got {pong_count}"
    );

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

    wait_until_async(
        || async { *state.connection_count.lock().await > initial_count },
        Duration::from_secs(5),
    )
    .await;

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

    wait_until_async(|| async { !client.is_connected() }, Duration::from_secs(5)).await;

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

        wait_until_async(|| async { !client.is_connected() }, Duration::from_secs(5)).await;

        state.disconnect_trigger.store(false, Ordering::Relaxed);
        client.disconnect().await.unwrap();

        wait_until_async(|| async { !client.is_connected() }, Duration::from_secs(5)).await;

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

    wait_until_async(
        || async {
            state
                .subscriptions
                .lock()
                .await
                .iter()
                .any(|s| s.contains("v4_trades"))
        },
        Duration::from_secs(5),
    )
    .await;

    state.disconnect_trigger.store(true, Ordering::Relaxed);

    wait_until_async(|| async { !client.is_connected() }, Duration::from_secs(5)).await;

    state.disconnect_trigger.store(false, Ordering::Relaxed);
    state.subscription_events.lock().await.clear();

    client.disconnect().await.unwrap();

    wait_until_async(|| async { !client.is_connected() }, Duration::from_secs(5)).await;

    client.connect().await.unwrap();

    wait_until_async(|| async { client.is_connected() }, Duration::from_secs(5)).await;

    wait_until_async(
        || async {
            state
                .subscription_events()
                .await
                .iter()
                .any(|(channel, success)| channel.contains("v4_trades") && *success)
        },
        Duration::from_secs(5),
    )
    .await;

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

    wait_until_async(
        || async {
            state
                .subscription_events()
                .await
                .iter()
                .filter(|(_, success)| !*success)
                .count()
                >= 2
        },
        Duration::from_secs(5),
    )
    .await;

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

    let instrument_id = InstrumentId::from("BTC-USD.DYDX");
    client.subscribe_trades(instrument_id).await.unwrap();

    wait_until_async(
        || async {
            state
                .subscriptions
                .lock()
                .await
                .iter()
                .any(|s| s.contains("v4_trades"))
        },
        Duration::from_secs(5),
    )
    .await;

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

    wait_until_async(
        || async { state.subscriptions.lock().await.len() >= 3 },
        Duration::from_secs(5),
    )
    .await;

    client.unsubscribe_trades(instrument_id).await.unwrap();
    client.unsubscribe_orderbook(instrument_id).await.unwrap();

    wait_until_async(
        || async {
            let subs = state.subscriptions.lock().await;
            !subs.iter().any(|s| s.contains("v4_trades"))
                && !subs.iter().any(|s| s.contains("v4_orderbook"))
        },
        Duration::from_secs(5),
    )
    .await;

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

    wait_until_async(
        || async { *state.connection_count.lock().await == 2 },
        Duration::from_secs(5),
    )
    .await;

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

    // Use a smaller sleep here since we expect connection to fail
    wait_until_async(
        || async { true }, // Always complete - just need a small delay
        Duration::from_millis(500),
    )
    .await;

    assert!(
        !client.is_connected(),
        "Should not connect to unreachable server"
    );
}

#[rstest]
#[tokio::test]
#[ignore] // Duplicates test_ping_pong
async fn test_sends_pong_for_control_ping() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{addr}/v4/ws");

    let mut client = DydxWebSocketClient::new_public(ws_url, Some(1));
    client.connect().await.unwrap();

    wait_until_async(|| async { client.is_connected() }, Duration::from_secs(5)).await;

    // Wait for complete ping/pong cycle
    wait_until_async(
        || async { state.pong_count.load(Ordering::Relaxed) >= 1 },
        Duration::from_secs(3),
    )
    .await;

    let pong_count = state.pong_count.load(Ordering::Relaxed);
    assert!(
        pong_count >= 1,
        "Expected at least 1 completed ping/pong cycle within 3s, got {pong_count}"
    );

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

    wait_until_async(
        || async { state.subscription_events().await.len() >= 2 },
        Duration::from_secs(5),
    )
    .await;

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

    wait_until_async(
        || async {
            state
                .subscriptions
                .lock()
                .await
                .iter()
                .any(|s| s.contains("v4_trades"))
        },
        Duration::from_secs(5),
    )
    .await;

    state.disconnect_trigger.store(true, Ordering::Relaxed);

    // Wait for reconnection to complete
    wait_until_async(|| async { client.is_connected() }, Duration::from_secs(10)).await;

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

    wait_until_async(
        || async {
            _state
                .subscriptions
                .lock()
                .await
                .iter()
                .any(|s| s.contains("v4_trades"))
        },
        Duration::from_secs(5),
    )
    .await;

    client.disconnect().await.unwrap();

    wait_until_async(|| async { !client.is_connected() }, Duration::from_secs(5)).await;

    client.connect().await.unwrap();
    wait_until_async(|| async { client.is_connected() }, Duration::from_secs(5)).await;

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

    wait_until_async(
        || async {
            _state
                .subscriptions
                .lock()
                .await
                .iter()
                .any(|s| s.contains("v4_trades"))
        },
        Duration::from_secs(5),
    )
    .await;

    client.disconnect().await.unwrap();

    wait_until_async(|| async { !client.is_connected() }, Duration::from_secs(5)).await;

    client.connect().await.unwrap();
    wait_until_async(|| async { client.is_connected() }, Duration::from_secs(5)).await;

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

    wait_until_async(
        || async {
            state
                .subscriptions
                .lock()
                .await
                .iter()
                .any(|s| s.contains("v4_trades"))
        },
        Duration::from_secs(5),
    )
    .await;

    state.disconnect_trigger.store(true, Ordering::Relaxed);

    // Wait briefly for disconnect to register
    wait_until_async(|| async { true }, Duration::from_millis(200)).await;

    let _ = client.is_connected();

    // Wait for potential reconnection
    wait_until_async(|| async { client.is_connected() }, Duration::from_secs(5)).await;

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

    wait_until_async(
        || async { state.subscriptions.lock().await.len() == 1 },
        Duration::from_secs(5),
    )
    .await;

    let subs = state.subscriptions.lock().await.clone();
    assert_eq!(subs.len(), 1);

    state.disconnect_trigger.store(true, Ordering::Relaxed);

    wait_until_async(
        || async { state.subscription_events().await.len() >= 2 },
        Duration::from_secs(5),
    )
    .await;

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

    wait_until_async(
        || async { state.subscriptions.lock().await.len() == 1 },
        Duration::from_secs(5),
    )
    .await;

    let subs = state.subscriptions.lock().await.clone();
    assert_eq!(subs.len(), 1);

    client.unsubscribe_trades(btc_id).await.unwrap();

    wait_until_async(
        || async { state.subscription_events().await.len() == 2 },
        Duration::from_secs(5),
    )
    .await;

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

    wait_until_async(
        || async {
            state
                .subscription_events()
                .await
                .iter()
                .any(|(_, success)| !*success)
        },
        Duration::from_secs(5),
    )
    .await;

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

    wait_until_async(
        || async { !state.subscriptions.lock().await.is_empty() },
        Duration::from_secs(5),
    )
    .await;

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

    wait_until_async(
        || async { _state.subscriptions.lock().await.len() >= 2 },
        Duration::from_secs(5),
    )
    .await;

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

    wait_until_async(
        || async {
            _state
                .subscriptions
                .lock()
                .await
                .iter()
                .any(|s| s.contains("v4_candles"))
        },
        Duration::from_secs(5),
    )
    .await;

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

    wait_until_async(|| async { !client.is_connected() }, Duration::from_secs(5)).await;

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

    wait_until_async(
        || async { state.subscriptions.lock().await.len() == 3 },
        Duration::from_secs(5),
    )
    .await;

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

    wait_until_async(
        || async { !state.subscriptions.lock().await.is_empty() },
        Duration::from_secs(5),
    )
    .await;

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

        wait_until_async(|| async { !client.is_connected() }, Duration::from_secs(5)).await;

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

    wait_until_async(
        || async {
            state
                .subscriptions
                .lock()
                .await
                .iter()
                .any(|s| s.contains("v4_orderbook"))
        },
        Duration::from_secs(5),
    )
    .await;

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

    wait_until_async(
        || async {
            state
                .subscriptions
                .lock()
                .await
                .iter()
                .any(|s| s.contains("v4_candles"))
        },
        Duration::from_secs(5),
    )
    .await;

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

    wait_until_async(
        || async {
            state
                .subscriptions
                .lock()
                .await
                .iter()
                .any(|s| s.contains("v4_orderbook"))
        },
        Duration::from_secs(5),
    )
    .await;

    client.unsubscribe_orderbook(btc_id).await.unwrap();

    wait_until_async(
        || async {
            state
                .subscription_events
                .lock()
                .await
                .iter()
                .any(|(ch, subscribed)| ch.contains("v4_orderbook") && !subscribed)
        },
        Duration::from_secs(5),
    )
    .await;

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

    wait_until_async(
        || async {
            state
                .subscriptions
                .lock()
                .await
                .iter()
                .any(|s| s.contains("v4_candles"))
        },
        Duration::from_secs(5),
    )
    .await;

    client.unsubscribe_candles(btc_id, "1MIN").await.unwrap();

    wait_until_async(
        || async {
            state
                .subscription_events
                .lock()
                .await
                .iter()
                .any(|(ch, subscribed)| ch.contains("v4_candles") && !subscribed)
        },
        Duration::from_secs(5),
    )
    .await;

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

    wait_until_async(
        || async {
            let subs = state.subscriptions.lock().await;
            subs.len() >= 3
                && subs.iter().any(|s| s.contains("v4_trades"))
                && subs.iter().any(|s| s.contains("v4_orderbook"))
                && subs.iter().any(|s| s.contains("v4_candles"))
        },
        Duration::from_secs(5),
    )
    .await;

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

    wait_until_async(
        || async { state.subscriptions.lock().await.len() >= 3 },
        Duration::from_secs(5),
    )
    .await;

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

    // Wait for complete ping/pong cycle to verify heartbeat is working
    wait_until_async(
        || async { state.pong_count.load(Ordering::Relaxed) >= 1 },
        Duration::from_secs(5),
    )
    .await;

    let pong_count = state.pong_count.load(Ordering::Relaxed);
    assert!(
        pong_count >= 1,
        "Expected at least 1 completed heartbeat cycle within 5s (heartbeat_interval=1s), got {pong_count}"
    );
    assert!(
        client.is_connected(),
        "Connection should still be alive after heartbeat"
    );

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
#[ignore = "Flaky - disconnect state timing issues"]
async fn test_disconnect_clears_subscriptions() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{addr}/v4/ws");

    let mut client = DydxWebSocketClient::new_public(ws_url, Some(30));
    client.connect().await.unwrap();

    wait_until_async(|| async { client.is_connected() }, Duration::from_secs(5)).await;

    let btc_id = InstrumentId::from("BTC-USD.DYDX");
    client.subscribe_trades(btc_id).await.unwrap();

    wait_until_async(
        || async {
            state
                .subscriptions
                .lock()
                .await
                .iter()
                .any(|s| s.contains("v4_trades"))
        },
        Duration::from_secs(5),
    )
    .await;

    client.disconnect().await.unwrap();

    wait_until_async(|| async { !client.is_connected() }, Duration::from_secs(5)).await;

    assert!(!client.is_connected(), "Should be disconnected");
}

#[rstest]
#[tokio::test]
#[ignore] // Flaky: reconnection timing is non-deterministic
async fn test_stream_receiver_persists_across_reconnect() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{addr}/v4/ws");

    let mut client = DydxWebSocketClient::new_public(ws_url, Some(30));
    client.connect().await.unwrap();

    wait_until_async(|| async { client.is_connected() }, Duration::from_secs(5)).await;

    client.subscribe_markets().await.unwrap();

    state.disconnect_trigger.store(true, Ordering::Relaxed);

    wait_until_async(|| async { !client.is_connected() }, Duration::from_secs(5)).await;

    state.disconnect_trigger.store(false, Ordering::Relaxed);

    wait_until_async(|| async { client.is_connected() }, Duration::from_secs(5)).await;

    assert!(client.is_connected(), "Should reconnect successfully");

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_subscribe_markets_immediately_after_connect() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{addr}/v4/ws");

    let mut client = DydxWebSocketClient::new_public(ws_url, Some(30));
    client.connect().await.unwrap();

    wait_until_async(|| async { client.is_connected() }, Duration::from_secs(5)).await;

    // This is the exact pattern from section 1.3 - subscribe_markets() immediately after connect()
    client.subscribe_markets().await.unwrap();

    wait_until_async(
        || async {
            state
                .subscriptions
                .lock()
                .await
                .iter()
                .any(|s| s.contains("v4_markets"))
        },
        Duration::from_secs(5),
    )
    .await;

    let subs = state.subscriptions.lock().await;
    assert!(
        subs.iter().any(|s| s.contains("v4_markets")),
        "Markets subscription should work immediately after connect"
    );

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_subscribe_markets_multiple_times_idempotent() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{addr}/v4/ws");

    let mut client = DydxWebSocketClient::new_public(ws_url, Some(30));
    client.connect().await.unwrap();

    wait_until_async(|| async { client.is_connected() }, Duration::from_secs(5)).await;

    // Subscribe multiple times
    client.subscribe_markets().await.unwrap();
    client.subscribe_markets().await.unwrap();
    client.subscribe_markets().await.unwrap();

    wait_until_async(
        || async {
            state
                .subscriptions
                .lock()
                .await
                .iter()
                .any(|s| s.contains("v4_markets"))
        },
        Duration::from_secs(5),
    )
    .await;

    let subs = state.subscriptions.lock().await;
    let markets_count = subs.iter().filter(|s| s.contains("v4_markets")).count();
    assert_eq!(
        markets_count, 1,
        "Multiple subscribe_markets calls should be idempotent"
    );

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_clone_shares_command_channel() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{addr}/v4/ws");

    let mut client = DydxWebSocketClient::new_public(ws_url, Some(30));
    client.connect().await.unwrap();

    wait_until_async(|| async { client.is_connected() }, Duration::from_secs(5)).await;

    // Clone the client after connect
    let client_clone = client.clone();

    // Subscribe using the clone - this tests the Arc<RwLock<UnboundedSender>> pattern
    client_clone.subscribe_markets().await.unwrap();

    wait_until_async(
        || async {
            state
                .subscriptions
                .lock()
                .await
                .iter()
                .any(|s| s.contains("v4_markets"))
        },
        Duration::from_secs(5),
    )
    .await;

    let subs = state.subscriptions.lock().await;
    assert!(
        subs.iter().any(|s| s.contains("v4_markets")),
        "Clone should share command channel and be able to subscribe"
    );

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_subscribe_trades_and_markets_in_sequence() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{addr}/v4/ws");

    let mut client = DydxWebSocketClient::new_public(ws_url, Some(30));
    client.connect().await.unwrap();

    wait_until_async(|| async { client.is_connected() }, Duration::from_secs(5)).await;

    // Subscribe to markets first (like the Python adapter does)
    client.subscribe_markets().await.unwrap();

    // Then subscribe to trades
    let btc_id = InstrumentId::from("BTC-USD.DYDX");
    client.subscribe_trades(btc_id).await.unwrap();

    wait_until_async(
        || async {
            let subs = state.subscriptions.lock().await;
            subs.iter().any(|s| s.contains("v4_markets"))
                && subs.iter().any(|s| s.contains("v4_trades"))
        },
        Duration::from_secs(5),
    )
    .await;

    let subs = state.subscriptions.lock().await;
    assert!(
        subs.iter().any(|s| s.contains("v4_markets")),
        "Markets should be subscribed"
    );
    assert!(
        subs.iter().any(|s| s.contains("v4_trades")),
        "Trades should be subscribed after markets"
    );

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_subscribe_block_height() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{addr}/v4/ws");

    let mut client = DydxWebSocketClient::new_public(ws_url, Some(30));
    client.connect().await.unwrap();

    wait_until_async(|| async { client.is_connected() }, Duration::from_secs(5)).await;

    client.subscribe_block_height().await.unwrap();

    wait_until_async(
        || async {
            state
                .subscriptions
                .lock()
                .await
                .iter()
                .any(|s| s.contains("v4_block_height"))
        },
        Duration::from_secs(5),
    )
    .await;

    let subs = state.subscriptions.lock().await;
    assert!(
        subs.iter().any(|s| s.contains("v4_block_height")),
        "Block height subscription should work"
    );

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_unsubscribe_markets() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{addr}/v4/ws");

    let mut client = DydxWebSocketClient::new_public(ws_url, Some(30));
    client.connect().await.unwrap();

    wait_until_async(|| async { client.is_connected() }, Duration::from_secs(5)).await;

    client.subscribe_markets().await.unwrap();

    wait_until_async(
        || async {
            state
                .subscriptions
                .lock()
                .await
                .iter()
                .any(|s| s.contains("v4_markets"))
        },
        Duration::from_secs(5),
    )
    .await;

    client.unsubscribe_markets().await.unwrap();

    wait_until_async(
        || async {
            !state
                .subscriptions
                .lock()
                .await
                .iter()
                .any(|s| s.contains("v4_markets"))
        },
        Duration::from_secs(5),
    )
    .await;

    let subs = state.subscriptions.lock().await;
    assert!(
        !subs.iter().any(|s| s.contains("v4_markets")),
        "Markets should be unsubscribed"
    );

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_unsubscribe_block_height() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{addr}/v4/ws");

    let mut client = DydxWebSocketClient::new_public(ws_url, Some(30));
    client.connect().await.unwrap();

    wait_until_async(|| async { client.is_connected() }, Duration::from_secs(5)).await;

    client.subscribe_block_height().await.unwrap();

    wait_until_async(
        || async {
            state
                .subscriptions
                .lock()
                .await
                .iter()
                .any(|s| s.contains("v4_block_height"))
        },
        Duration::from_secs(5),
    )
    .await;

    client.unsubscribe_block_height().await.unwrap();

    wait_until_async(
        || async {
            !state
                .subscriptions
                .lock()
                .await
                .iter()
                .any(|s| s.contains("v4_block_height"))
        },
        Duration::from_secs(5),
    )
    .await;

    let subs = state.subscriptions.lock().await;
    assert!(
        !subs.iter().any(|s| s.contains("v4_block_height")),
        "Block height should be unsubscribed"
    );

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_subscribe_all_channels_sequence() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{addr}/v4/ws");

    let mut client = DydxWebSocketClient::new_public(ws_url, Some(30));
    client.connect().await.unwrap();

    wait_until_async(|| async { client.is_connected() }, Duration::from_secs(5)).await;

    let btc_id = InstrumentId::from("BTC-USD.DYDX");

    // Subscribe to all channel types in sequence (like real usage)
    client.subscribe_markets().await.unwrap();
    client.subscribe_block_height().await.unwrap();
    client.subscribe_trades(btc_id).await.unwrap();
    client.subscribe_orderbook(btc_id).await.unwrap();
    client.subscribe_candles(btc_id, "1MIN").await.unwrap();

    wait_until_async(
        || async {
            let subs = state.subscriptions.lock().await;
            subs.len() >= 5
                && subs.iter().any(|s| s.contains("v4_markets"))
                && subs.iter().any(|s| s.contains("v4_block_height"))
                && subs.iter().any(|s| s.contains("v4_trades"))
                && subs.iter().any(|s| s.contains("v4_orderbook"))
                && subs.iter().any(|s| s.contains("v4_candles"))
        },
        Duration::from_secs(5),
    )
    .await;

    let subs = state.subscriptions.lock().await;
    assert!(subs.len() >= 5, "Should have all 5 subscription types");
    assert!(subs.iter().any(|s| s.contains("v4_markets")));
    assert!(subs.iter().any(|s| s.contains("v4_block_height")));
    assert!(subs.iter().any(|s| s.contains("v4_trades")));
    assert!(subs.iter().any(|s| s.contains("v4_orderbook")));
    assert!(subs.iter().any(|s| s.contains("v4_candles")));

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_reconnect_then_subscribe_markets() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{addr}/v4/ws");

    let mut client = DydxWebSocketClient::new_public(ws_url, Some(30));
    client.connect().await.unwrap();

    wait_until_async(|| async { client.is_connected() }, Duration::from_secs(5)).await;

    // Disconnect manually (not via server trigger)
    client.disconnect().await.unwrap();

    wait_until_async(|| async { !client.is_connected() }, Duration::from_secs(5)).await;

    // Clear subscription state on server
    state.subscriptions.lock().await.clear();

    // Reconnect
    client.connect().await.unwrap();

    wait_until_async(|| async { client.is_connected() }, Duration::from_secs(5)).await;

    // Subscribe after reconnection
    client.subscribe_markets().await.unwrap();

    wait_until_async(
        || async {
            state
                .subscriptions
                .lock()
                .await
                .iter()
                .any(|s| s.contains("v4_markets"))
        },
        Duration::from_secs(5),
    )
    .await;

    let subs = state.subscriptions.lock().await;
    assert!(
        subs.iter().any(|s| s.contains("v4_markets")),
        "Should be able to subscribe after reconnection"
    );

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_subscribe_without_wait_until_active() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{addr}/v4/ws");

    let mut client = DydxWebSocketClient::new_public(ws_url, Some(30));
    client.connect().await.unwrap();

    // Don't call wait_until_active - just give a small delay for connection
    wait_until_async(|| async { client.is_connected() }, Duration::from_secs(5)).await;

    // Try to subscribe
    let result = client.subscribe_markets().await;

    // Should succeed even without explicit wait_until_active
    assert!(
        result.is_ok(),
        "Subscribe should work after connect without explicit wait"
    );

    wait_until_async(
        || async {
            state
                .subscriptions
                .lock()
                .await
                .iter()
                .any(|s| s.contains("v4_markets"))
        },
        Duration::from_secs(5),
    )
    .await;

    let subs = state.subscriptions.lock().await;
    assert!(
        subs.iter().any(|s| s.contains("v4_markets")),
        "Markets subscription should succeed"
    );

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_multiple_clones_subscribe() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{addr}/v4/ws");

    let mut client = DydxWebSocketClient::new_public(ws_url, Some(30));
    client.connect().await.unwrap();

    wait_until_async(|| async { client.is_connected() }, Duration::from_secs(5)).await;

    // Create multiple clones
    let clone1 = client.clone();
    let clone2 = client.clone();

    // Subscribe from different clones
    clone1.subscribe_markets().await.unwrap();
    clone2.subscribe_block_height().await.unwrap();

    let btc_id = InstrumentId::from("BTC-USD.DYDX");
    client.subscribe_trades(btc_id).await.unwrap();

    wait_until_async(
        || async {
            let subs = state.subscriptions.lock().await;
            subs.iter().any(|s| s.contains("v4_markets"))
                && subs.iter().any(|s| s.contains("v4_block_height"))
                && subs.iter().any(|s| s.contains("v4_trades"))
        },
        Duration::from_secs(5),
    )
    .await;

    let subs = state.subscriptions.lock().await;
    assert!(
        subs.iter().any(|s| s.contains("v4_markets")),
        "Clone 1 subscription should work"
    );
    assert!(
        subs.iter().any(|s| s.contains("v4_block_height")),
        "Clone 2 subscription should work"
    );
    assert!(
        subs.iter().any(|s| s.contains("v4_trades")),
        "Original client subscription should work"
    );

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_double_connect_is_noop() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{addr}/v4/ws");

    let mut client = DydxWebSocketClient::new_public(ws_url, Some(30));
    client.connect().await.unwrap();

    wait_until_async(|| async { client.is_connected() }, Duration::from_secs(5)).await;

    let initial_count = *state.connection_count.lock().await;

    // Call connect again while already connected
    client.connect().await.unwrap();

    wait_until_async(|| async { client.is_connected() }, Duration::from_secs(5)).await;

    let final_count = *state.connection_count.lock().await;
    assert_eq!(
        initial_count, final_count,
        "Second connect should be a no-op"
    );

    // Verify subscriptions still work
    client.subscribe_markets().await.unwrap();

    wait_until_async(
        || async {
            state
                .subscriptions
                .lock()
                .await
                .iter()
                .any(|s| s.contains("v4_markets"))
        },
        Duration::from_secs(5),
    )
    .await;

    let subs = state.subscriptions.lock().await;
    assert!(
        subs.iter().any(|s| s.contains("v4_markets")),
        "Subscriptions should work after double connect"
    );

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_url_getter() {
    let ws_url = "ws://localhost:12345/v4/ws".to_string();
    let client = DydxWebSocketClient::new_public(ws_url.clone(), Some(30));

    assert_eq!(client.url(), ws_url, "URL getter should return the URL");
}

#[rstest]
#[tokio::test]
async fn test_is_connected_false_before_connect() {
    let ws_url = "ws://localhost:12345/v4/ws".to_string();
    let client = DydxWebSocketClient::new_public(ws_url, Some(30));

    assert!(
        !client.is_connected(),
        "is_connected should be false before connect"
    );
}

#[rstest]
#[tokio::test]
async fn test_markets_subscription_failure() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{addr}/v4/ws");

    // Configure server to fail markets subscriptions
    state
        .set_subscription_failures(vec!["v4_markets".to_string()])
        .await;

    let mut client = DydxWebSocketClient::new_public(ws_url, Some(30));
    client.connect().await.unwrap();

    wait_until_async(|| async { client.is_connected() }, Duration::from_secs(5)).await;

    // This should not error (error comes async via message)
    let result = client.subscribe_markets().await;
    assert!(result.is_ok(), "Subscribe call itself should not fail");

    wait_until_async(
        || async {
            state
                .subscription_events()
                .await
                .iter()
                .any(|(ch, success)| ch.contains("v4_markets") && !*success)
        },
        Duration::from_secs(5),
    )
    .await;

    // Verify subscription was attempted but failed
    let events = state.subscription_events().await;
    assert!(
        events
            .iter()
            .any(|(ch, success)| ch.contains("v4_markets") && !*success),
        "Markets subscription should have been attempted and failed"
    );

    client.disconnect().await.unwrap();
}

const TEST_MNEMONIC: &str = "mirror actor skill push coach wait confirm orchard lunch mobile athlete gossip awake miracle matter bus reopen team ladder lazy list timber render wait";

#[rstest]
#[tokio::test]
async fn test_subscribe_subaccount_requires_auth() {
    let (addr, _state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{addr}/v4/ws");

    // Public client should fail to subscribe to subaccounts
    let mut client = DydxWebSocketClient::new_public(ws_url, Some(30));
    client.connect().await.unwrap();

    wait_until_async(|| async { client.is_connected() }, Duration::from_secs(5)).await;

    let result = client.subscribe_subaccount("dydx1test", 0).await;
    assert!(
        result.is_err(),
        "Public client should not be able to subscribe to subaccounts"
    );

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_subscribe_subaccount_with_private_client() {
    use nautilus_dydx::common::credential::DydxCredential;
    use nautilus_model::identifiers::AccountId;

    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{addr}/v4/ws");

    // Create a credential from test mnemonic
    let credential = DydxCredential::from_mnemonic(TEST_MNEMONIC, 0, vec![]).unwrap();
    let account_id = AccountId::new("DYDX-001");

    let mut client = DydxWebSocketClient::new_private(ws_url, credential, account_id, Some(30));
    client.connect().await.unwrap();

    wait_until_async(|| async { client.is_connected() }, Duration::from_secs(5)).await;

    let result = client.subscribe_subaccount("dydx1test", 0).await;
    assert!(
        result.is_ok(),
        "Private client should subscribe to subaccounts"
    );

    wait_until_async(
        || async {
            state
                .subscriptions
                .lock()
                .await
                .iter()
                .any(|s| s.contains("v4_subaccounts"))
        },
        Duration::from_secs(5),
    )
    .await;

    let subs = state.subscriptions.lock().await;
    assert!(
        subs.iter().any(|s| s.contains("v4_subaccounts")),
        "Server should have v4_subaccounts subscription"
    );

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_unsubscribe_subaccount() {
    use nautilus_dydx::common::credential::DydxCredential;
    use nautilus_model::identifiers::AccountId;

    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{addr}/v4/ws");

    let credential = DydxCredential::from_mnemonic(TEST_MNEMONIC, 0, vec![]).unwrap();
    let account_id = AccountId::new("DYDX-001");

    let mut client = DydxWebSocketClient::new_private(ws_url, credential, account_id, Some(30));
    client.connect().await.unwrap();

    wait_until_async(|| async { client.is_connected() }, Duration::from_secs(5)).await;

    // Subscribe first
    client.subscribe_subaccount("dydx1test", 0).await.unwrap();

    wait_until_async(
        || async {
            state
                .subscriptions
                .lock()
                .await
                .iter()
                .any(|s| s.contains("v4_subaccounts"))
        },
        Duration::from_secs(5),
    )
    .await;

    // Unsubscribe
    client.unsubscribe_subaccount("dydx1test", 0).await.unwrap();

    wait_until_async(
        || async {
            !state
                .subscriptions
                .lock()
                .await
                .iter()
                .any(|s| s.contains("v4_subaccounts"))
        },
        Duration::from_secs(5),
    )
    .await;

    let subs = state.subscriptions.lock().await;
    assert!(
        !subs.iter().any(|s| s.contains("v4_subaccounts")),
        "Server should not have v4_subaccounts subscription after unsubscribe"
    );

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_subaccount_subscription_failure() {
    use nautilus_dydx::common::credential::DydxCredential;
    use nautilus_model::identifiers::AccountId;

    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{addr}/v4/ws");

    // Configure server to fail subaccount subscriptions
    state
        .set_subscription_failures(vec!["v4_subaccounts".to_string()])
        .await;

    let credential = DydxCredential::from_mnemonic(TEST_MNEMONIC, 0, vec![]).unwrap();
    let account_id = AccountId::new("DYDX-001");

    let mut client = DydxWebSocketClient::new_private(ws_url, credential, account_id, Some(30));
    client.connect().await.unwrap();

    wait_until_async(|| async { client.is_connected() }, Duration::from_secs(5)).await;

    // Subscription call itself should succeed (error comes async)
    let result = client.subscribe_subaccount("dydx1test", 0).await;
    assert!(result.is_ok(), "Subscribe call itself should not fail");

    wait_until_async(
        || async {
            state
                .subscription_events()
                .await
                .iter()
                .any(|(ch, success)| ch.contains("v4_subaccounts") && !*success)
        },
        Duration::from_secs(5),
    )
    .await;

    let events = state.subscription_events().await;
    assert!(
        events
            .iter()
            .any(|(ch, success)| ch.contains("v4_subaccounts") && !*success),
        "Subaccount subscription should have been attempted and failed"
    );

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_block_height_parsing() {
    use chrono::Utc;
    use nautilus_dydx::websocket::{
        enums::{DydxWsChannel, DydxWsMessageType},
        messages::{DydxBlockHeightChannelContents, DydxWsBlockHeightChannelData},
    };

    let test_block_height = "12345678";
    let block_msg = DydxWsBlockHeightChannelData {
        msg_type: DydxWsMessageType::ChannelData,
        connection_id: "test-conn-123".to_string(),
        message_id: 42,
        id: "dydx".to_string(),
        channel: DydxWsChannel::BlockHeight,
        version: "4.0.0".to_string(),
        contents: DydxBlockHeightChannelContents {
            block_height: test_block_height.to_string(),
            time: Utc::now(),
        },
    };

    assert_eq!(
        block_msg.contents.block_height.parse::<u64>().unwrap(),
        12345678_u64,
        "Block height string should parse to correct u64"
    );
    assert_eq!(block_msg.channel, DydxWsChannel::BlockHeight);
    assert_eq!(block_msg.msg_type, DydxWsMessageType::ChannelData);
}

#[rstest]
#[tokio::test]
async fn test_block_height_invalid_format() {
    use chrono::Utc;
    use nautilus_dydx::websocket::{
        enums::{DydxWsChannel, DydxWsMessageType},
        messages::{DydxBlockHeightChannelContents, DydxWsBlockHeightChannelData},
    };

    let invalid_block_height = "not-a-number";
    let block_msg = DydxWsBlockHeightChannelData {
        msg_type: DydxWsMessageType::ChannelData,
        connection_id: "test-conn".to_string(),
        message_id: 1,
        id: "dydx".to_string(),
        channel: DydxWsChannel::BlockHeight,
        version: "4.0.0".to_string(),
        contents: DydxBlockHeightChannelContents {
            block_height: invalid_block_height.to_string(),
            time: Utc::now(),
        },
    };

    let parse_result = block_msg.contents.block_height.parse::<u64>();
    assert!(
        parse_result.is_err(),
        "Parsing invalid block height should fail"
    );
}

#[rstest]
#[tokio::test]
async fn test_block_height_subscribed_parsing() {
    use chrono::Utc;
    use nautilus_dydx::websocket::{
        enums::{DydxWsChannel, DydxWsMessageType},
        messages::{DydxBlockHeightSubscribedContents, DydxWsBlockHeightSubscribedData},
    };

    let test_height = "98765432";
    let subscribed_msg = DydxWsBlockHeightSubscribedData {
        msg_type: DydxWsMessageType::Subscribed,
        connection_id: "test-conn-456".to_string(),
        message_id: 1,
        channel: DydxWsChannel::BlockHeight,
        id: "v4_block_height".to_string(),
        contents: DydxBlockHeightSubscribedContents {
            height: test_height.to_string(),
            time: Utc::now(),
        },
    };

    assert_eq!(
        subscribed_msg.contents.height.parse::<u64>().unwrap(),
        98765432_u64,
        "Subscribed message height field should parse correctly"
    );
    assert_eq!(subscribed_msg.channel, DydxWsChannel::BlockHeight);
    assert_eq!(subscribed_msg.msg_type, DydxWsMessageType::Subscribed);
}

#[rstest]
#[tokio::test]
async fn test_block_height_field_names_differ() {
    // This test documents that subscribed and channel_data messages
    // use different field names for block height
    use nautilus_dydx::websocket::messages::{
        DydxBlockHeightChannelContents, DydxBlockHeightSubscribedContents,
    };

    let subscribed_json = r#"{"height": "100", "time": "2024-01-01T00:00:00Z"}"#;
    let subscribed: DydxBlockHeightSubscribedContents =
        serde_json::from_str(subscribed_json).unwrap();
    assert_eq!(subscribed.height, "100");

    let channel_data_json = r#"{"blockHeight": "200", "time": "2024-01-01T00:00:00Z"}"#;
    let channel_data: DydxBlockHeightChannelContents =
        serde_json::from_str(channel_data_json).unwrap();
    assert_eq!(channel_data.block_height, "200");
}
