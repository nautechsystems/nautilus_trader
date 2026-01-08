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

//! Integration tests for Ax WebSocket clients using a mock server.

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
use nautilus_architect_ax::{
    common::enums::{AxCandleWidth, AxMarketDataLevel, AxOrderSide, AxTimeInForce},
    websocket::{data::AxMdWebSocketClient, orders::AxOrdersWebSocketClient},
};
use nautilus_common::testing::wait_until_async;
use nautilus_model::{
    identifiers::{AccountId, ClientOrderId, InstrumentId, Symbol, Venue},
    instruments::{CryptoPerpetual, InstrumentAny},
    types::{Currency, Price, Quantity},
};
use rstest::rstest;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde_json::json;
use ustr::Ustr;

#[derive(Clone)]
struct TestServerState {
    connection_count: Arc<tokio::sync::Mutex<usize>>,
    subscriptions: Arc<tokio::sync::Mutex<Vec<String>>>,
    subscription_events: Arc<tokio::sync::Mutex<Vec<(String, bool)>>>,
    fail_next_subscriptions: Arc<tokio::sync::Mutex<Vec<String>>>,
    authenticated: Arc<AtomicBool>,
    disconnect_trigger: Arc<AtomicBool>,
    ping_count: Arc<AtomicUsize>,
    pong_count: Arc<AtomicUsize>,
    heartbeat_count: Arc<AtomicUsize>,
    messages_received: Arc<tokio::sync::Mutex<Vec<serde_json::Value>>>,
}

impl Default for TestServerState {
    fn default() -> Self {
        Self {
            connection_count: Arc::new(tokio::sync::Mutex::new(0)),
            subscriptions: Arc::new(tokio::sync::Mutex::new(Vec::new())),
            subscription_events: Arc::new(tokio::sync::Mutex::new(Vec::new())),
            fail_next_subscriptions: Arc::new(tokio::sync::Mutex::new(Vec::new())),
            authenticated: Arc::new(AtomicBool::new(false)),
            disconnect_trigger: Arc::new(AtomicBool::new(false)),
            ping_count: Arc::new(AtomicUsize::new(0)),
            pong_count: Arc::new(AtomicUsize::new(0)),
            heartbeat_count: Arc::new(AtomicUsize::new(0)),
            messages_received: Arc::new(tokio::sync::Mutex::new(Vec::new())),
        }
    }
}

impl TestServerState {
    async fn reset(&self) {
        *self.connection_count.lock().await = 0;
        self.subscriptions.lock().await.clear();
        self.subscription_events.lock().await.clear();
        self.fail_next_subscriptions.lock().await.clear();
        self.authenticated.store(false, Ordering::Relaxed);
        self.disconnect_trigger.store(false, Ordering::Relaxed);
        self.ping_count.store(0, Ordering::Relaxed);
        self.pong_count.store(0, Ordering::Relaxed);
        self.heartbeat_count.store(0, Ordering::Relaxed);
        self.messages_received.lock().await.clear();
    }

    async fn set_subscription_failures(&self, topics: Vec<String>) {
        *self.fail_next_subscriptions.lock().await = topics;
    }

    async fn subscription_events(&self) -> Vec<(String, bool)> {
        self.subscription_events.lock().await.clone()
    }

    async fn get_messages(&self) -> Vec<serde_json::Value> {
        self.messages_received.lock().await.clone()
    }
}

async fn handle_md_websocket(
    ws: WebSocketUpgrade,
    State(state): State<TestServerState>,
) -> Response {
    ws.on_upgrade(|socket| handle_md_socket(socket, state))
}

