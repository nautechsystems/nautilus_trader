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

//! Integration tests for the Kraken Futures WebSocket client using a mock WebSocket server.

use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use axum::{
    Router,
    extract::ws::{Message, WebSocket, WebSocketUpgrade},
    response::IntoResponse,
    routing::get,
};
use futures_util::{SinkExt, StreamExt, stream::SplitSink};
use nautilus_common::testing::wait_until_async;
use nautilus_kraken::websocket::futures::client::KrakenFuturesWebSocketClient;
use nautilus_model::identifiers::InstrumentId;
use rstest::rstest;
use serde_json::{Value, json};

#[derive(Clone, Default)]
struct TestServerState {
    connection_count: Arc<tokio::sync::Mutex<usize>>,
    subscriptions: Arc<tokio::sync::Mutex<Vec<Value>>>,
    unsubscriptions: Arc<tokio::sync::Mutex<Vec<Value>>>,
    #[allow(dead_code)]
    challenge_requests: Arc<tokio::sync::Mutex<Vec<Value>>>,
    drop_next_connection: Arc<AtomicBool>,
}

fn data_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test_data")
}

fn load_json(filename: &str) -> Value {
    let content = std::fs::read_to_string(data_path().join(filename))
        .unwrap_or_else(|_| panic!("Failed to read {filename}"));
    serde_json::from_str(&content).expect("Invalid JSON")
}

impl TestServerState {
    async fn handle_message(
        &self,
        msg: &str,
        sender: &mut SplitSink<WebSocket, Message>,
    ) -> Option<()> {
        let value: Value = serde_json::from_str(msg).ok()?;

        if let Some(event) = value.get("event").and_then(|v| v.as_str()) {
            match event {
                "subscribe" => {
                    self.subscriptions.lock().await.push(value.clone());

                    let feed = value.get("feed").and_then(|f| f.as_str()).unwrap_or("");
                    let product_ids = value
                        .get("product_ids")
                        .and_then(|p| p.as_array())
                        .cloned()
                        .unwrap_or_default();

                    let response = json!({
                        "event": "subscribed",
                        "feed": feed,
                        "product_ids": product_ids
                    });

                    sender
                        .send(Message::Text(response.to_string().into()))
                        .await
                        .ok()?;

                    match feed {
                        "trade" => {
                            let snapshot = load_json("ws_futures_trade_snapshot.json");
                            sender
                                .send(Message::Text(snapshot.to_string().into()))
                                .await
                                .ok()?;
                        }
                        "book" => {
                            let snapshot = load_json("ws_futures_book_snapshot.json");
                            sender
                                .send(Message::Text(snapshot.to_string().into()))
                                .await
                                .ok()?;
                        }
                        "ticker" => {
                            let ticker = load_json("ws_futures_ticker.json");
                            sender
                                .send(Message::Text(ticker.to_string().into()))
                                .await
                                .ok()?;
                        }
                        _ => {}
                    }
                }
                "unsubscribe" => {
                    self.unsubscriptions.lock().await.push(value.clone());

                    let feed = value.get("feed").and_then(|f| f.as_str()).unwrap_or("");
                    let product_ids = value
                        .get("product_ids")
                        .and_then(|p| p.as_array())
                        .cloned()
                        .unwrap_or_default();

                    let response = json!({
                        "event": "unsubscribed",
                        "feed": feed,
                        "product_ids": product_ids
                    });

                    sender
                        .send(Message::Text(response.to_string().into()))
                        .await
                        .ok()?;
                }
                "challenge" => {
                    self.challenge_requests.lock().await.push(value.clone());

                    let response = json!({
                        "event": "challenge",
                        "message": "test_challenge_string_12345"
                    });

                    sender
                        .send(Message::Text(response.to_string().into()))
                        .await
                        .ok()?;
                }
                _ => {}
            }
        }

        Some(())
    }
}

async fn ws_handler(ws: WebSocketUpgrade, state: Arc<TestServerState>) -> impl IntoResponse {
    ws.on_upgrade(|socket| handle_socket(socket, state))
}

async fn handle_socket(socket: WebSocket, state: Arc<TestServerState>) {
    if state.drop_next_connection.swap(false, Ordering::Relaxed) {
        return; // Drop connection immediately
    }

    *state.connection_count.lock().await += 1;
    let (mut sender, mut receiver) = socket.split();

    // Send info message on connect (Kraken Futures protocol)
    let info = json!({
        "event": "info",
        "version": 1
    });
    let _ = sender.send(Message::Text(info.to_string().into())).await;

    while let Some(msg) = receiver.next().await {
        if let Ok(Message::Text(text)) = msg
            && state.handle_message(&text, &mut sender).await.is_none()
        {
            break;
        }
    }
}

async fn start_test_server(
    state: Arc<TestServerState>,
) -> Result<std::net::SocketAddr, std::io::Error> {
    let app = Router::new().route("/ws/v1", get(move |ws| ws_handler(ws, state)));

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;

    tokio::spawn(async move {
        axum::serve(listener, app).await.ok();
    });

    tokio::time::sleep(Duration::from_millis(100)).await;

    Ok(addr)
}

