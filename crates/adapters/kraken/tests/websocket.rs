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

// Tests

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
    tokio::time::sleep(Duration::from_millis(200)).await;

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

    tokio::time::sleep(Duration::from_millis(200)).await;

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

    tokio::time::sleep(Duration::from_millis(200)).await;

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

    tokio::time::sleep(Duration::from_millis(200)).await;

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

    tokio::time::sleep(Duration::from_millis(200)).await;

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
    tokio::time::sleep(Duration::from_millis(100)).await;

    let result = client.unsubscribe_quotes(instrument_id).await;
    assert!(result.is_ok(), "Failed to unsubscribe: {result:?}");

    tokio::time::sleep(Duration::from_millis(200)).await;

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
    tokio::time::sleep(Duration::from_millis(100)).await;

    let result = client.unsubscribe_trades(instrument_id).await;
    assert!(result.is_ok(), "Failed to unsubscribe: {result:?}");

    tokio::time::sleep(Duration::from_millis(200)).await;

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
    tokio::time::sleep(Duration::from_millis(100)).await;

    let result = client.unsubscribe_book(instrument_id).await;
    assert!(result.is_ok(), "Failed to unsubscribe: {result:?}");

    tokio::time::sleep(Duration::from_millis(200)).await;

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
    tokio::time::sleep(Duration::from_millis(100)).await;

    let result = client.unsubscribe_bars(bar_type).await;
    assert!(result.is_ok(), "Failed to unsubscribe: {result:?}");

    tokio::time::sleep(Duration::from_millis(200)).await;

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

    tokio::time::sleep(Duration::from_millis(200)).await;

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

    tokio::time::sleep(Duration::from_millis(300)).await;

    let subs = state.subscriptions.lock().await;
    assert_eq!(subs.len(), 3, "Expected 3 subscriptions");

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_websocket_get_subscriptions() {
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

    let initial_subs = client.get_subscriptions();
    assert!(initial_subs.is_empty());

    let instrument_id = InstrumentId::from("XBT/USDT.KRAKEN");
    client.subscribe_quotes(instrument_id).await.unwrap();

    tokio::time::sleep(Duration::from_millis(200)).await;

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