async fn handle_md_socket(mut socket: WebSocket, state: TestServerState) {
    {
        let mut count = state.connection_count.lock().await;
        *count += 1;
    }

    let state_clone = state.clone();
    let heartbeat_handle = tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(1));
        loop {
            interval.tick().await;
            if state_clone.disconnect_trigger.load(Ordering::Relaxed) {
                break;
            }
            state_clone.heartbeat_count.fetch_add(1, Ordering::Relaxed);
        }
    });

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

        match msg {
            Message::Text(text) => {
                let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
                    continue;
                };

                state.messages_received.lock().await.push(value.clone());

                let msg_type = value.get("type").and_then(|v| v.as_str());

                match msg_type {
                    Some("subscribe") => {
                        let symbol = value.get("symbol").and_then(|v| v.as_str()).unwrap_or("");
                        let level = value
                            .get("level")
                            .and_then(|v| v.as_str())
                            .unwrap_or("LEVEL_1");
                        let key = format!("{symbol}:{level}");

                        let fail_list = state.fail_next_subscriptions.lock().await.clone();
                        let should_fail = fail_list.contains(&key);

                        state
                            .subscription_events
                            .lock()
                            .await
                            .push((key.clone(), !should_fail));

                        if !should_fail {
                            let mut subs = state.subscriptions.lock().await;
                            if !subs.contains(&key) {
                                subs.push(key);
                            }
                        }

                        let book_file = match level {
                            "LEVEL_1" => "ws_md_book_l1.json",
                            "LEVEL_2" => "ws_md_book_l2.json",
                            "LEVEL_3" => "ws_md_book_l3.json",
                            _ => "ws_md_book_l1.json",
                        };

                        let book_msg = load_test_data(book_file);
                        if socket
                            .send(Message::Text(book_msg.to_string().into()))
                            .await
                            .is_err()
                        {
                            break;
                        }

                        let trade_msg = load_test_data("ws_md_trade.json");
                        if socket
                            .send(Message::Text(trade_msg.to_string().into()))
                            .await
                            .is_err()
                        {
                            break;
                        }
                    }
                    Some("unsubscribe") => {
                        let symbol = value.get("symbol").and_then(|v| v.as_str()).unwrap_or("");

                        let mut subs = state.subscriptions.lock().await;
                        subs.retain(|s| !s.starts_with(symbol));

                        let mut events = state.subscription_events.lock().await;
                        events.retain(|(t, _)| !t.starts_with(symbol));
                    }
                    Some("subscribe_candles") => {
                        let symbol = value.get("symbol").and_then(|v| v.as_str()).unwrap_or("");
                        let width = value.get("width").and_then(|v| v.as_str()).unwrap_or("1m");

                        let key = format!("{symbol}:candle:{width}");
                        state
                            .subscription_events
                            .lock()
                            .await
                            .push((key.clone(), true));

                        let mut subs = state.subscriptions.lock().await;
                        if !subs.contains(&key) {
                            subs.push(key);
                        }

                        let candle_msg = load_test_data("ws_md_candle.json");
                        if socket
                            .send(Message::Text(candle_msg.to_string().into()))
                            .await
                            .is_err()
                        {
                            break;
                        }
                    }
                    Some("unsubscribe_candles") => {
                        let symbol = value.get("symbol").and_then(|v| v.as_str()).unwrap_or("");

                        let mut subs = state.subscriptions.lock().await;
                        subs.retain(|s| !s.starts_with(&format!("{symbol}:candle")));
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
    }

    heartbeat_handle.abort();

    let mut count = state.connection_count.lock().await;
    *count = count.saturating_sub(1);
}

async fn handle_orders_websocket(
    ws: WebSocketUpgrade,
    State(state): State<TestServerState>,
) -> Response {
    ws.on_upgrade(|socket| handle_orders_socket(socket, state))
}

async fn handle_orders_socket(mut socket: WebSocket, state: TestServerState) {
    {
        let mut count = state.connection_count.lock().await;
        *count += 1;
    }

    state.authenticated.store(true, Ordering::Relaxed);

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

        match msg {
            Message::Text(text) => {
                let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
                    continue;
                };

                state.messages_received.lock().await.push(value.clone());

                let msg_type = value.get("t").and_then(|v| v.as_str());

                match msg_type {
                    Some("p") => {
                        let rid = value.get("rid").and_then(|v| v.as_i64()).unwrap_or(0);
                        let ack = json!({
                            "t": "a",
                            "rid": rid,
                            "oid": format!("order-{rid}"),
                            "s": value.get("s").and_then(|v| v.as_str()).unwrap_or(""),
                            "d": value.get("d").and_then(|v| v.as_str()).unwrap_or("BUY"),
                            "q": value.get("q").and_then(|v| v.as_i64()).unwrap_or(0),
                            "p": value.get("p").and_then(|v| v.as_str()).unwrap_or("0"),
                        });
                        if socket
                            .send(Message::Text(ack.to_string().into()))
                            .await
                            .is_err()
                        {
                            break;
                        }
                    }
                    Some("c") => {
                        let rid = value.get("rid").and_then(|v| v.as_i64()).unwrap_or(0);
                        let ack = json!({
                            "t": "x",
                            "rid": rid,
                            "oid": value.get("oid").and_then(|v| v.as_str()).unwrap_or(""),
                        });
                        if socket
                            .send(Message::Text(ack.to_string().into()))
                            .await
                            .is_err()
                        {
                            break;
                        }
                    }
                    Some("o") => {
                        let rid = value.get("rid").and_then(|v| v.as_i64()).unwrap_or(0);
                        let response = load_test_data("ws_orders_open_orders.json");
                        let mut response = response.clone();
                        if let Some(obj) = response.as_object_mut() {
                            obj.insert("rid".to_string(), json!(rid));
                        }
                        if socket
                            .send(Message::Text(response.to_string().into()))
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
    }

    let mut count = state.connection_count.lock().await;
    *count = count.saturating_sub(1);
}

fn load_test_data(filename: &str) -> serde_json::Value {
    let path = format!("{}/test_data/{filename}", env!("CARGO_MANIFEST_DIR"));
    let content = std::fs::read_to_string(&path).unwrap_or_else(|_| match filename {
            "ws_md_book_l1.json" => r#"{"t":"1","s":"BTCUSD-PERP","b":"50000.00","B":"1.0","a":"50001.00","A":"1.0","ts":"1234567890000000000"}"#.to_string(),
            "ws_md_book_l2.json" => r#"{"t":"2","s":"BTCUSD-PERP","b":[],"a":[],"ts":"1234567890000000000"}"#.to_string(),
            "ws_md_book_l3.json" => r#"{"t":"3","s":"BTCUSD-PERP","b":[],"a":[],"ts":"1234567890000000000"}"#.to_string(),
            "ws_md_trade.json" => r#"{"t":"s","s":"BTCUSD-PERP","p":"50000.00","q":"0.1","d":"BUY","tx":"123","ts":"1234567890000000000"}"#.to_string(),
            "ws_md_candle.json" => r#"{"t":"c","s":"BTCUSD-PERP","o":"50000","h":"50100","l":"49900","c":"50050","v":"100","ts":"1234567890000000000"}"#.to_string(),
            "ws_orders_open_orders.json" => r#"{"t":"O","orders":[]}"#.to_string(),
            _ => "{}".to_string(),
    });
    serde_json::from_str(&content).unwrap_or_else(|_| json!({}))
}

fn create_test_router(state: TestServerState) -> Router {
    Router::new()
        .route("/md/ws", get(handle_md_websocket))
        .route("/orders/ws", get(handle_orders_websocket))
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

    wait_until_async(
        || async { tokio::net::TcpStream::connect(addr).await.is_ok() },
        Duration::from_secs(5),
    )
    .await;

    Ok((addr, state))
}

async fn wait_for_connection(state: &TestServerState) {
    wait_until_async(
        || async { *state.connection_count.lock().await > 0 },
        Duration::from_secs(5),
    )
    .await;
}

fn create_test_instrument(symbol: &str) -> InstrumentAny {
    let instrument = CryptoPerpetual::new(
        InstrumentId::new(Symbol::new(symbol), Venue::new("AX")),
        Symbol::new(symbol),
        Currency::USD(),
        Currency::USD(),
        Currency::USD(),
        false,
        2,
        3,
        Price::new(0.01, 2),
        Quantity::new(0.001, 3),
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        Some(Decimal::new(1, 2)),
        Some(Decimal::new(5, 3)),
        Some(Decimal::new(2, 4)),
        Some(Decimal::new(5, 4)),
        0.into(),
        0.into(),
    );
    InstrumentAny::CryptoPerpetual(instrument)
}

#[rstest]
#[tokio::test]
async fn test_md_client_connection() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{addr}/md/ws");

    let mut client = AxMdWebSocketClient::new(ws_url, "test_token".to_string(), Some(30));

    client.connect().await.unwrap();
    wait_for_connection(&state).await;

    assert!(client.is_active());
    assert_eq!(*state.connection_count.lock().await, 1);

    client.close().await;
}

#[rstest]
#[tokio::test]
async fn test_md_client_url_accessor() {
    let ws_url = "ws://localhost:9999/md/ws".to_string();
    let client = AxMdWebSocketClient::new(ws_url.clone(), "test_token".to_string(), None);

    assert_eq!(client.url(), ws_url);
}

#[rstest]
#[tokio::test]
async fn test_md_client_not_active_before_connect() {
    let client = AxMdWebSocketClient::new(
        "ws://localhost:9999/md/ws".to_string(),
        "test_token".to_string(),
        None,
    );

    assert!(!client.is_active());
    assert!(client.is_closed());
}

#[rstest]
#[tokio::test]
async fn test_md_connection_failure_to_invalid_url() {
    let mut client = AxMdWebSocketClient::new(
        "ws://127.0.0.1:9999/invalid".to_string(),
        "test_token".to_string(),
        None,
    );

    let result = client.connect().await;
    assert!(result.is_err());
}

#[rstest]
#[tokio::test]
async fn test_md_close_sets_closed_flag() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{addr}/md/ws");

    let mut client = AxMdWebSocketClient::new(ws_url, "test_token".to_string(), None);

    client.connect().await.unwrap();
    wait_for_connection(&state).await;

    assert!(!client.is_closed());

    client.close().await;

    assert!(client.is_closed());
}

#[rstest]
#[tokio::test]
async fn test_md_disconnect_without_close() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{addr}/md/ws");

    let mut client = AxMdWebSocketClient::new(ws_url, "test_token".to_string(), None);

    client.connect().await.unwrap();
    wait_for_connection(&state).await;

    client.disconnect().await;

    wait_until_async(|| async { !client.is_active() }, Duration::from_secs(5)).await;

    // Disconnect doesn't set closed flag
    assert!(!client.is_closed());

    client.close().await;
}

