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

//! Integration tests for the OKX WebSocket client using a mock Axum server.

use std::{
    collections::HashSet,
    net::SocketAddr,
    path::PathBuf,
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
use futures_util::{StreamExt, pin_mut};
use nautilus_common::testing::wait_until_async;
use nautilus_core::UnixNanos;
use nautilus_model::identifiers::{AccountId, InstrumentId};
use nautilus_okx::{
    common::{enums::OKXInstrumentType, parse::parse_instrument_any},
    websocket::client::OKXWebSocketClient,
};
use serde_json::{Value, json};
use tokio::sync::Mutex;

const TEXT_PING: &str = "ping";
const TEXT_PONG: &str = "pong";
const CONTROL_PING_PAYLOAD: &[u8] = b"server-control-ping";

type SubscriptionEvent = (String, Option<String>, bool);

#[derive(Clone, Default)]
struct TestServerState {
    connection_count: Arc<Mutex<usize>>,
    login_count: Arc<Mutex<usize>>,
    subscriptions: Arc<Mutex<Vec<Value>>>,
    unsubscriptions: Arc<Mutex<Vec<Value>>>,
    drop_next_connection: Arc<AtomicBool>,
    send_text_ping: Arc<AtomicBool>,
    send_control_ping: Arc<AtomicBool>,
    received_text_pong: Arc<AtomicBool>,
    received_control_pong: Arc<Mutex<Option<Vec<u8>>>>,
    authenticated: Arc<AtomicBool>,
    subscription_events: Arc<Mutex<Vec<SubscriptionEvent>>>,
    fail_next_subscriptions: Arc<Mutex<Vec<String>>>,
    auth_response_delay_ms: Arc<Mutex<Option<u64>>>,
    suppress_login_ack: Arc<AtomicBool>,
    suppress_control_pong: Arc<AtomicBool>,
    control_ping_count: Arc<Mutex<usize>>,
    fail_next_login: Arc<AtomicBool>,
}

fn data_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test_data")
}

fn load_json(filename: &str) -> Value {
    let content = std::fs::read_to_string(data_path().join(filename))
        .unwrap_or_else(|_| panic!("failed to read {filename}"));
    serde_json::from_str(&content).expect("invalid json")
}

fn load_instruments() -> Vec<nautilus_model::instruments::InstrumentAny> {
    let payload = load_json("http_get_instruments_spot.json");
    let response: nautilus_okx::http::client::OKXResponse<
        nautilus_okx::common::models::OKXInstrument,
    > = serde_json::from_value(payload).expect("invalid instrument payload");
    let ts_init = UnixNanos::default();
    response
        .data
        .iter()
        .filter_map(|raw| {
            parse_instrument_any(raw, None, None, None, None, ts_init)
                .ok()
                .flatten()
        })
        .collect()
}

fn value_matches_channel(value: &Value, channel: &str) -> bool {
    value
        .get("channel")
        .and_then(|c| c.as_str())
        .is_some_and(|name| name.eq(channel))
        || value.as_str().is_some_and(|name| name.eq(channel))
}

fn is_private_channel(channel: &str) -> bool {
    matches!(channel, "account" | "orders" | "fills" | "orders-algo")
}

impl TestServerState {
    fn subscription_key(arg: &Value) -> (String, Option<String>) {
        let channel = arg
            .get("channel")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();

        if let Some(inst_id) = arg.get("instId").and_then(|v| v.as_str()) {
            return (format!("{channel}:{inst_id}"), Some(inst_id.to_string()));
        }

        if let Some(inst_type) = arg.get("instType").and_then(|v| v.as_str()) {
            return (
                format!("{channel}:{inst_type}"),
                Some(inst_type.to_string()),
            );
        }

        if let Some(inst_family) = arg.get("instFamily").and_then(|v| v.as_str()) {
            return (
                format!("{channel}:{inst_family}"),
                Some(inst_family.to_string()),
            );
        }

        (channel, None)
    }

    async fn record_subscription_event(&self, arg: &Value, success: bool) {
        let (key, detail) = Self::subscription_key(arg);
        self.subscription_events
            .lock()
            .await
            .push((key, detail, success));
    }

    async fn pop_fail_subscription(&self, key: &str) -> bool {
        let mut pending = self.fail_next_subscriptions.lock().await;
        if let Some(pos) = pending.iter().position(|entry| entry == key) {
            pending.remove(pos);
            true
        } else {
            false
        }
    }

    async fn subscription_events(&self) -> Vec<(String, Option<String>, bool)> {
        self.subscription_events.lock().await.clone()
    }

    async fn clear_subscription_events(&self) {
        self.subscription_events.lock().await.clear();
    }

    async fn control_ping_count(&self) -> usize {
        *self.control_ping_count.lock().await
    }
}

