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

//! Integration tests for the Coinbase WebSocket client using a mock server.
//!
//! These tests exercise the [`CoinbaseWebSocketClient`] directly (without the
//! [`CoinbaseDataClient`] facade) to cover transport-layer behaviour: the
//! subscribe / unsubscribe wire protocol, subscription-state tracking,
//! reconnection-driven resubscribe, the credentials-required gate on the user
//! channel, and graceful tolerance of malformed messages.

use std::{
    net::SocketAddr,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
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
use futures_util::{SinkExt, StreamExt};
use nautilus_coinbase::{
    common::enums::CoinbaseWsChannel,
    websocket::{client::CoinbaseWebSocketClient, handler::NautilusWsMessage},
};
use nautilus_common::testing::wait_until_async;
use nautilus_network::websocket::TransportBackend;
use rstest::rstest;
use serde_json::Value;
use ustr::Ustr;

#[derive(Clone, Default)]
struct WsServerState {
    /// Total connection count over the server's lifetime; increments on each
    /// upgrade so reconnection tests can wait until a fresh connection lands.
    connection_count: Arc<AtomicUsize>,
    /// Subscribe/unsubscribe payloads received from the client, in arrival
    /// order. Tests assert against this to verify wire protocol shape and
    /// resubscribe-after-reconnect behaviour.
    received_messages: Arc<tokio::sync::Mutex<Vec<Value>>>,
    /// When set, the next received subscribe causes the server to close the
    /// WebSocket so the client can exercise its reconnect path.
    drop_after_subscribe: Arc<tokio::sync::Mutex<bool>>,
    /// When set, the server sends an arbitrary non-JSON text frame on every
    /// connect to verify the client tolerates malformed messages.
    inject_malformed: Arc<tokio::sync::Mutex<bool>>,
}

impl WsServerState {
    /// All `subscribe` messages received from the client, excluding the
    /// `heartbeats` keepalive that the client primes automatically on connect.
    async fn received_subscribes(&self) -> Vec<Value> {
        self.received_messages
            .lock()
            .await
            .iter()
            .filter(|v| v.get("type").and_then(|t| t.as_str()) == Some("subscribe"))
            .filter(|v| v.get("channel").and_then(|c| c.as_str()) != Some("heartbeats"))
            .cloned()
            .collect()
    }

    async fn received_unsubscribes(&self) -> Vec<Value> {
        self.received_messages
            .lock()
            .await
            .iter()
            .filter(|v| v.get("type").and_then(|t| t.as_str()) == Some("unsubscribe"))
            .cloned()
            .collect()
    }
}

async fn handle_ws_upgrade(ws: WebSocketUpgrade, State(state): State<WsServerState>) -> Response {
    ws.on_upgrade(move |socket| handle_ws_socket(socket, state))
}

async fn handle_ws_socket(socket: WebSocket, state: WsServerState) {
    state.connection_count.fetch_add(1, Ordering::SeqCst);
    let (mut sink, mut stream) = socket.split();

    if *state.inject_malformed.lock().await {
        // Send a non-JSON frame the parser cannot decode; client should log
        // and continue rather than tear the connection down.
        let _ = sink
            .send(Message::Text("not-valid-json{".to_string().into()))
            .await;
    }

    while let Some(message) = stream.next().await {
        let Ok(message) = message else { break };

        match message {
            Message::Text(text) => {
                let payload: Value = match serde_json::from_str(&text) {
                    Ok(v) => v,
                    Err(_) => continue,
                };

                state.received_messages.lock().await.push(payload.clone());

                let msg_type = payload.get("type").and_then(|t| t.as_str());

                if msg_type == Some("subscribe") {
                    let channel = payload
                        .get("channel")
                        .and_then(|c| c.as_str())
                        .unwrap_or("");
                    let product_ids = payload
                        .get("product_ids")
                        .and_then(|p| p.as_array())
                        .cloned()
                        .unwrap_or_default();

                    // Echo a Coinbase-style "subscriptions" ack listing the
                    // active channel + products so subscription-confirmation
                    // tests can observe the round-trip.
                    let ack = serde_json::json!({
                        "channel": "subscriptions",
                        "client_id": "",
                        "timestamp": "2026-04-29T00:00:00Z",
                        "sequence_num": 0,
                        "events": [{
                            "subscriptions": {
                                channel: product_ids,
                            }
                        }]
                    });
                    let _ = sink.send(Message::Text(ack.to_string().into())).await;

                    // Drop the connection only after a non-heartbeat
                    // subscribe so we exercise the resubscribe-replay path
                    // for user-issued topics. The client primes a
                    // heartbeats subscribe automatically on every connect;
                    // dropping on that would race the user-issued subscribe
                    // out of the cmd queue.
                    if channel != "heartbeats" && *state.drop_after_subscribe.lock().await {
                        // Reset the flag so the post-reconnect subscribe is
                        // not dropped again, then drop the sink to abruptly
                        // tear down the TCP connection. A graceful close
                        // frame would let the network layer treat the drop
                        // as a clean shutdown and skip its reconnect path.
                        *state.drop_after_subscribe.lock().await = false;
                        drop(sink);
                        return;
                    }
                }
            }
            // Inner if consumes `data`, cannot hoist into a match guard
            #[expect(clippy::collapsible_match)]
            Message::Ping(data) => {
                if sink.send(Message::Pong(data)).await.is_err() {
                    break;
                }
            }
            Message::Close(_) => break,
            _ => {}
        }
    }
}

async fn start_mock_ws_server(state: WsServerState) -> SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let router = Router::new()
        .route("/ws", get(handle_ws_upgrade))
        .with_state(state);

    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });

    let start = std::time::Instant::now();

    loop {
        if tokio::net::TcpStream::connect(addr).await.is_ok() {
            break;
        }
        assert!(
            start.elapsed() <= Duration::from_secs(5),
            "Mock server did not start within timeout"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    addr
}

fn ws_url(addr: SocketAddr) -> String {
    format!("ws://{addr}/ws")
}

#[rstest]
#[tokio::test]
async fn test_ws_connect_and_disconnect_lifecycle() {
    let state = WsServerState::default();
    let addr = start_mock_ws_server(state.clone()).await;

    let mut client = CoinbaseWebSocketClient::new(&ws_url(addr), TransportBackend::default(), None);
    assert!(!client.is_active());

    client.connect().await.unwrap();
    assert!(client.is_active());

    wait_until_async(
        || {
            let state = state.clone();
            async move { state.connection_count.load(Ordering::SeqCst) >= 1 }
        },
        Duration::from_secs(2),
    )
    .await;

    client.disconnect().await;
}

#[rstest]
#[tokio::test]
async fn test_ws_subscribe_sends_typed_payload() {
    let state = WsServerState::default();
    let addr = start_mock_ws_server(state.clone()).await;

    let mut client = CoinbaseWebSocketClient::new(&ws_url(addr), TransportBackend::default(), None);
    client.connect().await.unwrap();

    let product = Ustr::from("BTC-USD");
    client
        .subscribe(CoinbaseWsChannel::MarketTrades, &[product])
        .await
        .unwrap();

    wait_until_async(
        || {
            let state = state.clone();
            async move { !state.received_subscribes().await.is_empty() }
        },
        Duration::from_secs(2),
    )
    .await;

    let subs = state.received_subscribes().await;
    assert_eq!(subs.len(), 1);
    assert_eq!(
        subs[0].get("channel").and_then(|c| c.as_str()),
        Some("market_trades")
    );
    let pids = subs[0]
        .get("product_ids")
        .and_then(|p| p.as_array())
        .expect("product_ids array");
    assert_eq!(pids.len(), 1);
    assert_eq!(pids[0].as_str(), Some("BTC-USD"));

    client.disconnect().await;
}

#[rstest]
#[tokio::test]
async fn test_ws_unsubscribe_sends_typed_payload() {
    let state = WsServerState::default();
    let addr = start_mock_ws_server(state.clone()).await;

    let mut client = CoinbaseWebSocketClient::new(&ws_url(addr), TransportBackend::default(), None);
    client.connect().await.unwrap();

    let product = Ustr::from("BTC-USD");
    client
        .subscribe(CoinbaseWsChannel::Ticker, &[product])
        .await
        .unwrap();
    wait_until_async(
        || {
            let state = state.clone();
            async move { !state.received_subscribes().await.is_empty() }
        },
        Duration::from_secs(2),
    )
    .await;

    client
        .unsubscribe(CoinbaseWsChannel::Ticker, &[product])
        .await
        .unwrap();

    wait_until_async(
        || {
            let state = state.clone();
            async move { !state.received_unsubscribes().await.is_empty() }
        },
        Duration::from_secs(2),
    )
    .await;

    let unsubs = state.received_unsubscribes().await;
    assert_eq!(unsubs.len(), 1);
    assert_eq!(
        unsubs[0].get("channel").and_then(|c| c.as_str()),
        Some("ticker")
    );

    client.disconnect().await;
}

#[rstest]
#[tokio::test]
async fn test_ws_user_channel_subscribe_without_credentials_fails() {
    // Coinbase user channel requires JWT-signed authentication. Subscribing
    // without credentials should fail before the request reaches the wire.
    let state = WsServerState::default();
    let addr = start_mock_ws_server(state.clone()).await;

    let mut client = CoinbaseWebSocketClient::new(&ws_url(addr), TransportBackend::default(), None);
    client.connect().await.unwrap();

    let result = client
        .subscribe(CoinbaseWsChannel::User, &[Ustr::from("BTC-USD")])
        .await;
    assert!(result.is_err(), "user channel must require credentials");

    // The failed subscribe must not leak onto the wire.
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(state.received_subscribes().await.is_empty());

    client.disconnect().await;
}

#[rstest]
#[tokio::test]
async fn test_ws_resubscribes_topics_after_reconnect() {
    // Drop the connection right after the first subscribe lands so the
    // client's reconnect path fires. The retained subscription state should
    // re-issue the same subscribe on the new connection.
    let state = WsServerState::default();
    *state.drop_after_subscribe.lock().await = true;
    let addr = start_mock_ws_server(state.clone()).await;

    let mut client = CoinbaseWebSocketClient::new(&ws_url(addr), TransportBackend::default(), None);
    client.connect().await.unwrap();

    let product = Ustr::from("BTC-USD");
    client
        .subscribe(CoinbaseWsChannel::Ticker, &[product])
        .await
        .unwrap();

    // Wait for the server to receive the second connection, then for the
    // resubscribe to land. Splitting the waits keeps the failure mode clear:
    // a connection_count failure points at the network reconnect path; a
    // subscribe-count failure points at the resubscribe-replay path.
    let connect_ok = tokio::time::timeout(
        Duration::from_secs(15),
        wait_until_async(
            || {
                let state = state.clone();
                async move { state.connection_count.load(Ordering::SeqCst) >= 2 }
            },
            Duration::from_secs(15),
        ),
    )
    .await
    .is_ok();
    assert!(
        connect_ok,
        "client did not reconnect within 15s (connection_count={})",
        state.connection_count.load(Ordering::SeqCst)
    );

    let resub_ok = tokio::time::timeout(
        Duration::from_secs(10),
        wait_until_async(
            || {
                let state = state.clone();
                async move { state.received_subscribes().await.len() >= 2 }
            },
            Duration::from_secs(10),
        ),
    )
    .await
    .is_ok();

    if !resub_ok {
        let raw = state.received_messages.lock().await.clone();
        panic!(
            "resubscribe did not fire (connection_count={}, captured_messages={:?})",
            state.connection_count.load(Ordering::SeqCst),
            raw
        );
    }

    // Exactly two ticker subscribes are expected: the user-issued one, then
    // the replay on reconnect. Any duplicate send (a class of bug this test
    // is here to catch) would push the count higher.
    let subs = state.received_subscribes().await;
    assert_eq!(
        subs.len(),
        2,
        "expected exactly 2 ticker subscribes (initial + replay), was {}: {subs:?}",
        subs.len()
    );

    for sub in &subs {
        assert_eq!(sub.get("channel").and_then(|c| c.as_str()), Some("ticker"));
    }

    client.disconnect().await;
}

#[rstest]
#[tokio::test]
async fn test_ws_emits_reconnected_message_after_drop() {
    let state = WsServerState::default();
    *state.drop_after_subscribe.lock().await = true;
    let addr = start_mock_ws_server(state.clone()).await;

    let mut client = CoinbaseWebSocketClient::new(&ws_url(addr), TransportBackend::default(), None);
    client.connect().await.unwrap();

    let product = Ustr::from("BTC-USD");
    client
        .subscribe(CoinbaseWsChannel::Ticker, &[product])
        .await
        .unwrap();

    // Drain messages with a generous timeout while watching for Reconnected.
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    let mut saw_reconnected = false;

    while std::time::Instant::now() < deadline {
        match tokio::time::timeout(Duration::from_millis(200), client.next_message()).await {
            Ok(Some(NautilusWsMessage::Reconnected)) => {
                saw_reconnected = true;
                break;
            }
            Ok(None) => break,
            Ok(Some(_)) | Err(_) => {}
        }
    }
    assert!(saw_reconnected, "client did not emit Reconnected sentinel");

    client.disconnect().await;
}

#[rstest]
#[tokio::test]
async fn test_ws_handles_malformed_message_gracefully() {
    // The mock server emits a non-JSON text frame on connect. The client's
    // parser must log the failure and stay connected; subsequent legitimate
    // subscribes still work end-to-end.
    let state = WsServerState::default();
    *state.inject_malformed.lock().await = true;
    let addr = start_mock_ws_server(state.clone()).await;

    let mut client = CoinbaseWebSocketClient::new(&ws_url(addr), TransportBackend::default(), None);
    client.connect().await.unwrap();
    assert!(client.is_active());

    // Subsequent subscribe still reaches the server.
    client
        .subscribe(CoinbaseWsChannel::Ticker, &[Ustr::from("BTC-USD")])
        .await
        .unwrap();

    wait_until_async(
        || {
            let state = state.clone();
            async move { !state.received_subscribes().await.is_empty() }
        },
        Duration::from_secs(2),
    )
    .await;

    assert!(
        client.is_active(),
        "connection dropped after malformed frame"
    );

    client.disconnect().await;
}

#[rstest]
#[tokio::test]
async fn test_ws_multiple_subscribes_each_reach_server() {
    let state = WsServerState::default();
    let addr = start_mock_ws_server(state.clone()).await;

    let mut client = CoinbaseWebSocketClient::new(&ws_url(addr), TransportBackend::default(), None);
    client.connect().await.unwrap();

    let product = Ustr::from("BTC-USD");
    client
        .subscribe(CoinbaseWsChannel::MarketTrades, &[product])
        .await
        .unwrap();
    client
        .subscribe(CoinbaseWsChannel::Ticker, &[product])
        .await
        .unwrap();
    client
        .subscribe(CoinbaseWsChannel::Level2, &[product])
        .await
        .unwrap();

    wait_until_async(
        || {
            let state = state.clone();
            async move { state.received_subscribes().await.len() >= 3 }
        },
        Duration::from_secs(2),
    )
    .await;

    // Exact-count assertions: each channel appears exactly once. A
    // `contains` check would mask a duplicate-send regression on the wire.
    let subs = state.received_subscribes().await;
    assert_eq!(subs.len(), 3, "expected exactly 3 subscribes, was {subs:?}");
    let mut counts: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();

    for sub in &subs {
        if let Some(ch) = sub.get("channel").and_then(|c| c.as_str()) {
            *counts.entry(ch).or_insert(0) += 1;
        }
    }
    assert_eq!(counts.get("market_trades").copied(), Some(1));
    assert_eq!(counts.get("ticker").copied(), Some(1));
    assert_eq!(counts.get("level2").copied(), Some(1));

    client.disconnect().await;
}