#[rstest]
#[tokio::test]
async fn test_md_subscribe_l1() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{addr}/md/ws");

    let mut client = AxMdWebSocketClient::new(ws_url, "test_token".to_string(), None);

    client.connect().await.unwrap();
    wait_for_connection(&state).await;

    client
        .subscribe("BTCUSD-PERP", AxMarketDataLevel::Level1)
        .await
        .unwrap();

    wait_until_async(
        || async { !state.subscriptions.lock().await.is_empty() },
        Duration::from_secs(5),
    )
    .await;

    let subs = state.subscriptions.lock().await.clone();
    assert!(
        subs.iter()
            .any(|s| s.contains("BTCUSD-PERP") && s.contains("LEVEL_1"))
    );

    client.close().await;
}

#[rstest]
#[tokio::test]
async fn test_md_subscribe_l2() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{addr}/md/ws");

    let mut client = AxMdWebSocketClient::new(ws_url, "test_token".to_string(), None);

    client.connect().await.unwrap();
    wait_for_connection(&state).await;

    client
        .subscribe("BTCUSD-PERP", AxMarketDataLevel::Level2)
        .await
        .unwrap();

    wait_until_async(
        || async { !state.subscriptions.lock().await.is_empty() },
        Duration::from_secs(5),
    )
    .await;

    let subs = state.subscriptions.lock().await.clone();
    assert!(
        subs.iter()
            .any(|s| s.contains("BTCUSD-PERP") && s.contains("LEVEL_2"))
    );

    client.close().await;
}