async fn handle_ws_upgrade(
    ws: WebSocketUpgrade,
    State(state): State<Arc<TestServerState>>,
) -> Response {
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

async fn handle_socket(mut socket: WebSocket, state: Arc<TestServerState>) {
    state.authenticated.store(false, Ordering::Relaxed);
    {
        let mut count = state.connection_count.lock().await;
        *count += 1;
    }

    let trades_payload = load_json("ws_trades.json");

    if state.send_text_ping.load(Ordering::Relaxed)
        && socket
            .send(Message::Text(TEXT_PING.to_string().into()))
            .await
            .is_err()
    {
        return;
    }

    if state.send_control_ping.load(Ordering::Relaxed)
        && socket
            .send(Message::Ping(CONTROL_PING_PAYLOAD.to_vec().into()))
            .await
            .is_err()
    {
        return;
    }

    while let Some(message) = socket.next().await {
        let Ok(message) = message else { break };

        match message {
            Message::Text(text) => {
                if text == TEXT_PONG {
                    state.received_text_pong.store(true, Ordering::Relaxed);
                    continue;
                }

                if text == TEXT_PING {
                    {
                        let mut count = state.control_ping_count.lock().await;
                        *count += 1;
                    }

                    if state.suppress_control_pong.load(Ordering::Relaxed) {
                        let _ = socket.send(Message::Close(None)).await;
                        break;
                    }

                    if socket
                        .send(Message::Text(TEXT_PONG.to_string().into()))
                        .await
                        .is_err()
                    {
                        break;
                    }
                    continue;
                }

                if let Ok(payload) = serde_json::from_str::<Value>(&text) {
                    if payload.get("op") == Some(&json!("login")) {
                        if let Some(delay_ms) = *state.auth_response_delay_ms.lock().await {
                            tokio::time::sleep(Duration::from_millis(delay_ms)).await;
                        }

                        if state.suppress_login_ack.swap(false, Ordering::Relaxed) {
                            continue;
                        }

                        {
                            let mut login_count = state.login_count.lock().await;
                            *login_count += 1;
                        }

                        if state.fail_next_login.swap(false, Ordering::Relaxed) {
                            let response = json!({
                                "event": "login",
                                "code": "60015",
                                "msg": "Invalid signature",
                                "connId": "test-conn",
                            });
                            let _ = socket
                                .send(Message::Text(response.to_string().into()))
                                .await;
                            continue;
                        }

                        let response = json!({
                            "event": "login",
                            "code": "0",
                            "msg": "",
                            "connId": "test-conn",
                        });
                        if socket
                            .send(Message::Text(response.to_string().into()))
                            .await
                            .is_err()
                        {
                            break;
                        }

                        state.authenticated.store(true, Ordering::Relaxed);
                        continue;
                    }

                    if payload.get("op") == Some(&json!("subscribe")) {
                        if let Some(args) = payload.get("args").and_then(|value| value.as_array())
                            && let Some(first) = args.first()
                        {
                            let (key, _) = TestServerState::subscription_key(first);
                            let channel = first
                                .get("channel")
                                .and_then(|c| c.as_str())
                                .unwrap_or_default();

                            let mut success = true;
                            if is_private_channel(channel)
                                && !state.authenticated.load(Ordering::Relaxed)
                            {
                                success = false;
                            }

                            if success && state.pop_fail_subscription(&key).await {
                                success = false;
                                state.drop_next_connection.store(true, Ordering::Relaxed);
                            }

                            if success {
                                let mut subscriptions = state.subscriptions.lock().await;
                                subscriptions.push(first.clone());
                            }

                            let mut ack = json!({
                                "event": "subscribe",
                                "arg": first,
                                "connId": "test-conn",
                                "code": if success { "0" } else { "60019" },
                            });

                            if !success {
                                ack["msg"] = json!("Subscription failed");
                            }

                            if socket
                                .send(Message::Text(ack.to_string().into()))
                                .await
                                .is_err()
                            {
                                break;
                            }

                            state.record_subscription_event(first, success).await;

                            if success
                                && socket
                                    .send(Message::Text(trades_payload.to_string().into()))
                                    .await
                                    .is_err()
                            {
                                break;
                            }

                            if state.drop_next_connection.swap(false, Ordering::Relaxed) {
                                let _ = socket.send(Message::Close(None)).await;
                                break;
                            }
                        }
                        continue;
                    }

                    if payload.get("op") == Some(&json!("unsubscribe")) {
                        if let Some(args) = payload.get("args").and_then(|value| value.as_array())
                            && let Some(first) = args.first()
                        {
                            {
                                let mut unsubscriptions = state.unsubscriptions.lock().await;
                                unsubscriptions.push(first.clone());
                            }
                            let ack = json!({
                                "event": "unsubscribe",
                                "arg": first,
                                "connId": "test-conn",
                            });
                            if socket
                                .send(Message::Text(ack.to_string().into()))
                                .await
                                .is_err()
                            {
                                break;
                            }
                            if state.drop_next_connection.swap(false, Ordering::Relaxed) {
                                let _ = socket.send(Message::Close(None)).await;
                                break;
                            }
                        }
                        continue;
                    }
                }
            }
            Message::Ping(payload) => {
                {
                    let mut count = state.control_ping_count.lock().await;
                    *count += 1;
                }

                if state.suppress_control_pong.load(Ordering::Relaxed) {
                    continue;
                }

                if socket.send(Message::Pong(payload.clone())).await.is_err() {
                    break;
                }
            }
            Message::Pong(payload) => {
                *state.received_control_pong.lock().await = Some(payload.to_vec());
            }
            Message::Close(_) => break,
            _ => {}
        }
    }

    if state.drop_next_connection.swap(false, Ordering::Relaxed) {
        let _ = socket.send(Message::Close(None)).await;
    }

    state.authenticated.store(false, Ordering::Relaxed);

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

async fn connect_client(ws_url: &str) -> OKXWebSocketClient {
    OKXWebSocketClient::new(
        Some(ws_url.to_string()),
        Some("api_key".to_string()),
        Some("api_secret".to_string()),
        Some("passphrase".to_string()),
        Some(AccountId::from("OKX-TEST")),
        Some(30),
    )
    .expect("failed to construct okx websocket client")
}

#[tokio::test]
async fn test_websocket_connection() {
    let state = Arc::new(TestServerState::default());
    let addr = start_ws_server(state.clone()).await;
    let ws_url = format!("ws://{addr}/ws");

    let instruments = load_instruments();

    let mut client = connect_client(&ws_url).await;
    client.initialize_instruments_cache(instruments);
    client.connect().await.expect("connect failed");

    wait_until_async(
        || {
            let state = state.clone();
            async move { *state.connection_count.lock().await == 1 }
        },
        Duration::from_secs(1),
    )
    .await;

    client.close().await.expect("close failed");

    wait_until_async(
        || {
            let state = state.clone();
            async move { *state.connection_count.lock().await == 0 }
        },
        Duration::from_secs(1),
    )
    .await;
}

#[tokio::test]
async fn test_wait_until_active_timeout() {
    let client = OKXWebSocketClient::new(
        Some("ws://127.0.0.1:0/ws".to_string()),
        Some("api_key".to_string()),
        Some("api_secret".to_string()),
        Some("passphrase".to_string()),
        Some(AccountId::from("OKX-TEST")),
        Some(30),
    )
    .expect("construct client");

    let result = client.wait_until_active(0.1).await;
    assert!(result.is_err(), "expected timeout error");
}

#[tokio::test]
async fn test_trades_subscription_flow() {
    let state = Arc::new(TestServerState::default());
    let addr = start_ws_server(state.clone()).await;
    let ws_url = format!("ws://{addr}/ws");

    let instruments = load_instruments();

    let mut client = connect_client(&ws_url).await;
    client.initialize_instruments_cache(instruments);
    client.connect().await.expect("connect failed");
    client
        .wait_until_active(1.0)
        .await
        .expect("client inactive");

    client
        .subscribe_trades(InstrumentId::from("BTC-USD.OKX"), false)
        .await
        .expect("subscribe failed");

    let stream = client.stream();
    pin_mut!(stream);
    let message = tokio::time::timeout(Duration::from_secs(1), stream.next())
        .await
        .expect("no message received")
        .expect("stream ended unexpectedly");

    match message {
        nautilus_okx::websocket::messages::NautilusWsMessage::Data(data) => {
            assert!(!data.is_empty(), "expected trade payload");
        }
        other => panic!("unexpected message: {other:?}"),
    }

    let login_count = *state.login_count.lock().await;
    assert_eq!(login_count, 1);
}

#[tokio::test]
async fn test_reauth_and_resubscribe_after_disconnect() {
    let state = Arc::new(TestServerState::default());
    state.drop_next_connection.store(true, Ordering::Relaxed);

    let addr = start_ws_server(state.clone()).await;
    let ws_url = format!("ws://{addr}/ws");

    let instruments = load_instruments();

    let mut client = connect_client(&ws_url).await;
    client.initialize_instruments_cache(instruments);
    client.connect().await.expect("connect failed");
    client
        .wait_until_active(1.0)
        .await
        .expect("client inactive");

    client
        .subscribe_trades(InstrumentId::from("BTC-USD.OKX"), false)
        .await
        .expect("subscribe failed");

    wait_until_async(
        || {
            let state = state.clone();
            async move {
                let login_count = *state.login_count.lock().await;
                let subscription_count = state.subscriptions.lock().await.len();
                login_count >= 2 && subscription_count >= 2
            }
        },
        Duration::from_secs(2),
    )
    .await;
}

#[tokio::test]
async fn test_heartbeat_timeout_reconnection() {
    let state = Arc::new(TestServerState::default());
    state.suppress_control_pong.store(true, Ordering::Relaxed);

    let addr = start_ws_server(state.clone()).await;
    let ws_url = format!("ws://{addr}/ws");

    let instruments = load_instruments();

    let mut client = OKXWebSocketClient::new(
        Some(ws_url),
        Some("api_key".to_string()),
        Some("api_secret".to_string()),
        Some("passphrase".to_string()),
        Some(AccountId::from("OKX-TEST")),
        Some(1),
    )
    .expect("construct client");

    client.initialize_instruments_cache(instruments);
    client.connect().await.expect("connect failed");
    client
        .wait_until_active(1.0)
        .await
        .expect("client inactive");

    client
        .subscribe_trades(InstrumentId::from("BTC-USD.OKX"), false)
        .await
        .expect("subscribe trades failed");

    wait_until_async(
        || {
            let state = state.clone();
            async move {
                state
                    .subscription_events()
                    .await
                    .iter()
                    .any(|(key, _, ok)| key.starts_with("trades") && *ok)
            }
        },
        Duration::from_secs(1),
    )
    .await;

    state.clear_subscription_events().await;

    wait_until_async(
        || {
            let state = state.clone();
            async move { state.control_ping_count().await >= 1 }
        },
        Duration::from_secs(2),
    )
    .await;

    wait_until_async(
        || {
            let state = state.clone();
            async move { *state.login_count.lock().await >= 2 }
        },
        Duration::from_secs(5),
    )
    .await;

    state.suppress_control_pong.store(false, Ordering::Relaxed);

    wait_until_async(
        || {
            let state = state.clone();
            async move {
                state
                    .subscription_events()
                    .await
                    .iter()
                    .any(|(key, _, ok)| key.starts_with("trades") && *ok)
            }
        },
        Duration::from_secs(3),
    )
    .await;

    client.close().await.expect("close failed");
}

#[tokio::test]
async fn test_reconnection_retries_failed_subscriptions() {
    let state = Arc::new(TestServerState::default());
    {
        let mut pending = state.fail_next_subscriptions.lock().await;
        pending.push("trades:BTC-USD".to_string());
    }

    let addr = start_ws_server(state.clone()).await;
    let ws_url = format!("ws://{addr}/ws");

    let instruments = load_instruments();

    let mut client = connect_client(&ws_url).await;
    client.initialize_instruments_cache(instruments);
    client.connect().await.expect("connect failed");
    client
        .wait_until_active(1.0)
        .await
        .expect("client inactive");

    state.clear_subscription_events().await;

    client
        .subscribe_trades(InstrumentId::from("BTC-USD.OKX"), false)
        .await
        .expect("subscribe trades failed");

    if tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if state
                .subscription_events()
                .await
                .iter()
                .any(|(key, _, ok)| key.starts_with("trades") && !ok)
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .is_err()
    {
        let events = state.subscription_events().await;
        panic!("missing initial subscription failure: events={events:?}");
    }

    if tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if *state.login_count.lock().await >= 2 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .is_err()
    {
        let count = *state.login_count.lock().await;
        let events = state.subscription_events().await;
        panic!("login did not retry: count={count}, events={events:?}");
    }

    client
        .subscribe_trades(InstrumentId::from("BTC-USD.OKX"), false)
        .await
        .expect("retry subscribe trades failed");

    if tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let events = state.subscription_events().await;
            let mut trade_count = 0;
            let mut has_success = false;
            for (_, _, ok) in events
                .iter()
                .filter(|(key, _, _)| key.starts_with("trades"))
            {
                trade_count += 1;
                if *ok {
                    has_success = true;
                }
            }
            if trade_count >= 2 && has_success {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .is_err()
    {
        let events = state.subscription_events().await;
        panic!("subscription did not recover: events={events:?}");
    }

    let events = state.subscription_events().await;
    assert!(
        events
            .iter()
            .any(|(key, _, ok)| key.starts_with("trades") && !ok)
    );
    assert!(
        events
            .iter()
            .any(|(key, _, ok)| key.starts_with("trades") && *ok)
    );
}

#[tokio::test]
async fn test_reconnection_waits_for_delayed_auth_ack() {
    let state = Arc::new(TestServerState::default());

    let addr = start_ws_server(state.clone()).await;
    let ws_url = format!("ws://{addr}/ws");

    let instruments = load_instruments();

    let mut client = connect_client(&ws_url).await;
    client.initialize_instruments_cache(instruments);
    client.connect().await.expect("connect failed");
    client
        .wait_until_active(1.0)
        .await
        .expect("client inactive");

    client
        .subscribe_trades(InstrumentId::from("BTC-USD.OKX"), false)
        .await
        .expect("subscribe trades failed");
    client
        .subscribe_orders(OKXInstrumentType::Spot)
        .await
        .expect("subscribe orders failed");

    wait_until_async(
        || {
            let state = state.clone();
            async move {
                let events = state.subscription_events().await;
                let trades_ok = events
                    .iter()
                    .any(|(key, _, ok)| key.starts_with("trades") && *ok);
                let orders_ok = events
                    .iter()
                    .any(|(key, _, ok)| key.starts_with("orders") && *ok);
                trades_ok && orders_ok
            }
        },
        Duration::from_secs(2),
    )
    .await;

    state.clear_subscription_events().await;

    {
        let mut delay = state.auth_response_delay_ms.lock().await;
        *delay = Some(250);
    }

    state.drop_next_connection.store(true, Ordering::Relaxed);

    client
        .subscribe_trades(InstrumentId::from("ETH-USD.OKX"), false)
        .await
        .expect("trigger drop");

    wait_until_async(
        || {
            let state = state.clone();
            async move { *state.login_count.lock().await >= 2 }
        },
        Duration::from_secs(5),
    )
    .await;

    wait_until_async(
        || {
            let state = state.clone();
            async move {
                state
                    .subscription_events()
                    .await
                    .iter()
                    .any(|(key, _, ok)| key.starts_with("orders") && *ok)
            }
        },
        Duration::from_secs(2),
    )
    .await;

    let events = state.subscription_events().await;
    assert!(
        !events
            .iter()
            .any(|(key, _, ok)| key.starts_with("orders") && !ok)
    );

    client.close().await.expect("close failed");
}

#[tokio::test]
async fn test_login_failure_emits_error() {
    let state = Arc::new(TestServerState::default());
    state.fail_next_login.store(true, Ordering::Relaxed);

    let addr = start_ws_server(state.clone()).await;
    let ws_url = format!("ws://{addr}/ws");

    let instruments = load_instruments();

    let mut client = connect_client(&ws_url).await;
    client.initialize_instruments_cache(instruments);

    let connect_result = tokio::time::timeout(Duration::from_secs(1), client.connect()).await;

    match connect_result {
        Ok(Ok(())) => panic!("connect unexpectedly succeeded"),
        Ok(Err(e)) => assert!(format!("{e}").contains("Authentication")),
        Err(_) => {
            tokio::time::timeout(Duration::from_secs(1), async {
                loop {
                    if *state.login_count.lock().await >= 1 {
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(25)).await;
                }
            })
            .await
            .expect("login error did not arrive in time");
        }
    }

    assert_eq!(*state.login_count.lock().await, 1);
    assert!(!state.authenticated.load(Ordering::Relaxed));

    client.close().await.expect("close failed");
}

#[tokio::test]
async fn test_subscription_restoration_tracking() {
    let state = Arc::new(TestServerState::default());
    let addr = start_ws_server(state.clone()).await;
    let ws_url = format!("ws://{addr}/ws");

    let instruments = load_instruments();

    let mut client = connect_client(&ws_url).await;
    client.initialize_instruments_cache(instruments);
    client.connect().await.expect("connect failed");
    client
        .wait_until_active(1.0)
        .await
        .expect("client inactive");

    let btc = InstrumentId::from("BTC-USD.OKX");
    let eth = InstrumentId::from("ETH-USD.OKX");

    client
        .subscribe_trades(btc, false)
        .await
        .expect("subscribe trades BTC failed");
    client
        .subscribe_trades(eth, false)
        .await
        .expect("subscribe trades ETH failed");
    client
        .subscribe_orders(OKXInstrumentType::Spot)
        .await
        .expect("subscribe orders failed");

    wait_until_async(
        || {
            let state = state.clone();
            async move {
                let events = state.subscription_events().await;
                let trades = events
                    .iter()
                    .filter(|(key, _, ok)| key.starts_with("trades") && *ok)
                    .count();
                let orders_ok = events
                    .iter()
                    .any(|(key, _, ok)| key.starts_with("orders") && *ok);
                trades >= 2 && orders_ok
            }
        },
        Duration::from_secs(2),
    )
    .await;

    state.clear_subscription_events().await;
    state.drop_next_connection.store(true, Ordering::Relaxed);

    client
        .subscribe_book(InstrumentId::from("BTC-USD.OKX"))
        .await
        .expect("subscribe book failed");

    wait_until_async(
        || {
            let state = state.clone();
            async move { *state.login_count.lock().await >= 2 }
        },
        Duration::from_secs(5),
    )
    .await;

    state.clear_subscription_events().await;

    client
        .subscribe_trades(btc, false)
        .await
        .expect("resubscribe trades BTC failed");
    client
        .subscribe_trades(eth, false)
        .await
        .expect("resubscribe trades ETH failed");
    client
        .subscribe_orders(OKXInstrumentType::Spot)
        .await
        .expect("resubscribe orders failed");

    if tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let events = state.subscription_events().await;
            let mut restored = HashSet::new();
            for (key, _, ok) in events.iter() {
                if *ok {
                    restored.insert(key.clone());
                }
            }
            if restored.contains("trades:BTC-USD")
                && restored.contains("trades:ETH-USD")
                && restored.contains("orders:SPOT")
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .is_err()
    {
        let events = state.subscription_events().await;
        panic!("subscriptions not restored: events={events:?}");
    }

    client.close().await.expect("close failed");
}

#[tokio::test]
async fn test_true_auto_reconnect_with_verification() {
    let state = Arc::new(TestServerState::default());
    state.drop_next_connection.store(true, Ordering::Relaxed);

    let addr = start_ws_server(state.clone()).await;
    let ws_url = format!("ws://{addr}/ws");

    let instruments = load_instruments();

    let mut client = connect_client(&ws_url).await;
    client.initialize_instruments_cache(instruments);
    client.connect().await.expect("connect failed");
    client
        .wait_until_active(1.0)
        .await
        .expect("client inactive");

    client
        .subscribe_trades(InstrumentId::from("BTC-USD.OKX"), false)
        .await
        .expect("subscribe trades failed");

    let stream = client.stream();
    pin_mut!(stream);

    let first = tokio::time::timeout(Duration::from_secs(2), stream.next())
        .await
        .expect("first message timeout")
        .expect("stream closed too early");

    match first {
        nautilus_okx::websocket::messages::NautilusWsMessage::Data(payload) => {
            assert!(!payload.is_empty());
        }
        other => panic!("unexpected message before reconnect: {other:?}"),
    }

    wait_until_async(
        || {
            let state = state.clone();
            async move { *state.login_count.lock().await >= 2 }
        },
        Duration::from_secs(5),
    )
    .await;

    let second = tokio::time::timeout(Duration::from_secs(3), stream.next())
        .await
        .expect("second message timeout")
        .expect("stream closed after reconnect");

    match second {
        nautilus_okx::websocket::messages::NautilusWsMessage::Data(payload) => {
            assert!(!payload.is_empty());
        }
        other => panic!("unexpected message after reconnect: {other:?}"),
    }

    client.close().await.expect("close failed");
}

#[tokio::test]
async fn test_sends_pong_for_text_ping() {
    let state = Arc::new(TestServerState::default());
    state.send_text_ping.store(true, Ordering::Relaxed);

    let addr = start_ws_server(state.clone()).await;
    let ws_url = format!("ws://{addr}/ws");

    let instruments = load_instruments();

    let mut client = connect_client(&ws_url).await;
    client.initialize_instruments_cache(instruments);
    client.connect().await.expect("connect failed");
    client
        .wait_until_active(1.0)
        .await
        .expect("client inactive");

    client
        .subscribe_trades(InstrumentId::from("BTC-USD.OKX"), false)
        .await
        .expect("subscribe failed");

    let deadline = tokio::time::Instant::now() + Duration::from_secs(1);
    while !state.received_text_pong.load(Ordering::Relaxed) {
        if tokio::time::Instant::now() > deadline {
            panic!("client did not respond to text ping");
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

#[tokio::test]
async fn test_sends_pong_for_control_ping() {
    let state = Arc::new(TestServerState::default());
    state.send_control_ping.store(true, Ordering::Relaxed);

    let addr = start_ws_server(state.clone()).await;
    let ws_url = format!("ws://{addr}/ws");

    let instruments = load_instruments();

    let mut client = connect_client(&ws_url).await;
    client.initialize_instruments_cache(instruments);
    client.connect().await.expect("connect failed");
    client
        .wait_until_active(1.0)
        .await
        .expect("client inactive");

    client
        .subscribe_trades(InstrumentId::from("BTC-USD.OKX"), false)
        .await
        .expect("subscribe failed");

    let deadline = tokio::time::Instant::now() + Duration::from_secs(1);
    loop {
        {
            let guard = state.received_control_pong.lock().await;
            if guard
                .as_ref()
                .is_some_and(|payload| payload.as_slice() == CONTROL_PING_PAYLOAD)
            {
                break;
            }
        }
        if tokio::time::Instant::now() > deadline {
            panic!("client did not respond to control ping");
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

#[tokio::test]
async fn test_unsubscribe_orders_sends_request() {
    let state = Arc::new(TestServerState::default());
    let addr = start_ws_server(state.clone()).await;
    let ws_url = format!("ws://{addr}/ws");

    let instruments = load_instruments();

    let mut client = connect_client(&ws_url).await;
    client.initialize_instruments_cache(instruments);
    client.connect().await.expect("connect failed");
    client
        .wait_until_active(1.0)
        .await
        .expect("client inactive");

    client
        .subscribe_orders(OKXInstrumentType::Spot)
        .await
        .expect("subscribe orders failed");

    wait_until_async(
        || {
            let state = state.clone();
            async move {
                state
                    .subscriptions
                    .lock()
                    .await
                    .iter()
                    .any(|value| value_matches_channel(value, "orders"))
            }
        },
        Duration::from_secs(1),
    )
    .await;

    client
        .unsubscribe_orders(OKXInstrumentType::Spot)
        .await
        .expect("unsubscribe orders failed");

    wait_until_async(
        || {
            let state = state.clone();
            async move {
                state
                    .unsubscriptions
                    .lock()
                    .await
                    .iter()
                    .any(|value| value_matches_channel(value, "orders"))
            }
        },
        Duration::from_secs(1),
    )
    .await;
}

#[tokio::test]
async fn test_subscribe_to_orderbook() {
    let state = Arc::new(TestServerState::default());
    let addr = start_ws_server(state.clone()).await;
    let ws_url = format!("ws://{addr}/ws");

    let instruments = load_instruments();

    let mut client = connect_client(&ws_url).await;
    client.initialize_instruments_cache(instruments);
    client.connect().await.expect("connect failed");
    client
        .wait_until_active(1.0)
        .await
        .expect("client inactive");

    state.clear_subscription_events().await;

    client
        .subscribe_book(InstrumentId::from("BTC-USD.OKX"))
        .await
        .expect("subscribe book failed");

    wait_until_async(
        || {
            let state = state.clone();
            async move {
                state
                    .subscription_events()
                    .await
                    .iter()
                    .any(|(key, detail, ok)| {
                        key.starts_with("books") && detail.as_deref() == Some("BTC-USD") && *ok
                    })
            }
        },
        Duration::from_secs(1),
    )
    .await;
}

#[tokio::test]
async fn test_multiple_symbols_subscription() {
    let state = Arc::new(TestServerState::default());
    let addr = start_ws_server(state.clone()).await;
    let ws_url = format!("ws://{addr}/ws");

    let instruments = load_instruments();

    let mut client = connect_client(&ws_url).await;
    client.initialize_instruments_cache(instruments);
    client.connect().await.expect("connect failed");
    client
        .wait_until_active(1.0)
        .await
        .expect("client inactive");

    state.clear_subscription_events().await;

    let btc = InstrumentId::from("BTC-USD.OKX");
    let eth = InstrumentId::from("ETH-USD.OKX");

    client
        .subscribe_trades(btc, false)
        .await
        .expect("subscribe trades BTC");
    client
        .subscribe_trades(eth, false)
        .await
        .expect("subscribe trades ETH");

    wait_until_async(
        || {
            let state = state.clone();
            async move {
                let events = state.subscription_events().await;
                let btc_ok = events.iter().any(|(key, detail, ok)| {
                    key.starts_with("trades") && detail.as_deref() == Some("BTC-USD") && *ok
                });
                let eth_ok = events.iter().any(|(key, detail, ok)| {
                    key.starts_with("trades") && detail.as_deref() == Some("ETH-USD") && *ok
                });
                btc_ok && eth_ok
            }
        },
        Duration::from_secs(1),
    )
    .await;
}

#[tokio::test]
async fn test_unsubscribed_private_channel_not_resubscribed_after_disconnect() {
    let state = Arc::new(TestServerState::default());
    let addr = start_ws_server(state.clone()).await;
    let ws_url = format!("ws://{addr}/ws");

    let instruments = load_instruments();

    let mut client = connect_client(&ws_url).await;
    client.initialize_instruments_cache(instruments);
    client.connect().await.expect("connect failed");
    client
        .wait_until_active(1.0)
        .await
        .expect("client inactive");

    client
        .subscribe_trades(InstrumentId::from("BTC-USD.OKX"), false)
        .await
        .expect("subscribe trades failed");
    client
        .subscribe_orders(OKXInstrumentType::Spot)
        .await
        .expect("subscribe orders failed");

    wait_until_async(
        || {
            let state = state.clone();
            async move {
                let subscriptions = state.subscriptions.lock().await;
                let trade_count = subscriptions
                    .iter()
                    .filter(|value| value_matches_channel(value, "trades"))
                    .count();
                let orders_count = subscriptions
                    .iter()
                    .filter(|value| value_matches_channel(value, "orders"))
                    .count();
                trade_count >= 1 && orders_count >= 1
            }
        },
        Duration::from_secs(1),
    )
    .await;

    state.drop_next_connection.store(true, Ordering::Relaxed);

    client
        .unsubscribe_orders(OKXInstrumentType::Spot)
        .await
        .expect("unsubscribe orders failed");

    wait_until_async(
        || {
            let state = state.clone();
            async move {
                state
                    .unsubscriptions
                    .lock()
                    .await
                    .iter()
                    .any(|value| value_matches_channel(value, "orders"))
            }
        },
        Duration::from_secs(1),
    )
    .await;

    wait_until_async(
        || {
            let state = state.clone();
            async move { *state.login_count.lock().await >= 2 }
        },
        Duration::from_secs(3),
    )
    .await;

    // Allow time for any subscription replay after login
    tokio::time::sleep(Duration::from_millis(100)).await;

    let subscriptions = state.subscriptions.lock().await;
    let orders_count = subscriptions
        .iter()
        .filter(|value| value_matches_channel(value, "orders"))
        .count();
    let trades_count = subscriptions
        .iter()
        .filter(|value| value_matches_channel(value, "trades"))
        .count();

    assert_eq!(
        orders_count, 1,
        "orders channel was resubscribed unexpectedly"
    );
    assert!(
        trades_count >= 2,
        "expected trades channel to be restored on reconnect"
    );
}

#[tokio::test]
async fn test_auth_and_subscription_restoration_order() {
    let state = Arc::new(TestServerState::default());
    let addr = start_ws_server(state.clone()).await;
    let ws_url = format!("ws://{addr}/ws");

    let instruments = load_instruments();

    let mut client = connect_client(&ws_url).await;
    client.initialize_instruments_cache(instruments);
    client.connect().await.expect("connect failed");
    client
        .wait_until_active(1.0)
        .await
        .expect("client inactive");

    client
        .subscribe_trades(InstrumentId::from("BTC-USD.OKX"), false)
        .await
        .expect("subscribe trades failed");
    client
        .subscribe_orders(OKXInstrumentType::Spot)
        .await
        .expect("subscribe orders failed");

    state.clear_subscription_events().await;
    state.drop_next_connection.store(true, Ordering::Relaxed);

    wait_until_async(
        || {
            let state = state.clone();
            async move { *state.login_count.lock().await >= 2 }
        },
        Duration::from_secs(3),
    )
    .await;

    wait_until_async(
        || {
            let state = state.clone();
            async move {
                let events = state.subscription_events().await;
                let orders_ok = events
                    .iter()
                    .any(|(key, _, ok)| key.starts_with("orders") && *ok);
                let trades_ok = events
                    .iter()
                    .any(|(key, _, ok)| key.starts_with("trades") && *ok);
                orders_ok && trades_ok
            }
        },
        Duration::from_secs(2),
    )
    .await;

    client.close().await.expect("close failed");
}

#[tokio::test]
async fn test_unauthenticated_private_channel_rejection() {
    let state = Arc::new(TestServerState::default());
    let addr = start_ws_server(state.clone()).await;
    let ws_url = format!("ws://{addr}/ws");

    let instruments = load_instruments();

    let mut client = OKXWebSocketClient::new(
        Some(ws_url),
        None,
        None,
        None,
        Some(AccountId::from("OKX-TEST")),
        Some(30),
    )
    .expect("construct client");

    client.initialize_instruments_cache(instruments);
    client.connect().await.expect("connect failed");
    client
        .wait_until_active(1.0)
        .await
        .expect("client inactive");

    state.clear_subscription_events().await;

    client
        .subscribe_orders(OKXInstrumentType::Spot)
        .await
        .expect("subscribe orders call failed unexpectedly");

    wait_until_async(
        || {
            let state = state.clone();
            async move {
                state
                    .subscription_events()
                    .await
                    .iter()
                    .any(|(key, _, ok)| key.starts_with("orders") && !ok)
            }
        },
        Duration::from_secs(1),
    )
    .await;

    client.close().await.expect("close failed");
}

#[tokio::test]
async fn test_rapid_consecutive_reconnections() {
    // Test that rapid consecutive disconnects/reconnects don't cause state corruption
    let state = Arc::new(TestServerState::default());
    let addr = start_ws_server(state.clone()).await;
    let ws_url = format!("ws://{addr}/ws");

    let instruments = load_instruments();

    let mut client = connect_client(&ws_url).await;
    client.initialize_instruments_cache(instruments);
    client.connect().await.expect("connect failed");
    client
        .wait_until_active(1.0)
        .await
        .expect("client inactive");

    // Subscribe to multiple channels
    client
        .subscribe_trades(InstrumentId::from("BTC-USD.OKX"), false)
        .await
        .expect("subscribe trades failed");
    client
        .subscribe_book(InstrumentId::from("BTC-USD.OKX"))
        .await
        .expect("subscribe book failed");
    client
        .subscribe_orders(OKXInstrumentType::Spot)
        .await
        .expect("subscribe orders failed");

    wait_until_async(
        || {
            let state = state.clone();
            async move { *state.login_count.lock().await >= 1 }
        },
        Duration::from_secs(2),
    )
    .await;

    let initial_login_count = *state.login_count.lock().await;
    assert_eq!(initial_login_count, 1, "Should have 1 initial login");

    // Perform 3 rapid disconnect/reconnect cycles
    for cycle in 1..=3 {
        // Clear subscription events to verify fresh resubscriptions
        state.clear_subscription_events().await;

        state.drop_next_connection.store(true, Ordering::Relaxed);

        // Trigger disconnect by subscribing to a new channel
        client
            .subscribe_trades(InstrumentId::from("ETH-USD.OKX"), false)
            .await
            .expect("subscribe trigger failed");

        // Wait for reconnection
        wait_until_async(
            || {
                let state = state.clone();
                let expected = initial_login_count + cycle;
                async move { *state.login_count.lock().await >= expected }
            },
            Duration::from_secs(8),
        )
        .await;

        // Wait for subscription restoration (20s to account for slower CI runners)
        wait_until_async(
            || {
                let state = state.clone();
                async move {
                    let events = state.subscription_events().await;
                    events
                        .iter()
                        .any(|(key, _, ok)| key.starts_with("trades") && *ok)
                        && events
                            .iter()
                            .any(|(key, _, ok)| key.starts_with("orders") && *ok)
                }
            },
            Duration::from_secs(20),
        )
        .await;

        // Verify subscriptions were restored in this cycle
        let events = state.subscription_events().await;
        assert!(
            events
                .iter()
                .any(|(key, _, ok)| key.starts_with("trades") && *ok),
            "Cycle {cycle}: trades subscription should be restored; events={events:?}"
        );
        assert!(
            events
                .iter()
                .any(|(key, _, ok)| key.starts_with("orders") && *ok),
            "Cycle {cycle}: orders subscription should be restored; events={events:?}"
        );

        let login_count = *state.login_count.lock().await;
        assert_eq!(
            login_count,
            initial_login_count + cycle,
            "Login count mismatch after cycle {cycle}"
        );
    }

    // Verify final state
    let final_login_count = *state.login_count.lock().await;
    assert_eq!(
        final_login_count, 4,
        "Should have 4 total logins (1 initial + 3 reconnects)"
    );

    client.close().await.expect("close failed");
}

#[tokio::test]
async fn test_multiple_partial_subscription_failures() {
    // Test handling of subscription failures during restore and automatic retry
    // Note: OKX mock server drops connection on first failure, triggering immediate retry
    let state = Arc::new(TestServerState::default());
    let addr = start_ws_server(state.clone()).await;
    let ws_url = format!("ws://{addr}/ws");

    let instruments = load_instruments();

    let mut client = connect_client(&ws_url).await;
    client.initialize_instruments_cache(instruments);
    client.connect().await.expect("connect failed");
    client
        .wait_until_active(1.0)
        .await
        .expect("client inactive");

    let btc = InstrumentId::from("BTC-USD.OKX");
    let eth = InstrumentId::from("ETH-USD.OKX");

    // Subscribe to multiple channels
    client
        .subscribe_trades(btc, false)
        .await
        .expect("subscribe BTC trades failed");
    client
        .subscribe_trades(eth, false)
        .await
        .expect("subscribe ETH trades failed");
    client
        .subscribe_book(btc)
        .await
        .expect("subscribe book failed");
    client
        .subscribe_orders(OKXInstrumentType::Spot)
        .await
        .expect("subscribe orders failed");

    wait_until_async(
        || {
            let state = state.clone();
            async move {
                let events = state.subscription_events().await;
                events.iter().filter(|(_, _, ok)| *ok).count() >= 4
            }
        },
        Duration::from_secs(2),
    )
    .await;

    state.clear_subscription_events().await;

    // Set up one subscription to fail on next reconnect
    {
        let mut pending = state.fail_next_subscriptions.lock().await;
        pending.push("orders:SPOT".to_string());
    }

    // Trigger disconnect
    state.drop_next_connection.store(true, Ordering::Relaxed);
    client
        .subscribe_trades(InstrumentId::from("SOL-USD.OKX"), false)
        .await
        .expect("trigger disconnect failed");

    // Wait for the failure + automatic retry cycle
    // Flow: reconnect → try orders:SPOT → fail → drop → reconnect → retry successfully
    wait_until_async(
        || {
            let state = state.clone();
            async move {
                let events = state.subscription_events().await;
                events
                    .iter()
                    .any(|(key, _, ok)| key == "orders:SPOT" && !*ok)
                    && events
                        .iter()
                        .any(|(key, _, ok)| key == "orders:SPOT" && *ok)
            }
        },
        Duration::from_secs(10),
    )
    .await;

    let events = state.subscription_events().await;

    // Verify failure followed by successful retry
    assert!(
        events
            .iter()
            .any(|(key, _, ok)| key == "orders:SPOT" && !*ok),
        "Orders should fail initially: {events:?}"
    );
    assert!(
        events
            .iter()
            .any(|(key, _, ok)| key == "orders:SPOT" && *ok),
        "Orders should succeed on retry: {events:?}"
    );

    // Other subscriptions should succeed
    let other_success = events
        .iter()
        .filter(|(key, _, ok)| *ok && !key.contains("orders"))
        .count();
    assert!(
        other_success >= 1,
        "At least one other subscription should succeed: {events:?}"
    );

    client.close().await.expect("close failed");
}

#[tokio::test]
async fn test_reconnection_race_condition() {
    // Test disconnect request during active reconnection
    let state = Arc::new(TestServerState::default());
    let addr = start_ws_server(state.clone()).await;
    let ws_url = format!("ws://{addr}/ws");

    let instruments = load_instruments();

    let mut client = connect_client(&ws_url).await;
    client.initialize_instruments_cache(instruments);
    client.connect().await.expect("connect failed");
    client
        .wait_until_active(1.0)
        .await
        .expect("client inactive");

    client
        .subscribe_trades(InstrumentId::from("BTC-USD.OKX"), false)
        .await
        .expect("subscribe trades failed");
    client
        .subscribe_orders(OKXInstrumentType::Spot)
        .await
        .expect("subscribe orders failed");

    wait_until_async(
        || {
            let state = state.clone();
            async move { *state.login_count.lock().await >= 1 }
        },
        Duration::from_secs(2),
    )
    .await;

    // Add significant auth delay to create a window for race condition
    {
        let mut delay = state.auth_response_delay_ms.lock().await;
        *delay = Some(1000);
    }

    // Trigger first disconnect
    state.drop_next_connection.store(true, Ordering::Relaxed);
    client
        .subscribe_trades(InstrumentId::from("ETH-USD.OKX"), false)
        .await
        .expect("trigger disconnect failed");

    // Wait a bit for reconnection to start but not complete (due to auth delay)
    tokio::time::sleep(Duration::from_millis(300)).await;

    // Trigger another disconnect while reconnection is in progress
    state.drop_next_connection.store(true, Ordering::Relaxed);
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Clear the delay
    {
        let mut delay = state.auth_response_delay_ms.lock().await;
        *delay = None;
    }

    // Client should eventually recover
    wait_until_async(
        || {
            let state = state.clone();
            async move { *state.login_count.lock().await >= 2 }
        },
        Duration::from_secs(10),
    )
    .await;

    // Give time for subscriptions to restore
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Verify subscriptions are restored
    let subscriptions = state.subscriptions.lock().await;
    let trades_count = subscriptions
        .iter()
        .filter(|value| value_matches_channel(value, "trades"))
        .count();
    let orders_count = subscriptions
        .iter()
        .filter(|value| value_matches_channel(value, "orders"))
        .count();

    assert!(
        trades_count >= 1,
        "Should have at least 1 trade subscription restored"
    );
    assert!(
        orders_count >= 1,
        "Should have at least 1 order subscription restored"
    );

    client.close().await.expect("close failed");
}
