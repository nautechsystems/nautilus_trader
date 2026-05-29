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

//! Integration tests for the Derive WebSocket client against an axum mock
//! server. Mirrors the established pattern in `hyperliquid/tests/websocket.rs`
//! and reuses the SESSION_KEY_HEX / TEST_WALLET constants from
//! `derive/tests/http.rs`.

use std::{
    collections::HashMap,
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
    http::StatusCode,
    response::Response,
    routing::get,
};
use futures_util::StreamExt;
use nautilus_common::testing::wait_until_async;
use nautilus_derive::{
    common::enums::DeriveEnvironment,
    websocket::{
        DeriveWebSocketClient, DeriveWsCredentials, DeriveWsError, DeriveWsMessage,
        WsSubscriptionPayload,
    },
};
use nautilus_network::{http::HttpClient, websocket::TransportBackend};
use rstest::rstest;
use serde_json::{Value, json};

const SESSION_KEY_HEX: &str = "0x2ae8be44db8a590d20bffbe3b6872df9b569147d3bf6801a35a28281a4816bbd";
const TEST_WALLET: &str = "0x000000000000000000000000000000000000aaaa";

#[derive(Clone, Default)]
struct ServerState {
    connection_count: Arc<AtomicUsize>,
    login_frames: Arc<tokio::sync::Mutex<Vec<Value>>>,
    subscribe_frames: Arc<tokio::sync::Mutex<Vec<Value>>>,
    unsubscribe_frames: Arc<tokio::sync::Mutex<Vec<Value>>>,
    login_result: Arc<tokio::sync::Mutex<Option<Value>>>,
    subscribe_with_current_subscriptions: Arc<tokio::sync::Mutex<bool>>,
    reject_login: Arc<tokio::sync::Mutex<bool>>,
    reject_subscribe: Arc<tokio::sync::Mutex<bool>>,
    push_notification_on_subscribe: Arc<tokio::sync::Mutex<Option<Value>>>,
}

impl ServerState {
    fn new() -> Self {
        Self::default()
    }

    async fn captured_login(&self) -> Option<Value> {
        self.login_frames.lock().await.first().cloned()
    }

    async fn captured_subscribes(&self) -> Vec<Value> {
        self.subscribe_frames.lock().await.clone()
    }

    async fn captured_unsubscribes(&self) -> Vec<Value> {
        self.unsubscribe_frames.lock().await.clone()
    }
}