#[rstest]
#[tokio::test]
async fn test_md_subscribe_l3() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{addr}/md/ws");

    let mut client = AxMdWebSocketClient::new(ws_url, "test_token".to_string(), None);

    client.connect().await.unwrap();
    wait_for_connection(&state).await;

    client
        .subscribe("BTCUSD-PERP", AxMarketDataLevel::Level3)
        .await
        .unwrap();

    wait_until_async(
        || async { !state.subscriptions.lock().await.is_empty() },
        Duration::from_secs(5),
    )
    .await;

    let subs = state.subscriptions.lock().await.clone();
    assert!(
        subs.iter()
            .any(|s| s.contains("BTCUSD-PERP") && s.contains("LEVEL_3"))
    );

    client.close().await;
}

#[rstest]
#[tokio::test]
async fn test_md_subscribe_multiple_symbols() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{addr}/md/ws");

    let mut client = AxMdWebSocketClient::new(ws_url, "test_token".to_string(), None);

    client.connect().await.unwrap();
    wait_for_connection(&state).await;

    client
        .subscribe("BTCUSD-PERP", AxMarketDataLevel::Level1)
        .await
        .unwrap();
    client
        .subscribe("ETHUSD-PERP", AxMarketDataLevel::Level2)
        .await
        .unwrap();
    client
        .subscribe("EURUSD-PERP", AxMarketDataLevel::Level1)
        .await
        .unwrap();

    wait_until_async(
        || async { state.subscriptions.lock().await.len() >= 3 },
        Duration::from_secs(5),
    )
    .await;

    let subs = state.subscriptions.lock().await.clone();
    assert!(subs.iter().any(|s| s.contains("BTCUSD-PERP")));
    assert!(subs.iter().any(|s| s.contains("ETHUSD-PERP")));
    assert!(subs.iter().any(|s| s.contains("EURUSD-PERP")));

    client.close().await;
}