// =============================================================================
// Connection Tests
// =============================================================================

#[rstest]
#[tokio::test]
async fn test_futures_websocket_connection() {
    let state = Arc::new(TestServerState::default());
    let addr = start_test_server(state.clone()).await.unwrap();
    let url = format!("ws://{addr}/ws/v1");

    let mut client = KrakenFuturesWebSocketClient::new(url, None);
    client.connect().await.unwrap();
    client.wait_until_active(5.0).await.unwrap();

    assert!(client.is_active());
    assert_eq!(*state.connection_count.lock().await, 1);

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_futures_websocket_subscribe_trades() {
    let state = Arc::new(TestServerState::default());
    let addr = start_test_server(state.clone()).await.unwrap();
    let url = format!("ws://{addr}/ws/v1");

    let mut client = KrakenFuturesWebSocketClient::new(url, None);
    client.connect().await.unwrap();
    client.wait_until_active(5.0).await.unwrap();

    let instrument_id = InstrumentId::from("PF_XBTUSD.KRAKEN");
    client.subscribe_trades(instrument_id).await.unwrap();

    wait_until_async(
        || {
            let state = state.clone();
            async move { !state.subscriptions.lock().await.is_empty() }
        },
        Duration::from_secs(5),
    )
    .await;

    let subs = state.subscriptions.lock().await;
    assert_eq!(subs.len(), 1);

    let sub = &subs[0];
    assert_eq!(sub.get("event").and_then(|e| e.as_str()), Some("subscribe"));
    assert_eq!(sub.get("feed").and_then(|f| f.as_str()), Some("trade"));

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_futures_websocket_subscribe_book() {
    let state = Arc::new(TestServerState::default());
    let addr = start_test_server(state.clone()).await.unwrap();
    let url = format!("ws://{addr}/ws/v1");

    let mut client = KrakenFuturesWebSocketClient::new(url, None);
    client.connect().await.unwrap();
    client.wait_until_active(5.0).await.unwrap();

    let instrument_id = InstrumentId::from("PF_XBTUSD.KRAKEN");
    client.subscribe_book(instrument_id, None).await.unwrap();

    wait_until_async(
        || {
            let state = state.clone();
            async move { !state.subscriptions.lock().await.is_empty() }
        },
        Duration::from_secs(5),
    )
    .await;

    let subs = state.subscriptions.lock().await;
    assert!(!subs.is_empty());

    let book_sub = subs
        .iter()
        .find(|s| s.get("feed").and_then(|f| f.as_str()) == Some("book"));
    assert!(book_sub.is_some(), "Expected book subscription");

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_futures_websocket_reconnection() {
    let state = Arc::new(TestServerState::default());
    let addr = start_test_server(state.clone()).await.unwrap();
    let url = format!("ws://{addr}/ws/v1");

    let mut client = KrakenFuturesWebSocketClient::new(url, None);
    client.connect().await.unwrap();
    client.wait_until_active(5.0).await.unwrap();

    assert!(client.is_active());
    assert_eq!(*state.connection_count.lock().await, 1);

    client.disconnect().await.unwrap();

    // Small delay to ensure disconnect completes
    tokio::time::sleep(Duration::from_millis(100)).await;

    client.connect().await.unwrap();
    client.wait_until_active(5.0).await.unwrap();

    assert!(client.is_active());
    assert_eq!(*state.connection_count.lock().await, 2);

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_futures_websocket_unsubscribe() {
    let state = Arc::new(TestServerState::default());
    let addr = start_test_server(state.clone()).await.unwrap();
    let url = format!("ws://{addr}/ws/v1");

    let mut client = KrakenFuturesWebSocketClient::new(url, None);
    client.connect().await.unwrap();
    client.wait_until_active(5.0).await.unwrap();

    let instrument_id = InstrumentId::from("PF_XBTUSD.KRAKEN");
    client.subscribe_trades(instrument_id).await.unwrap();

    wait_until_async(
        || {
            let state = state.clone();
            async move { !state.subscriptions.lock().await.is_empty() }
        },
        Duration::from_secs(5),
    )
    .await;

    client.unsubscribe_trades(instrument_id).await.unwrap();

    wait_until_async(
        || {
            let state = state.clone();
            async move { !state.unsubscriptions.lock().await.is_empty() }
        },
        Duration::from_secs(5),
    )
    .await;

    let unsubs = state.unsubscriptions.lock().await;
    assert_eq!(unsubs.len(), 1);

    let unsub = &unsubs[0];
    assert_eq!(
        unsub.get("event").and_then(|e| e.as_str()),
        Some("unsubscribe")
    );
    assert_eq!(unsub.get("feed").and_then(|f| f.as_str()), Some("trade"));

    client.disconnect().await.unwrap();
}
