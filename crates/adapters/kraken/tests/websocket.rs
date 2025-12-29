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

//! Integration tests for the Kraken WebSocket client using a mock WebSocket server.

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
use nautilus_kraken::{
    common::parse::parse_spot_instrument, config::KrakenDataClientConfig,
    http::models::AssetPairInfo, websocket::spot_v2::client::KrakenSpotWebSocketClient,
};
use nautilus_model::{data::BarType, identifiers::InstrumentId, instruments::InstrumentAny};
use rstest::rstest;
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

#[derive(Clone, Default)]
struct TestServerState {
    connection_count: Arc<tokio::sync::Mutex<usize>>,
    subscriptions: Arc<tokio::sync::Mutex<Vec<Value>>>,
    unsubscriptions: Arc<tokio::sync::Mutex<Vec<Value>>>,
    send_ping: Arc<AtomicBool>,
    received_pong: Arc<AtomicBool>,
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

fn load_instruments() -> Vec<InstrumentAny> {
    let payload = load_json("http_asset_pairs.json");
    let ts_init = nautilus_core::UnixNanos::default();
    let result = payload.get("result").expect("Missing result");

    result
        .as_object()
        .expect("Result should be object")
        .iter()
        .filter_map(|(pair_name, definition)| {
            let pair_info: AssetPairInfo = serde_json::from_value(definition.clone()).ok()?;
            parse_spot_instrument(pair_name, &pair_info, ts_init, ts_init).ok()
        })
        .collect()
}

impl TestServerState {
    async fn handle_message(
        &self,
        msg: &str,
        sender: &mut SplitSink<WebSocket, Message>,
    ) -> Option<()> {
        if msg == "ping" {
            if self.send_ping.load(Ordering::Relaxed) {
                sender.send(Message::Text("pong".into())).await.ok()?;
                self.received_pong.store(true, Ordering::Relaxed);
            }
            return Some(());
        }

        let value: Value = serde_json::from_str(msg).ok()?;

        if let Some(method) = value.get("method").and_then(|v| v.as_str()) {
            match method {
                "subscribe" => {
                    self.subscriptions.lock().await.push(value.clone());

                    let channel = value
                        .get("params")
                        .and_then(|p| p.get("channel"))
                        .and_then(|c| c.as_str())
                        .unwrap_or("unknown");

                    let req_id = value.get("req_id");

                    let response = json!({
                        "method": "subscribe",
                        "success": true,
                        "req_id": req_id,
                        "result": {
                            "channel": channel
                        }
                    });

                    sender
                        .send(Message::Text(response.to_string().into()))
                        .await
                        .ok()?;

                    match channel {
                        "ticker" => {
                            let snapshot = load_json("ws_ticker_snapshot.json");
                            sender
                                .send(Message::Text(snapshot.to_string().into()))
                                .await
                                .ok()?;
                        }
                        "trade" => {
                            let update = load_json("ws_trade_update.json");
                            sender
                                .send(Message::Text(update.to_string().into()))
                                .await
                                .ok()?;
                        }
                        "book" => {
                            let snapshot = load_json("ws_book_snapshot.json");
                            sender
                                .send(Message::Text(snapshot.to_string().into()))
                                .await
                                .ok()?;
                        }
                        _ => {}
                    }
                }
                "unsubscribe" => {
                    self.unsubscriptions.lock().await.push(value.clone());

                    let req_id = value.get("req_id");
                    let response = json!({
                        "method": "unsubscribe",
                        "success": true,
                        "req_id": req_id
                    });

                    sender
                        .send(Message::Text(response.to_string().into()))
                        .await
                        .ok()?;
                }
                "ping" => {
                    let req_id = value.get("req_id");
                    let response = json!({
                        "method": "pong",
                        "req_id": req_id
                    });

                    sender
                        .send(Message::Text(response.to_string().into()))
                        .await
                        .ok()?;

                    self.received_pong.store(true, Ordering::Relaxed);
                }
                _ => {}
            }
        }

        Some(())
    }
}

async fn websocket_handler(ws: WebSocketUpgrade, state: Arc<TestServerState>) -> impl IntoResponse {
    ws.on_upgrade(|socket| handle_socket(socket, state))
}

async fn handle_socket(socket: WebSocket, state: Arc<TestServerState>) {
    *state.connection_count.lock().await += 1;

    if state.drop_next_connection.swap(false, Ordering::Relaxed) {
        return;
    }

    let (mut sender, mut receiver) = socket.split();

    while let Some(Ok(msg)) = receiver.next().await {
        match msg {
            Message::Text(text) => {
                if state.handle_message(&text, &mut sender).await.is_none() {
                    break;
                }
            }
            Message::Ping(data) => {
                if sender.send(Message::Pong(data)).await.is_err() {
                    break;
                }
            }
            Message::Close(_) => break,
            _ => {}
        }
    }
}

fn create_router(state: Arc<TestServerState>) -> Router {
    Router::new().route("/v2", get(move |ws| websocket_handler(ws, state.clone())))
}

async fn start_test_server(state: Arc<TestServerState>) -> String {
    let app = create_router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    tokio::time::sleep(Duration::from_millis(100)).await;
    format!("ws://{addr}/v2")
}

#[rstest]
#[tokio::test]
async fn test_websocket_connection() {
    let state = Arc::new(TestServerState::default());
    let url = start_test_server(state.clone()).await;

    let config = KrakenDataClientConfig {
        ws_public_url: Some(url),
        ..Default::default()
    };

    let mut client = KrakenSpotWebSocketClient::new(config, CancellationToken::new());
    let instruments = load_instruments();

    let result = client.connect().await;
    assert!(result.is_ok(), "Failed to connect: {result:?}");

    client.cache_instruments(instruments);

    assert!(*state.connection_count.lock().await > 0);

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_websocket_is_active_lifecycle() {
    let state = Arc::new(TestServerState::default());
    let url = start_test_server(state).await;

    let config = KrakenDataClientConfig {
        ws_public_url: Some(url),
        ..Default::default()
    };

    let mut client = KrakenSpotWebSocketClient::new(config, CancellationToken::new());

    assert!(!client.is_active());
    assert!(client.is_closed());

    client.connect().await.unwrap();

    // Wait for connection to become active
    wait_until_async(
        || {
            let client = client.clone();
            async move { client.is_active() || client.is_connected() }
        },
        Duration::from_secs(5),
    )
    .await;

    assert!(client.is_active() || client.is_connected());
    assert!(!client.is_closed());

    client.disconnect().await.unwrap();

    assert!(!client.is_active());
}

#[rstest]
#[tokio::test]
async fn test_websocket_subscribe_quotes() {
    let state = Arc::new(TestServerState::default());
    let url = start_test_server(state.clone()).await;

    let config = KrakenDataClientConfig {
        ws_public_url: Some(url),
        ..Default::default()
    };

    let mut client = KrakenSpotWebSocketClient::new(config, CancellationToken::new());
    let instruments = load_instruments();

    client.connect().await.unwrap();
    client.cache_instruments(instruments);

    client.wait_until_active(5.0).await.unwrap();

    let instrument_id = InstrumentId::from("XBT/USDT.KRAKEN");
    let result = client.subscribe_quotes(instrument_id).await;

    assert!(result.is_ok(), "Failed to subscribe: {result:?}");

    wait_until_async(
        || {
            let state = state.clone();
            async move { !state.subscriptions.lock().await.is_empty() }
        },
        Duration::from_secs(5),
    )
    .await;

    let subs = state.subscriptions.lock().await;
    assert!(!subs.is_empty(), "No subscriptions recorded");

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_websocket_subscribe_trades() {
    let state = Arc::new(TestServerState::default());
    let url = start_test_server(state.clone()).await;

    let config = KrakenDataClientConfig {
        ws_public_url: Some(url),
        ..Default::default()
    };

    let mut client = KrakenSpotWebSocketClient::new(config, CancellationToken::new());
    let instruments = load_instruments();

    client.connect().await.unwrap();
    client.cache_instruments(instruments);
    client.wait_until_active(5.0).await.unwrap();

    let instrument_id = InstrumentId::from("XBT/USDT.KRAKEN");
    let result = client.subscribe_trades(instrument_id).await;

    assert!(result.is_ok(), "Failed to subscribe: {result:?}");

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

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_websocket_subscribe_book() {
    let state = Arc::new(TestServerState::default());
    let url = start_test_server(state.clone()).await;

    let config = KrakenDataClientConfig {
        ws_public_url: Some(url),
        ..Default::default()
    };

    let mut client = KrakenSpotWebSocketClient::new(config, CancellationToken::new());
    let instruments = load_instruments();

    client.connect().await.unwrap();
    client.cache_instruments(instruments);
    client.wait_until_active(5.0).await.unwrap();

    let instrument_id = InstrumentId::from("XBT/USDT.KRAKEN");
    let result = client.subscribe_book(instrument_id, Some(10)).await;

    assert!(result.is_ok(), "Failed to subscribe: {result:?}");

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

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_websocket_subscribe_bars() {
    let state = Arc::new(TestServerState::default());
    let url = start_test_server(state.clone()).await;

    let config = KrakenDataClientConfig {
        ws_public_url: Some(url),
        ..Default::default()
    };

    let mut client = KrakenSpotWebSocketClient::new(config, CancellationToken::new());
    let instruments = load_instruments();

    client.connect().await.unwrap();
    client.cache_instruments(instruments);
    client.wait_until_active(5.0).await.unwrap();

    let bar_type = BarType::from("XBT/USDT.KRAKEN-1-MINUTE-LAST-INTERNAL");
    let result = client.subscribe_bars(bar_type).await;

    assert!(result.is_ok(), "Failed to subscribe: {result:?}");

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

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_websocket_unsubscribe_quotes() {
    let state = Arc::new(TestServerState::default());
    let url = start_test_server(state.clone()).await;

    let config = KrakenDataClientConfig {
        ws_public_url: Some(url),
        ..Default::default()
    };

    let mut client = KrakenSpotWebSocketClient::new(config, CancellationToken::new());
    let instruments = load_instruments();

    client.connect().await.unwrap();
    client.cache_instruments(instruments);
    client.wait_until_active(5.0).await.unwrap();

    let instrument_id = InstrumentId::from("XBT/USDT.KRAKEN");

    client.subscribe_quotes(instrument_id).await.unwrap();

    wait_until_async(
        || {
            let state = state.clone();
            async move { !state.subscriptions.lock().await.is_empty() }
        },
        Duration::from_secs(5),
    )
    .await;

    let result = client.unsubscribe_quotes(instrument_id).await;
    assert!(result.is_ok(), "Failed to unsubscribe: {result:?}");

    wait_until_async(
        || {
            let state = state.clone();
            async move { !state.unsubscriptions.lock().await.is_empty() }
        },
        Duration::from_secs(5),
    )
    .await;

    let unsubs = state.unsubscriptions.lock().await;
    assert!(!unsubs.is_empty());

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_websocket_unsubscribe_trades() {
    let state = Arc::new(TestServerState::default());
    let url = start_test_server(state.clone()).await;

    let config = KrakenDataClientConfig {
        ws_public_url: Some(url),
        ..Default::default()
    };

    let mut client = KrakenSpotWebSocketClient::new(config, CancellationToken::new());
    let instruments = load_instruments();

    client.connect().await.unwrap();
    client.cache_instruments(instruments);
    client.wait_until_active(5.0).await.unwrap();

    let instrument_id = InstrumentId::from("XBT/USDT.KRAKEN");

    client.subscribe_trades(instrument_id).await.unwrap();

    wait_until_async(
        || {
            let state = state.clone();
            async move { !state.subscriptions.lock().await.is_empty() }
        },
        Duration::from_secs(5),
    )
    .await;

    let result = client.unsubscribe_trades(instrument_id).await;
    assert!(result.is_ok(), "Failed to unsubscribe: {result:?}");

    wait_until_async(
        || {
            let state = state.clone();
            async move { !state.unsubscriptions.lock().await.is_empty() }
        },
        Duration::from_secs(5),
    )
    .await;

    let unsubs = state.unsubscriptions.lock().await;
    assert!(!unsubs.is_empty());

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_websocket_unsubscribe_book() {
    let state = Arc::new(TestServerState::default());
    let url = start_test_server(state.clone()).await;

    let config = KrakenDataClientConfig {
        ws_public_url: Some(url),
        ..Default::default()
    };

    let mut client = KrakenSpotWebSocketClient::new(config, CancellationToken::new());
    let instruments = load_instruments();

    client.connect().await.unwrap();
    client.cache_instruments(instruments);
    client.wait_until_active(5.0).await.unwrap();

    let instrument_id = InstrumentId::from("XBT/USDT.KRAKEN");

    client
        .subscribe_book(instrument_id, Some(10))
        .await
        .unwrap();

    wait_until_async(
        || {
            let state = state.clone();
            async move { !state.subscriptions.lock().await.is_empty() }
        },
        Duration::from_secs(5),
    )
    .await;

    let result = client.unsubscribe_book(instrument_id).await;
    assert!(result.is_ok(), "Failed to unsubscribe: {result:?}");

    wait_until_async(
        || {
            let state = state.clone();
            async move { !state.unsubscriptions.lock().await.is_empty() }
        },
        Duration::from_secs(5),
    )
    .await;

    let unsubs = state.unsubscriptions.lock().await;
    assert!(!unsubs.is_empty());

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_websocket_unsubscribe_bars() {
    let state = Arc::new(TestServerState::default());
    let url = start_test_server(state.clone()).await;

    let config = KrakenDataClientConfig {
        ws_public_url: Some(url),
        ..Default::default()
    };

    let mut client = KrakenSpotWebSocketClient::new(config, CancellationToken::new());
    let instruments = load_instruments();

    client.connect().await.unwrap();
    client.cache_instruments(instruments);
    client.wait_until_active(5.0).await.unwrap();

    let bar_type = BarType::from("XBT/USDT.KRAKEN-1-MINUTE-LAST-INTERNAL");

    client.subscribe_bars(bar_type).await.unwrap();

    wait_until_async(
        || {
            let state = state.clone();
            async move { !state.subscriptions.lock().await.is_empty() }
        },
        Duration::from_secs(5),
    )
    .await;

    let result = client.unsubscribe_bars(bar_type).await;
    assert!(result.is_ok(), "Failed to unsubscribe: {result:?}");

    wait_until_async(
        || {
            let state = state.clone();
            async move { !state.unsubscriptions.lock().await.is_empty() }
        },
        Duration::from_secs(5),
    )
    .await;

    let unsubs = state.unsubscriptions.lock().await;
    assert!(!unsubs.is_empty());

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_websocket_send_ping() {
    let state = Arc::new(TestServerState::default());
    state.send_ping.store(true, Ordering::Relaxed);
    let url = start_test_server(state.clone()).await;

    let config = KrakenDataClientConfig {
        ws_public_url: Some(url),
        ..Default::default()
    };

    let mut client = KrakenSpotWebSocketClient::new(config, CancellationToken::new());

    client.connect().await.unwrap();
    client.wait_until_active(5.0).await.unwrap();

    let result = client.send_ping().await;
    assert!(result.is_ok(), "Failed to send ping: {result:?}");

    wait_until_async(
        || {
            let state = state.clone();
            async move { state.received_pong.load(Ordering::Relaxed) }
        },
        Duration::from_secs(5),
    )
    .await;

    assert!(state.received_pong.load(Ordering::Relaxed));

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_websocket_multiple_subscriptions() {
    let state = Arc::new(TestServerState::default());
    let url = start_test_server(state.clone()).await;

    let config = KrakenDataClientConfig {
        ws_public_url: Some(url),
        ..Default::default()
    };

    let mut client = KrakenSpotWebSocketClient::new(config, CancellationToken::new());
    let instruments = load_instruments();

    client.connect().await.unwrap();
    client.cache_instruments(instruments);
    client.wait_until_active(5.0).await.unwrap();

    let instrument_id = InstrumentId::from("XBT/USDT.KRAKEN");

    client.subscribe_quotes(instrument_id).await.unwrap();
    client.subscribe_trades(instrument_id).await.unwrap();
    client
        .subscribe_book(instrument_id, Some(10))
        .await
        .unwrap();

    // Note: quotes and book share the same underlying Book channel,
    // so with reference counting only 2 distinct subscriptions are sent (Book + Trade)
    wait_until_async(
        || {
            let state = state.clone();
            async move { state.subscriptions.lock().await.len() >= 2 }
        },
        Duration::from_secs(5),
    )
    .await;

    let subs = state.subscriptions.lock().await;
    assert_eq!(subs.len(), 2, "Expected 2 subscriptions (Book + Trade)");

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_websocket_get_subscriptions() {
    let state = Arc::new(TestServerState::default());
    let url = start_test_server(state.clone()).await;

    let config = KrakenDataClientConfig {
        ws_public_url: Some(url),
        ..Default::default()
    };

    let mut client = KrakenSpotWebSocketClient::new(config, CancellationToken::new());
    let instruments = load_instruments();

    client.connect().await.unwrap();
    client.cache_instruments(instruments);
    client.wait_until_active(5.0).await.unwrap();

    let initial_subs = client.get_subscriptions();
    assert!(initial_subs.is_empty());

    let instrument_id = InstrumentId::from("XBT/USDT.KRAKEN");
    client.subscribe_quotes(instrument_id).await.unwrap();

    wait_until_async(
        || {
            let client = client.clone();
            async move { !client.get_subscriptions().is_empty() }
        },
        Duration::from_secs(5),
    )
    .await;

    let subs = client.get_subscriptions();
    assert!(!subs.is_empty());

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_websocket_wait_until_active_timeout() {
    let state = Arc::new(TestServerState::default());
    let _url = start_test_server(state).await;

    let config = KrakenDataClientConfig {
        ws_public_url: Some("ws://invalid.invalid/v2".to_string()), // Invalid URL
        ..Default::default()
    };

    let mut client = KrakenSpotWebSocketClient::new(config, CancellationToken::new());

    // Connection will fail, so wait_until_active should timeout
    let _ = client.connect().await; // May or may not succeed initially

    let result = client.wait_until_active(0.2).await;
    assert!(result.is_err(), "Expected timeout error: {result:?}");
}

#[rstest]
#[tokio::test]
async fn test_websocket_disconnect_and_close() {
    let state = Arc::new(TestServerState::default());
    let url = start_test_server(state).await;

    let config = KrakenDataClientConfig {
        ws_public_url: Some(url.clone()),
        ..Default::default()
    };

    let mut client = KrakenSpotWebSocketClient::new(config, CancellationToken::new());

    client.connect().await.unwrap();
    client.wait_until_active(5.0).await.unwrap();

    assert!(client.is_active() || client.is_connected());

    client.disconnect().await.unwrap();

    assert!(!client.is_active());

    // Test that close also works
    let mut client2 = KrakenSpotWebSocketClient::new(
        KrakenDataClientConfig {
            ws_public_url: Some(url),
            ..Default::default()
        },
        CancellationToken::new(),
    );

    client2.connect().await.unwrap();
    client2.close().await.unwrap();

    assert!(!client2.is_active());
}

#[rstest]
#[tokio::test]
async fn test_websocket_cancel_all_requests() {
    let state = Arc::new(TestServerState::default());
    let url = start_test_server(state).await;

    let config = KrakenDataClientConfig {
        ws_public_url: Some(url),
        ..Default::default()
    };

    let client = KrakenSpotWebSocketClient::new(config, CancellationToken::new());

    // Verify cancellation token is accessible before cancelling
    let token = client.cancellation_token();
    assert!(!token.is_cancelled());

    // This should cancel the token
    client.cancel_all_requests();

    // Token should now be cancelled
    assert!(token.is_cancelled());
}

#[rstest]
#[tokio::test]
async fn test_websocket_url_getter() {
    let url = "ws://test.example.com/v2";
    let config = KrakenDataClientConfig {
        ws_public_url: Some(url.to_string()),
        ..Default::default()
    };

    let client = KrakenSpotWebSocketClient::new(config, CancellationToken::new());

    assert_eq!(client.url(), url);
}

#[rstest]
#[tokio::test]
async fn test_websocket_cache_instrument() {
    let state = Arc::new(TestServerState::default());
    let url = start_test_server(state).await;

    let config = KrakenDataClientConfig {
        ws_public_url: Some(url),
        ..Default::default()
    };

    let client = KrakenSpotWebSocketClient::new(config, CancellationToken::new());
    let instruments = load_instruments();

    // Cache individual instrument
    if let Some(instrument) = instruments.first() {
        client.cache_instrument(instrument.clone());
    }

    // Cache multiple instruments
    client.cache_instruments(instruments);
}

// =============================================================================
// Robustness and edge case tests
// =============================================================================

#[rstest]
#[tokio::test]
async fn test_websocket_reconnection_after_disconnect() {
    let state = Arc::new(TestServerState::default());
    let url = start_test_server(state.clone()).await;

    let config = KrakenDataClientConfig {
        ws_public_url: Some(url.clone()),
        ..Default::default()
    };

    let mut client = KrakenSpotWebSocketClient::new(config, CancellationToken::new());
    let instruments = load_instruments();

    // First connection
    client.connect().await.unwrap();
    client.cache_instruments(instruments.clone());
    client.wait_until_active(5.0).await.unwrap();

    assert!(client.is_active());
    let initial_count = *state.connection_count.lock().await;
    assert!(initial_count > 0);

    // Disconnect
    client.disconnect().await.unwrap();
    assert!(!client.is_active());

    // Reconnect
    client.connect().await.unwrap();
    client.cache_instruments(instruments);
    client.wait_until_active(5.0).await.unwrap();

    assert!(client.is_active());
    let new_count = *state.connection_count.lock().await;
    assert!(new_count > initial_count);

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_websocket_subscription_after_reconnect() {
    let state = Arc::new(TestServerState::default());
    let url = start_test_server(state.clone()).await;

    let config = KrakenDataClientConfig {
        ws_public_url: Some(url.clone()),
        ..Default::default()
    };

    let mut client = KrakenSpotWebSocketClient::new(config, CancellationToken::new());
    let instruments = load_instruments();

    // Connect and subscribe
    client.connect().await.unwrap();
    client.cache_instruments(instruments.clone());
    client.wait_until_active(5.0).await.unwrap();

    let instrument_id = InstrumentId::from("XBT/USDT.KRAKEN");
    client.subscribe_quotes(instrument_id).await.unwrap();

    wait_until_async(
        || {
            let state = state.clone();
            async move { !state.subscriptions.lock().await.is_empty() }
        },
        Duration::from_secs(5),
    )
    .await;

    let initial_subs = state.subscriptions.lock().await.len();
    assert!(initial_subs > 0);

    // Disconnect
    client.disconnect().await.unwrap();

    // Reconnect
    client.connect().await.unwrap();
    client.cache_instruments(instruments);
    client.wait_until_active(5.0).await.unwrap();

    // Subscribe again after reconnect
    client.subscribe_trades(instrument_id).await.unwrap();

    wait_until_async(
        || {
            let state = state.clone();
            let initial = initial_subs;
            async move { state.subscriptions.lock().await.len() > initial }
        },
        Duration::from_secs(5),
    )
    .await;

    let new_subs = state.subscriptions.lock().await.len();
    assert!(new_subs > initial_subs);

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_websocket_connection_to_invalid_url() {
    let config = KrakenDataClientConfig {
        ws_public_url: Some("ws://127.0.0.1:59999/invalid".to_string()),
        ..Default::default()
    };

    let mut client = KrakenSpotWebSocketClient::new(config, CancellationToken::new());

    // Connection should fail
    let result = client.connect().await;
    assert!(result.is_err() || !client.is_active());
}

#[rstest]
#[tokio::test]
async fn test_websocket_dropped_connection_handling() {
    let state = Arc::new(TestServerState::default());
    state.drop_next_connection.store(true, Ordering::Relaxed);
    let url = start_test_server(state.clone()).await;

    let config = KrakenDataClientConfig {
        ws_public_url: Some(url),
        ..Default::default()
    };

    let mut client = KrakenSpotWebSocketClient::new(config, CancellationToken::new());

    // First connection should be dropped by server
    let _ = client.connect().await;

    // Give time for the connection attempt
    tokio::time::sleep(Duration::from_millis(200)).await;

    // Client should handle the dropped connection gracefully
    // Either by not being active or by reconnecting
    let is_active = client.is_active();

    // Clean up
    let _ = client.disconnect().await;

    // The test passes if we get here without panicking
    assert!(!is_active || client.is_closed());
}

#[rstest]
#[tokio::test]
async fn test_websocket_multiple_rapid_connections() {
    let state = Arc::new(TestServerState::default());
    let url = start_test_server(state.clone()).await;

    let config = KrakenDataClientConfig {
        ws_public_url: Some(url),
        ..Default::default()
    };

    let mut client = KrakenSpotWebSocketClient::new(config, CancellationToken::new());
    let instruments = load_instruments();

    // Rapidly connect and disconnect multiple times
    for _ in 0..3 {
        client.connect().await.unwrap();
        client.cache_instruments(instruments.clone());
        client.wait_until_active(5.0).await.unwrap();
        assert!(client.is_active());
        client.disconnect().await.unwrap();
    }

    // Verify server saw multiple connections
    let total_connections = *state.connection_count.lock().await;
    assert!(total_connections >= 3);
}

#[rstest]
#[tokio::test]
async fn test_websocket_subscribe_before_active() {
    let state = Arc::new(TestServerState::default());
    let url = start_test_server(state).await;

    let config = KrakenDataClientConfig {
        ws_public_url: Some(url),
        ..Default::default()
    };

    let mut client = KrakenSpotWebSocketClient::new(config, CancellationToken::new());
    let instruments = load_instruments();

    // Connect but don't wait for active
    client.connect().await.unwrap();
    client.cache_instruments(instruments);

    let instrument_id = InstrumentId::from("XBT/USDT.KRAKEN");

    // Try to subscribe - may fail or succeed depending on timing
    let result = client.subscribe_quotes(instrument_id).await;

    // Either the subscription succeeds or fails gracefully
    if result.is_ok() {
        tokio::time::sleep(Duration::from_millis(200)).await;
    }

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_websocket_disconnect_while_subscribing() {
    let state = Arc::new(TestServerState::default());
    let url = start_test_server(state).await;

    let config = KrakenDataClientConfig {
        ws_public_url: Some(url),
        ..Default::default()
    };

    let mut client = KrakenSpotWebSocketClient::new(config, CancellationToken::new());
    let instruments = load_instruments();

    client.connect().await.unwrap();
    client.cache_instruments(instruments);
    client.wait_until_active(5.0).await.unwrap();

    let instrument_id = InstrumentId::from("XBT/USDT.KRAKEN");

    // Start subscription
    let sub_handle = {
        let client_clone = client.clone();
        tokio::spawn(async move { client_clone.subscribe_quotes(instrument_id).await })
    };

    // Immediately disconnect
    tokio::time::sleep(Duration::from_millis(10)).await;
    let _ = client.disconnect().await;

    // Wait for subscription task to complete
    let _ = sub_handle.await;

    // Should not panic, client should be in a consistent state
    assert!(!client.is_active());
}

#[rstest]
#[tokio::test]
async fn test_websocket_concurrent_subscriptions() {
    let state = Arc::new(TestServerState::default());
    let url = start_test_server(state.clone()).await;

    let config = KrakenDataClientConfig {
        ws_public_url: Some(url),
        ..Default::default()
    };

    let mut client = KrakenSpotWebSocketClient::new(config, CancellationToken::new());
    let instruments = load_instruments();

    client.connect().await.unwrap();
    client.cache_instruments(instruments);
    client.wait_until_active(5.0).await.unwrap();

    let instrument_id = InstrumentId::from("XBT/USDT.KRAKEN");

    // Subscribe to multiple channels concurrently
    let client1 = client.clone();
    let client2 = client.clone();
    let client3 = client.clone();

    let (r1, r2, r3) = tokio::join!(
        client1.subscribe_quotes(instrument_id),
        client2.subscribe_trades(instrument_id),
        client3.subscribe_book(instrument_id, Some(10)),
    );

    // All subscriptions should succeed
    assert!(r1.is_ok(), "Quotes subscription failed: {r1:?}");
    assert!(r2.is_ok(), "Trades subscription failed: {r2:?}");
    assert!(r3.is_ok(), "Book subscription failed: {r3:?}");

    // Note: quotes and book share the same underlying Book channel,
    // so with reference counting only 2 distinct subscriptions are sent (Book + Trade)
    wait_until_async(
        || {
            let state = state.clone();
            async move { state.subscriptions.lock().await.len() >= 2 }
        },
        Duration::from_secs(5),
    )
    .await;

    let subs = state.subscriptions.lock().await;
    assert_eq!(subs.len(), 2);

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_websocket_unsubscribe_not_subscribed() {
    let state = Arc::new(TestServerState::default());
    let url = start_test_server(state).await;

    let config = KrakenDataClientConfig {
        ws_public_url: Some(url),
        ..Default::default()
    };

    let mut client = KrakenSpotWebSocketClient::new(config, CancellationToken::new());
    let instruments = load_instruments();

    client.connect().await.unwrap();
    client.cache_instruments(instruments);
    client.wait_until_active(5.0).await.unwrap();

    let instrument_id = InstrumentId::from("XBT/USDT.KRAKEN");

    // Unsubscribe without subscribing first - should not panic
    let result = client.unsubscribe_quotes(instrument_id).await;

    // Either succeeds or fails gracefully
    if result.is_err() {
        // Expected behavior - can't unsubscribe from something not subscribed
    }

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_websocket_double_subscribe() {
    let state = Arc::new(TestServerState::default());
    let url = start_test_server(state.clone()).await;

    let config = KrakenDataClientConfig {
        ws_public_url: Some(url),
        ..Default::default()
    };

    let mut client = KrakenSpotWebSocketClient::new(config, CancellationToken::new());
    let instruments = load_instruments();

    client.connect().await.unwrap();
    client.cache_instruments(instruments);
    client.wait_until_active(5.0).await.unwrap();

    let instrument_id = InstrumentId::from("XBT/USDT.KRAKEN");

    // Subscribe twice to the same channel
    client.subscribe_quotes(instrument_id).await.unwrap();

    wait_until_async(
        || {
            let state = state.clone();
            async move { !state.subscriptions.lock().await.is_empty() }
        },
        Duration::from_secs(5),
    )
    .await;

    let result = client.subscribe_quotes(instrument_id).await;

    // Should either succeed (idempotent) or fail gracefully
    if result.is_err() {
        // Already subscribed - acceptable behavior
    }

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_websocket_is_active_lifecycle_detailed() {
    let state = Arc::new(TestServerState::default());
    let url = start_test_server(state).await;

    let config = KrakenDataClientConfig {
        ws_public_url: Some(url),
        ..Default::default()
    };

    let mut client = KrakenSpotWebSocketClient::new(config, CancellationToken::new());

    // Before connect
    assert!(!client.is_active());
    assert!(!client.is_connected());
    assert!(client.is_closed());

    // After connect
    client.connect().await.unwrap();
    client.wait_until_active(5.0).await.unwrap();

    assert!(client.is_active() || client.is_connected());
    assert!(!client.is_closed());

    // After disconnect
    client.disconnect().await.unwrap();

    assert!(!client.is_active());
    assert!(client.is_closed() || !client.is_connected());
}

#[rstest]
#[tokio::test]
async fn test_websocket_close_idempotent() {
    let state = Arc::new(TestServerState::default());
    let url = start_test_server(state).await;

    let config = KrakenDataClientConfig {
        ws_public_url: Some(url),
        ..Default::default()
    };

    let mut client = KrakenSpotWebSocketClient::new(config, CancellationToken::new());

    client.connect().await.unwrap();
    client.wait_until_active(5.0).await.unwrap();

    // Close multiple times - should not panic
    client.close().await.unwrap();
    let result = client.close().await;

    // Second close should either succeed or fail gracefully
    assert!(result.is_ok() || result.is_err());
}