#[rstest]
#[tokio::test]
async fn test_md_unsubscribe() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{addr}/md/ws");

    let mut client = AxMdWebSocketClient::new(ws_url, "test_token".to_string(), None);

    client.connect().await.unwrap();
    wait_for_connection(&state).await;

    client
        .subscribe("BTCUSD-PERP", AxMarketDataLevel::Level1)
        .await
        .unwrap();

    wait_until_async(
        || async { !state.subscriptions.lock().await.is_empty() },
        Duration::from_secs(5),
    )
    .await;

    client.unsubscribe("BTCUSD-PERP").await.unwrap();

    wait_until_async(
        || async { state.subscriptions.lock().await.is_empty() },
        Duration::from_secs(5),
    )
    .await;

    assert!(state.subscriptions.lock().await.is_empty());

    client.close().await;
}

#[rstest]
#[tokio::test]
async fn test_md_subscribe_candles() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{addr}/md/ws");

    let mut client = AxMdWebSocketClient::new(ws_url, "test_token".to_string(), None);

    client.connect().await.unwrap();
    wait_for_connection(&state).await;

    client
        .subscribe_candles("BTCUSD-PERP", AxCandleWidth::Minutes1)
        .await
        .unwrap();

    wait_until_async(
        || async { !state.subscriptions.lock().await.is_empty() },
        Duration::from_secs(5),
    )
    .await;

    let subs = state.subscriptions.lock().await.clone();
    assert!(
        subs.iter()
            .any(|s| s.contains("BTCUSD-PERP") && s.contains("candle"))
    );

    client.close().await;
}

#[rstest]
#[tokio::test]
async fn test_md_unsubscribe_candles() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{addr}/md/ws");

    let mut client = AxMdWebSocketClient::new(ws_url, "test_token".to_string(), None);

    client.connect().await.unwrap();
    wait_for_connection(&state).await;

    client
        .subscribe_candles("BTCUSD-PERP", AxCandleWidth::Minutes1)
        .await
        .unwrap();

    wait_until_async(
        || async { !state.subscriptions.lock().await.is_empty() },
        Duration::from_secs(5),
    )
    .await;

    client
        .unsubscribe_candles("BTCUSD-PERP", AxCandleWidth::Minutes1)
        .await
        .unwrap();

    wait_until_async(
        || async { state.subscriptions.lock().await.is_empty() },
        Duration::from_secs(5),
    )
    .await;

    assert!(state.subscriptions.lock().await.is_empty());

    client.close().await;
}

#[rstest]
#[tokio::test]
async fn test_md_subscription_count_starts_at_zero() {
    let client = AxMdWebSocketClient::new(
        "ws://localhost:9999/md/ws".to_string(),
        "test_token".to_string(),
        None,
    );

    assert_eq!(client.subscription_count(), 0);
}

#[rstest]
#[tokio::test]
async fn test_md_cache_instrument() {
    let client = AxMdWebSocketClient::new(
        "ws://localhost:9999/md/ws".to_string(),
        "test_token".to_string(),
        None,
    );

    let instrument = create_test_instrument("BTCUSD-PERP");
    client.cache_instrument(instrument);

    let cached = client.get_cached_instrument(&Ustr::from("BTCUSD-PERP"));
    assert!(cached.is_some());
}

#[rstest]
#[tokio::test]
async fn test_md_cache_multiple_instruments() {
    let client = AxMdWebSocketClient::new(
        "ws://localhost:9999/md/ws".to_string(),
        "test_token".to_string(),
        None,
    );

    client.cache_instrument(create_test_instrument("BTCUSD-PERP"));
    client.cache_instrument(create_test_instrument("ETHUSD-PERP"));
    client.cache_instrument(create_test_instrument("EURUSD-PERP"));

    assert!(
        client
            .get_cached_instrument(&Ustr::from("BTCUSD-PERP"))
            .is_some()
    );
    assert!(
        client
            .get_cached_instrument(&Ustr::from("ETHUSD-PERP"))
            .is_some()
    );
    assert!(
        client
            .get_cached_instrument(&Ustr::from("EURUSD-PERP"))
            .is_some()
    );
}

#[rstest]
#[tokio::test]
async fn test_md_get_cached_instrument_returns_none_for_unknown() {
    let client = AxMdWebSocketClient::new(
        "ws://localhost:9999/md/ws".to_string(),
        "test_token".to_string(),
        None,
    );

    let cached = client.get_cached_instrument(&Ustr::from("UNKNOWN-SYMBOL"));
    assert!(cached.is_none());
}

#[rstest]
#[tokio::test]
async fn test_md_ping_pong() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{addr}/md/ws");

    let mut client = AxMdWebSocketClient::new(
        ws_url,
        "test_token".to_string(),
        Some(1), // 1 second heartbeat
    );

    client.connect().await.unwrap();
    wait_for_connection(&state).await;

    wait_until_async(
        || async { state.ping_count.load(Ordering::Relaxed) > 0 },
        Duration::from_secs(5),
    )
    .await;

    assert!(state.ping_count.load(Ordering::Relaxed) > 0);

    client.close().await;
}