async fn handle_upgrade(ws: WebSocketUpgrade, State(state): State<ServerState>) -> Response {
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

async fn handle_socket(mut socket: WebSocket, state: ServerState) {
    state.connection_count.fetch_add(1, Ordering::SeqCst);

    while let Some(frame) = socket.next().await {
        let Ok(frame) = frame else { break };
        match frame {
            Message::Text(text) => {
                let Ok(payload) = serde_json::from_str::<Value>(&text) else {
                    continue;
                };
                let id = payload.get("id").and_then(Value::as_u64).unwrap_or(0);
                let method = payload
                    .get("method")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();

                match method.as_str() {
                    "public/login" => {
                        state.login_frames.lock().await.push(payload.clone());
                        let reject = *state.reject_login.lock().await;
                        let reply = if reject {
                            json!({"id": id, "error": {"code": -32602, "message": "bad signature"}})
                        } else {
                            let result = state
                                .login_result
                                .lock()
                                .await
                                .clone()
                                .unwrap_or_else(|| json!({"success": true}));
                            json!({"id": id, "result": result})
                        };

                        if socket
                            .send(Message::Text(reply.to_string().into()))
                            .await
                            .is_err()
                        {
                            break;
                        }
                    }
                    "subscribe" => {
                        state.subscribe_frames.lock().await.push(payload.clone());
                        let reject = *state.reject_subscribe.lock().await;
                        let reply = if reject {
                            json!({"id": id, "error": {"code": -32603, "message": "subscribe denied"}})
                        } else {
                            let channels = payload
                                .get("params")
                                .and_then(|p| p.get("channels"))
                                .cloned()
                                .unwrap_or_else(|| json!([]));

                            if *state.subscribe_with_current_subscriptions.lock().await {
                                let mut status = serde_json::Map::new();

                                if let Some(channels) = channels.as_array() {
                                    for channel in channels {
                                        if let Some(channel) = channel.as_str() {
                                            status.insert(channel.to_string(), json!("ok"));
                                        }
                                    }
                                }
                                json!({
                                    "id": id,
                                    "result": {
                                        "current_subscriptions": channels,
                                        "status": status,
                                    },
                                })
                            } else {
                                json!({"id": id, "result": {"channels": channels}})
                            }
                        };

                        if socket
                            .send(Message::Text(reply.to_string().into()))
                            .await
                            .is_err()
                        {
                            break;
                        }

                        if !reject
                            && let Some(notification) =
                                state.push_notification_on_subscribe.lock().await.clone()
                            && socket
                                .send(Message::Text(notification.to_string().into()))
                                .await
                                .is_err()
                        {
                            break;
                        }
                    }
                    "unsubscribe" => {
                        state.unsubscribe_frames.lock().await.push(payload.clone());
                        let reply = json!({"id": id, "result": {"success": true}});
                        if socket
                            .send(Message::Text(reply.to_string().into()))
                            .await
                            .is_err()
                        {
                            break;
                        }
                    }
                    _ => {}
                }
            }
            Message::Close(_) => break,
            _ => {}
        }
    }

    state.connection_count.fetch_sub(1, Ordering::SeqCst);
}

async fn start_server(state: ServerState) -> SocketAddr {
    let router = Router::new()
        .route("/ws", get(handle_upgrade))
        .route("/health", get(|| async { StatusCode::OK }))
        .with_state(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    wait_for_http_health(addr).await;
    addr
}

async fn wait_for_http_health(addr: SocketAddr) {
    let health_url = format!("http://{addr}/health");
    let http_client =
        HttpClient::new(HashMap::new(), Vec::new(), Vec::new(), None, None, None).unwrap();
    wait_until_async(
        || {
            let url = health_url.clone();
            let client = http_client.clone();
            async move { client.get(url, None, None, Some(1), None).await.is_ok() }
        },
        Duration::from_secs(5),
    )
    .await;
}

fn ws_url(addr: SocketAddr) -> String {
    format!("ws://{addr}/ws")
}

fn test_credentials() -> DeriveWsCredentials {
    DeriveWsCredentials::new(TEST_WALLET, SESSION_KEY_HEX).unwrap()
}

async fn wait_for_active(client: &DeriveWebSocketClient, timeout: Duration) {
    wait_until_async(|| async { client.is_active() }, timeout).await;
}

async fn wait_for_inactive(client: &DeriveWebSocketClient, timeout: Duration) {
    wait_until_async(|| async { !client.is_active() }, timeout).await;
}

#[rstest]
#[tokio::test]
async fn test_connect_with_credentials_completes_login() {
    let state = ServerState::new();
    let addr = start_server(state.clone()).await;

    let mut client = DeriveWebSocketClient::with_credentials(
        Some(ws_url(addr)),
        DeriveEnvironment::Mainnet,
        TransportBackend::default(),
        None,
        test_credentials(),
    );
    client.connect().await.expect("connect failed");
    wait_for_active(&client, Duration::from_secs(2)).await;

    assert!(client.is_active());
    assert!(client.is_authenticated());

    let login = state.captured_login().await.expect("login captured");
    assert_eq!(login["jsonrpc"], "2.0");
    assert_eq!(login["method"], "public/login");
    let params = &login["params"];
    assert_eq!(params["wallet"], TEST_WALLET);
    let signature = params["signature"].as_str().expect("signature is string");
    assert!(signature.starts_with("0x"));
    assert_eq!(signature.len(), 2 + 130, "signature is 65-byte hex");
    let timestamp: u64 = params["timestamp"]
        .as_str()
        .expect("timestamp string")
        .parse()
        .expect("timestamp parses");
    assert!(timestamp > 1_700_000_000_000);

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_connect_accepts_venue_array_login_result() {
    let state = ServerState::new();
    *state.login_result.lock().await = Some(json!([30769]));
    let addr = start_server(state).await;

    let mut client = DeriveWebSocketClient::with_credentials(
        Some(ws_url(addr)),
        DeriveEnvironment::Mainnet,
        TransportBackend::default(),
        None,
        test_credentials(),
    );
    client.connect().await.expect("connect failed");
    wait_for_active(&client, Duration::from_secs(2)).await;

    assert!(client.is_active());
    assert!(client.is_authenticated());

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_connect_with_login_rejection_tears_down_transport() {
    let state = ServerState::new();
    *state.reject_login.lock().await = true;
    let addr = start_server(state.clone()).await;

    let mut client = DeriveWebSocketClient::with_credentials(
        Some(ws_url(addr)),
        DeriveEnvironment::Mainnet,
        TransportBackend::default(),
        None,
        test_credentials(),
    );
    let err = client.connect().await.expect_err("login must reject");
    match err {
        DeriveWsError::JsonRpc { code, .. } => assert_eq!(code, -32602),
        other => panic!("expected JsonRpc(-32602), was {other:?}"),
    }
    wait_for_inactive(&client, Duration::from_secs(2)).await;
    assert!(!client.is_active(), "transport must be torn down");
    assert!(!client.is_authenticated());

    // Retry must rebuild from a clean slate.
    *state.reject_login.lock().await = false;
    client.connect().await.expect("retry connect");
    wait_for_active(&client, Duration::from_secs(2)).await;
    assert!(client.is_authenticated());
    assert_eq!(state.login_frames.lock().await.len(), 2);

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_subscribe_ticker_sends_jsonrpc_subscribe_and_tracks_channel() {
    let state = ServerState::new();
    let addr = start_server(state.clone()).await;

    let mut client = DeriveWebSocketClient::new(
        Some(ws_url(addr)),
        DeriveEnvironment::Mainnet,
        TransportBackend::default(),
        None,
    );
    client.connect().await.expect("connect failed");
    wait_for_active(&client, Duration::from_secs(2)).await;

    client
        .subscribe_ticker("ETH-PERP", "1000")
        .await
        .expect("subscribe failed");

    let frames = state.captured_subscribes().await;
    assert_eq!(frames.len(), 1);
    assert_eq!(frames[0]["method"], "subscribe");
    assert_eq!(
        frames[0]["params"]["channels"][0],
        "ticker_slim.ETH-PERP.1000",
    );
    assert_eq!(client.subscription_count(), 1);

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_subscribe_accepts_current_subscriptions_ack_and_tracks_channel() {
    let state = ServerState::new();
    *state.subscribe_with_current_subscriptions.lock().await = true;
    let addr = start_server(state.clone()).await;

    let mut client = DeriveWebSocketClient::new(
        Some(ws_url(addr)),
        DeriveEnvironment::Mainnet,
        TransportBackend::default(),
        None,
    );
    client.connect().await.expect("connect failed");
    wait_for_active(&client, Duration::from_secs(2)).await;

    client
        .subscribe_ticker("ETH-PERP", "1000")
        .await
        .expect("subscribe failed");

    let frames = state.captured_subscribes().await;
    assert_eq!(frames.len(), 1);
    assert_eq!(frames[0]["method"], "subscribe");
    assert_eq!(
        frames[0]["params"]["channels"][0],
        "ticker_slim.ETH-PERP.1000",
    );
    assert_eq!(client.subscription_count(), 1);

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_subscribe_orderbook_sends_jsonrpc_subscribe_and_tracks_channel() {
    let state = ServerState::new();
    let addr = start_server(state.clone()).await;

    let mut client = DeriveWebSocketClient::new(
        Some(ws_url(addr)),
        DeriveEnvironment::Mainnet,
        TransportBackend::default(),
        None,
    );
    client.connect().await.expect("connect failed");
    wait_for_active(&client, Duration::from_secs(2)).await;

    client
        .subscribe_orderbook("ETH-PERP", "1", "10")
        .await
        .expect("subscribe failed");

    let frames = state.captured_subscribes().await;
    assert_eq!(frames.len(), 1);
    assert_eq!(frames[0]["method"], "subscribe");
    assert_eq!(
        frames[0]["params"]["channels"][0],
        "orderbook.ETH-PERP.1.10"
    );
    assert_eq!(client.subscription_count(), 1);

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_subscribe_trades_sends_jsonrpc_subscribe_and_tracks_channel() {
    let state = ServerState::new();
    let addr = start_server(state.clone()).await;

    let mut client = DeriveWebSocketClient::new(
        Some(ws_url(addr)),
        DeriveEnvironment::Mainnet,
        TransportBackend::default(),
        None,
    );
    client.connect().await.expect("connect failed");
    wait_for_active(&client, Duration::from_secs(2)).await;

    client
        .subscribe_trades("perp", "ETH")
        .await
        .expect("subscribe failed");

    let frames = state.captured_subscribes().await;
    assert_eq!(frames.len(), 1);
    assert_eq!(frames[0]["method"], "subscribe");
    assert_eq!(frames[0]["params"]["channels"][0], "trades.perp.ETH");
    assert_eq!(client.subscription_count(), 1);

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_subscribe_failure_does_not_track_channel() {
    let state = ServerState::new();
    *state.reject_subscribe.lock().await = true;
    let addr = start_server(state.clone()).await;

    let mut client = DeriveWebSocketClient::new(
        Some(ws_url(addr)),
        DeriveEnvironment::Mainnet,
        TransportBackend::default(),
        None,
    );
    client.connect().await.expect("connect failed");
    wait_for_active(&client, Duration::from_secs(2)).await;

    let err = client
        .subscribe_ticker("ETH-PERP", "1000")
        .await
        .expect_err("subscribe must reject");

    match err {
        DeriveWsError::JsonRpc { code, .. } => assert_eq!(code, -32603),
        other => panic!("expected JsonRpc(-32603), was {other:?}"),
    }
    assert_eq!(
        client.subscription_count(),
        0,
        "failed subscribe must not be tracked",
    );

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_subscription_notification_yields_message() {
    let state = ServerState::new();
    *state.push_notification_on_subscribe.lock().await = Some(json!({
        "method": "subscription",
        "params": {
            "channel": "ticker_slim.ETH-PERP.1000",
            "data": {"instrument_name": "ETH-PERP", "mark_price": "3500.5"},
        },
    }));
    let addr = start_server(state).await;

    let mut client = DeriveWebSocketClient::new(
        Some(ws_url(addr)),
        DeriveEnvironment::Mainnet,
        TransportBackend::default(),
        None,
    );
    client.connect().await.expect("connect failed");
    wait_for_active(&client, Duration::from_secs(2)).await;
    client
        .subscribe_ticker("ETH-PERP", "1000")
        .await
        .expect("subscribe failed");

    // Drain `next_event` until we observe the Subscription frame or time out.
    let payload = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            match client.next_event().await {
                Some(DeriveWsMessage::Subscription(payload)) => return payload,
                Some(_) => {}
                None => panic!("event stream closed before subscription arrived"),
            }
        }
    })
    .await
    .expect("notification arrived in time");

    let WsSubscriptionPayload { channel, data } = payload;
    assert_eq!(channel.as_str(), "ticker_slim.ETH-PERP.1000");
    assert_eq!(data["instrument_name"], "ETH-PERP");
    assert_eq!(data["mark_price"], "3500.5");

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_double_connect_is_idempotent_when_healthy() {
    let state = ServerState::new();
    let addr = start_server(state.clone()).await;

    let mut client = DeriveWebSocketClient::new(
        Some(ws_url(addr)),
        DeriveEnvironment::Mainnet,
        TransportBackend::default(),
        None,
    );
    client.connect().await.expect("first connect failed");
    wait_for_active(&client, Duration::from_secs(2)).await;

    client
        .subscribe_ticker("ETH-PERP", "1000")
        .await
        .expect("subscribe failed");
    assert_eq!(client.subscription_count(), 1);

    client.connect().await.expect("second connect failed");
    assert_eq!(state.connection_count.load(Ordering::SeqCst), 1);
    assert_eq!(client.subscription_count(), 1);

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_disconnect_resets_state_and_allows_reconnect() {
    let state = ServerState::new();
    let addr = start_server(state.clone()).await;

    let mut client = DeriveWebSocketClient::new(
        Some(ws_url(addr)),
        DeriveEnvironment::Mainnet,
        TransportBackend::default(),
        None,
    );
    client.connect().await.expect("connect failed");
    wait_for_active(&client, Duration::from_secs(2)).await;

    // Subscribe so disconnect-clears-tracked-state is observable.
    client
        .subscribe_ticker("ETH-PERP", "1000")
        .await
        .expect("subscribe failed");
    assert_eq!(client.subscription_count(), 1);

    client.disconnect().await.expect("disconnect failed");
    wait_for_inactive(&client, Duration::from_secs(2)).await;
    assert!(!client.is_active());
    assert_eq!(
        client.subscription_count(),
        0,
        "disconnect must clear tracked subscriptions",
    );

    client.connect().await.expect("reconnect failed");
    wait_for_active(&client, Duration::from_secs(2)).await;
    assert!(client.is_active());

    wait_until_async(
        || {
            let state = state.clone();
            async move { state.connection_count.load(Ordering::SeqCst) == 1 }
        },
        Duration::from_secs(2),
    )
    .await;

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_unsubscribe_ticker_removes_from_tracked_set() {
    let state = ServerState::new();
    let addr = start_server(state.clone()).await;

    let mut client = DeriveWebSocketClient::new(
        Some(ws_url(addr)),
        DeriveEnvironment::Mainnet,
        TransportBackend::default(),
        None,
    );
    client.connect().await.expect("connect failed");
    wait_for_active(&client, Duration::from_secs(2)).await;

    client
        .subscribe_ticker("ETH-PERP", "1000")
        .await
        .expect("subscribe failed");
    assert_eq!(client.subscription_count(), 1);

    client
        .unsubscribe_ticker("ETH-PERP", "1000")
        .await
        .expect("unsubscribe failed");
    assert_eq!(client.subscription_count(), 0);

    let frames = state.captured_unsubscribes().await;
    assert_eq!(frames.len(), 1);
    assert_eq!(frames[0]["method"], "unsubscribe");
    assert_eq!(
        frames[0]["params"]["channels"][0],
        "ticker_slim.ETH-PERP.1000",
    );

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_unsubscribe_orderbook_removes_from_tracked_set() {
    let state = ServerState::new();
    let addr = start_server(state.clone()).await;

    let mut client = DeriveWebSocketClient::new(
        Some(ws_url(addr)),
        DeriveEnvironment::Mainnet,
        TransportBackend::default(),
        None,
    );
    client.connect().await.expect("connect failed");
    wait_for_active(&client, Duration::from_secs(2)).await;

    client
        .subscribe_orderbook("ETH-PERP", "1", "20")
        .await
        .expect("subscribe failed");
    assert_eq!(client.subscription_count(), 1);

    client
        .unsubscribe_orderbook("ETH-PERP", "1", "20")
        .await
        .expect("unsubscribe failed");
    assert_eq!(client.subscription_count(), 0);

    let frames = state.captured_unsubscribes().await;
    assert_eq!(frames.len(), 1);
    assert_eq!(frames[0]["method"], "unsubscribe");
    assert_eq!(
        frames[0]["params"]["channels"][0],
        "orderbook.ETH-PERP.1.20"
    );

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_unsubscribe_trades_removes_from_tracked_set() {
    let state = ServerState::new();
    let addr = start_server(state.clone()).await;

    let mut client = DeriveWebSocketClient::new(
        Some(ws_url(addr)),
        DeriveEnvironment::Mainnet,
        TransportBackend::default(),
        None,
    );
    client.connect().await.expect("connect failed");
    wait_for_active(&client, Duration::from_secs(2)).await;

    client
        .subscribe_trades("perp", "ETH")
        .await
        .expect("subscribe failed");
    assert_eq!(client.subscription_count(), 1);

    client
        .unsubscribe_trades("perp", "ETH")
        .await
        .expect("unsubscribe failed");
    assert_eq!(client.subscription_count(), 0);

    let frames = state.captured_unsubscribes().await;
    assert_eq!(frames.len(), 1);
    assert_eq!(frames[0]["method"], "unsubscribe");
    assert_eq!(frames[0]["params"]["channels"][0], "trades.perp.ETH");

    client.disconnect().await.unwrap();
}