#[rstest]
#[tokio::test]
async fn test_md_server_disconnect_handling() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{addr}/md/ws");

    let mut client = AxMdWebSocketClient::new(ws_url, "test_token".to_string(), None);

    client.connect().await.unwrap();
    wait_for_connection(&state).await;

    client
        .subscribe("BTCUSD-PERP", AxMarketDataLevel::Level1)
        .await
        .unwrap();

    wait_until_async(
        || async { !state.subscriptions.lock().await.is_empty() },
        Duration::from_secs(5),
    )
    .await;

    state.disconnect_trigger.store(true, Ordering::Relaxed);

    wait_until_async(
        || async { *state.connection_count.lock().await == 0 },
        Duration::from_secs(5),
    )
    .await;

    client.close().await;
}

#[rstest]
#[tokio::test]
async fn test_md_reconnection_after_disconnect() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{addr}/md/ws");

    let mut client = AxMdWebSocketClient::new(ws_url.clone(), "test_token".to_string(), None);

    client.connect().await.unwrap();
    wait_for_connection(&state).await;

    let initial_count = *state.connection_count.lock().await;
    assert_eq!(initial_count, 1);

    state.disconnect_trigger.store(true, Ordering::Relaxed);

    wait_until_async(
        || async { *state.connection_count.lock().await == 0 },
        Duration::from_secs(5),
    )
    .await;

    state.reset().await;

    let mut client2 = AxMdWebSocketClient::new(ws_url, "test_token".to_string(), None);

    client2.connect().await.unwrap();
    wait_for_connection(&state).await;

    assert!(client2.is_active());

    client.close().await;
    client2.close().await;
}

#[rstest]
#[tokio::test]
async fn test_md_rapid_subscribe_unsubscribe() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{addr}/md/ws");

    let mut client = AxMdWebSocketClient::new(ws_url, "test_token".to_string(), None);

    client.connect().await.unwrap();
    wait_for_connection(&state).await;

    for _ in 0..5 {
        client
            .subscribe("BTCUSD-PERP", AxMarketDataLevel::Level1)
            .await
            .unwrap();
        client.unsubscribe("BTCUSD-PERP").await.unwrap();
    }

    client
        .subscribe("BTCUSD-PERP", AxMarketDataLevel::Level1)
        .await
        .unwrap();

    wait_until_async(
        || async { !state.subscriptions.lock().await.is_empty() },
        Duration::from_secs(5),
    )
    .await;

    client.close().await;
}

#[rstest]
#[tokio::test]
async fn test_md_subscribe_same_symbol_different_levels() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{addr}/md/ws");

    let mut client = AxMdWebSocketClient::new(ws_url, "test_token".to_string(), None);

    client.connect().await.unwrap();
    wait_for_connection(&state).await;

    client
        .subscribe("BTCUSD-PERP", AxMarketDataLevel::Level1)
        .await
        .unwrap();
    client
        .subscribe("BTCUSD-PERP", AxMarketDataLevel::Level2)
        .await
        .unwrap();

    wait_until_async(
        || async { state.subscriptions.lock().await.len() >= 2 },
        Duration::from_secs(5),
    )
    .await;

    let subs = state.subscriptions.lock().await.clone();
    assert!(subs.iter().any(|s| s.contains("LEVEL_1")));
    assert!(subs.iter().any(|s| s.contains("LEVEL_2")));

    client.close().await;
}

#[rstest]
#[tokio::test]
async fn test_orders_client_connection() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{addr}/orders/ws");
    let account_id = AccountId::from("AX-001");

    let mut client = AxOrdersWebSocketClient::new(ws_url, account_id, Some(30));

    client.connect("test_bearer_token").await.unwrap();
    wait_for_connection(&state).await;

    assert!(client.is_active());
    assert_eq!(*state.connection_count.lock().await, 1);

    client.close().await;
}

#[rstest]
#[tokio::test]
async fn test_orders_client_url_accessor() {
    let ws_url = "ws://localhost:9999/orders/ws".to_string();
    let account_id = AccountId::from("AX-001");
    let client = AxOrdersWebSocketClient::new(ws_url.clone(), account_id, None);

    assert_eq!(client.url(), ws_url);
}

#[rstest]
#[tokio::test]
async fn test_orders_client_account_id_accessor() {
    let ws_url = "ws://localhost:9999/orders/ws".to_string();
    let account_id = AccountId::from("AX-001");
    let client = AxOrdersWebSocketClient::new(ws_url, account_id, None);

    assert_eq!(client.account_id(), account_id);
}

#[rstest]
#[tokio::test]
async fn test_orders_client_not_active_before_connect() {
    let account_id = AccountId::from("AX-001");
    let client = AxOrdersWebSocketClient::new(
        "ws://localhost:9999/orders/ws".to_string(),
        account_id,
        None,
    );

    assert!(!client.is_active());
    assert!(client.is_closed());
}

#[rstest]
#[tokio::test]
async fn test_orders_connection_failure_to_invalid_url() {
    let account_id = AccountId::from("AX-001");
    let mut client =
        AxOrdersWebSocketClient::new("ws://127.0.0.1:9999/invalid".to_string(), account_id, None);

    let result = client.connect("test_token").await;
    assert!(result.is_err());
}

#[rstest]
#[tokio::test]
async fn test_orders_close_sets_closed_flag() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{addr}/orders/ws");
    let account_id = AccountId::from("AX-001");

    let mut client = AxOrdersWebSocketClient::new(ws_url, account_id, None);

    client.connect("test_token").await.unwrap();
    wait_for_connection(&state).await;

    assert!(!client.is_closed());

    client.close().await;

    assert!(client.is_closed());
}

#[rstest]
#[tokio::test]
async fn test_orders_place_order() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{addr}/orders/ws");
    let account_id = AccountId::from("AX-001");

    let mut client = AxOrdersWebSocketClient::new(ws_url, account_id, None);

    client.connect("test_token").await.unwrap();
    wait_for_connection(&state).await;

    let request_id = client
        .place_order(
            ClientOrderId::from("TEST-001"),
            Ustr::from("BTCUSD-PERP"),
            AxOrderSide::Buy,
            100000, // qty in minor units
            dec!(50000.00),
            AxTimeInForce::Gtc,
            false,
            None,
        )
        .await
        .unwrap();

    assert!(request_id > 0);

    wait_until_async(
        || async { !state.get_messages().await.is_empty() },
        Duration::from_secs(5),
    )
    .await;

    let messages = state.get_messages().await;
    assert!(!messages.is_empty());

    client.close().await;
}

#[rstest]
#[tokio::test]
async fn test_orders_cancel_order() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{addr}/orders/ws");
    let account_id = AccountId::from("AX-001");

    let mut client = AxOrdersWebSocketClient::new(ws_url, account_id, None);

    client.connect("test_token").await.unwrap();
    wait_for_connection(&state).await;

    let request_id = client.cancel_order("order-123").await.unwrap();

    assert!(request_id > 0);

    wait_until_async(
        || async { !state.get_messages().await.is_empty() },
        Duration::from_secs(5),
    )
    .await;

    client.close().await;
}

#[rstest]
#[tokio::test]
async fn test_orders_get_open_orders() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{addr}/orders/ws");
    let account_id = AccountId::from("AX-001");

    let mut client = AxOrdersWebSocketClient::new(ws_url, account_id, None);

    client.connect("test_token").await.unwrap();
    wait_for_connection(&state).await;

    let request_id = client.get_open_orders().await.unwrap();

    assert!(request_id > 0);

    wait_until_async(
        || async { !state.get_messages().await.is_empty() },
        Duration::from_secs(5),
    )
    .await;

    client.close().await;
}

#[rstest]
#[tokio::test]
async fn test_orders_cache_instrument() {
    let account_id = AccountId::from("AX-001");
    let client = AxOrdersWebSocketClient::new(
        "ws://localhost:9999/orders/ws".to_string(),
        account_id,
        None,
    );

    let instrument = create_test_instrument("BTCUSD-PERP");
    client.cache_instrument(instrument);

    let cached = client.get_cached_instrument(&Ustr::from("BTCUSD-PERP"));
    assert!(cached.is_some());
}

#[rstest]
#[tokio::test]
async fn test_orders_get_cached_instrument_returns_none_for_unknown() {
    let account_id = AccountId::from("AX-001");
    let client = AxOrdersWebSocketClient::new(
        "ws://localhost:9999/orders/ws".to_string(),
        account_id,
        None,
    );

    let cached = client.get_cached_instrument(&Ustr::from("UNKNOWN-SYMBOL"));
    assert!(cached.is_none());
}

#[rstest]
#[tokio::test]
async fn test_md_subscription_events_tracking() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{addr}/md/ws");

    let mut client = AxMdWebSocketClient::new(ws_url, "test_token".to_string(), None);

    client.connect().await.unwrap();
    wait_for_connection(&state).await;

    client
        .subscribe("BTCUSD-PERP", AxMarketDataLevel::Level1)
        .await
        .unwrap();

    wait_until_async(
        || async { !state.subscription_events().await.is_empty() },
        Duration::from_secs(5),
    )
    .await;

    let events = state.subscription_events().await;
    assert!(
        events
            .iter()
            .any(|(topic, success)| topic.contains("BTCUSD-PERP") && *success)
    );

    client.close().await;
}

#[rstest]
#[tokio::test]
async fn test_md_subscription_failure_tracking() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{addr}/md/ws");

    state
        .set_subscription_failures(vec!["FAIL-SYMBOL:LEVEL_1".to_string()])
        .await;

    let mut client = AxMdWebSocketClient::new(ws_url, "test_token".to_string(), None);

    client.connect().await.unwrap();
    wait_for_connection(&state).await;

    client
        .subscribe("FAIL-SYMBOL", AxMarketDataLevel::Level1)
        .await
        .unwrap();

    wait_until_async(
        || async { !state.subscription_events().await.is_empty() },
        Duration::from_secs(5),
    )
    .await;

    let events = state.subscription_events().await;
    assert!(
        events
            .iter()
            .any(|(topic, success)| topic.contains("FAIL-SYMBOL") && !*success)
    );

    client.close().await;
}

#[rstest]
#[tokio::test]
async fn test_multiple_md_clients() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{addr}/md/ws");

    let mut client1 = AxMdWebSocketClient::new(ws_url.clone(), "token1".to_string(), None);
    let mut client2 = AxMdWebSocketClient::new(ws_url, "token2".to_string(), None);

    client1.connect().await.unwrap();

    wait_until_async(
        || async { *state.connection_count.lock().await >= 1 },
        Duration::from_secs(5),
    )
    .await;

    client2.connect().await.unwrap();

    wait_until_async(
        || async { *state.connection_count.lock().await >= 2 },
        Duration::from_secs(5),
    )
    .await;

    assert_eq!(*state.connection_count.lock().await, 2);

    client1.close().await;
    client2.close().await;
}

#[rstest]
#[tokio::test]
async fn test_md_client_debug() {
    let client = AxMdWebSocketClient::new(
        "ws://localhost:9999/md/ws".to_string(),
        "test_token".to_string(),
        Some(30),
    );

    let debug_str = format!("{client:?}");
    assert!(debug_str.contains("AxMdWebSocketClient"));
    assert!(debug_str.contains("ws://localhost:9999/md/ws"));
}

#[rstest]
#[tokio::test]
async fn test_orders_client_debug() {
    let account_id = AccountId::from("AX-001");
    let client = AxOrdersWebSocketClient::new(
        "ws://localhost:9999/orders/ws".to_string(),
        account_id,
        Some(30),
    );

    let debug_str = format!("{client:?}");
    assert!(debug_str.contains("AxOrdersWebSocketClient"));
    assert!(debug_str.contains("ws://localhost:9999/orders/ws"));
}

#[rstest]
#[tokio::test]
async fn test_md_rapid_connect_disconnect() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{addr}/md/ws");

    for _ in 0..3 {
        let mut client = AxMdWebSocketClient::new(ws_url.clone(), "test_token".to_string(), None);

        client.connect().await.unwrap();
        wait_for_connection(&state).await;

        assert!(client.is_active());

        client.close().await;

        wait_until_async(
            || async { *state.connection_count.lock().await == 0 },
            Duration::from_secs(5),
        )
        .await;
    }
}

#[rstest]
#[tokio::test]
async fn test_md_many_subscriptions() {
    let (addr, state) = start_test_server().await.unwrap();
    let ws_url = format!("ws://{addr}/md/ws");

    let mut client = AxMdWebSocketClient::new(ws_url, "test_token".to_string(), None);

    client.connect().await.unwrap();
    wait_for_connection(&state).await;

    let symbols = [
        "BTCUSD-PERP",
        "ETHUSD-PERP",
        "EURUSD-PERP",
        "GBPUSD-PERP",
        "USDJPY-PERP",
    ];

    for symbol in symbols {
        client
            .subscribe(symbol, AxMarketDataLevel::Level1)
            .await
            .unwrap();
    }

    wait_until_async(
        || async { state.subscriptions.lock().await.len() >= symbols.len() },
        Duration::from_secs(10),
    )
    .await;

    let subs = state.subscriptions.lock().await.clone();
    assert!(subs.len() >= symbols.len());

    client.close().await;
}
