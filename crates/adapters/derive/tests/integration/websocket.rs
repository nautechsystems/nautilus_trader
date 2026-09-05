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
//! server. Mirrors the established pattern in `hyperliquid/tests/integration/websocket.rs`
//! and reuses the SESSION_KEY_HEX / TEST_WALLET constants from
//! `derive/tests/integration/http.rs`.

use std::{
    collections::HashMap,
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
    http::StatusCode,
    response::Response,
    routing::get,
};
use futures_util::StreamExt;
use nautilus_common::testing::wait_until_async;
use nautilus_derive::{
    common::enums::DeriveEnvironment,
    http::query::{DeriveCancelAllParams, DeriveCancelByInstrumentParams},
    websocket::{
        DeriveWebSocketClient, DeriveWsChannel, DeriveWsCredentials, DeriveWsError,
        DeriveWsMessage, WsSubscriptionPayload,
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
    subscribe_status: Arc<tokio::sync::Mutex<Option<HashMap<String, String>>>>,
    subscribe_status_after_first: Arc<tokio::sync::Mutex<Option<HashMap<String, String>>>>,
    subscribe_with_current_subscriptions: Arc<tokio::sync::Mutex<bool>>,
    reject_login: Arc<tokio::sync::Mutex<bool>>,
    reject_subscribe: Arc<tokio::sync::Mutex<bool>>,
    login_failures_after_first: Arc<AtomicUsize>,
    subscribe_failures_after_first: Arc<AtomicUsize>,
    subscribe_timeouts_after_first: Arc<AtomicUsize>,
    disconnect_after_subscribe: Arc<AtomicBool>,
    disconnect_after_subscribe_responses: Arc<AtomicUsize>,
    disconnect_recovery_subscribes: Arc<AtomicUsize>,
    disconnect_before_private_reply: Arc<AtomicBool>,
    private_frames: Arc<tokio::sync::Mutex<Vec<Value>>>,
    push_notification_on_subscribe: Arc<tokio::sync::Mutex<Option<Value>>>,
    push_notification_after_first: Arc<AtomicBool>,
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
                        let login_count = {
                            let mut frames = state.login_frames.lock().await;
                            frames.push(payload.clone());
                            frames.len()
                        };
                        let reject_reconnect = login_count > 1
                            && state
                                .login_failures_after_first
                                .try_update(Ordering::SeqCst, Ordering::SeqCst, |remaining| {
                                    remaining.checked_sub(1)
                                })
                                .is_ok();
                        let reject = *state.reject_login.lock().await || reject_reconnect;
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
                        let subscribe_count = {
                            let mut frames = state.subscribe_frames.lock().await;
                            frames.push(payload.clone());
                            frames.len()
                        };
                        let reject_reconnect = subscribe_count > 1
                            && state
                                .subscribe_failures_after_first
                                .try_update(Ordering::SeqCst, Ordering::SeqCst, |remaining| {
                                    remaining.checked_sub(1)
                                })
                                .is_ok();
                        let reject = *state.reject_subscribe.lock().await || reject_reconnect;
                        let timeout_reconnect = subscribe_count > 1
                            && state
                                .subscribe_timeouts_after_first
                                .try_update(Ordering::SeqCst, Ordering::SeqCst, |remaining| {
                                    remaining.checked_sub(1)
                                })
                                .is_ok();

                        if timeout_reconnect {
                            continue;
                        }
                        let disconnect_recovery = subscribe_count > 1
                            && state
                                .disconnect_recovery_subscribes
                                .try_update(Ordering::SeqCst, Ordering::SeqCst, |remaining| {
                                    remaining.checked_sub(1)
                                })
                                .is_ok();

                        if disconnect_recovery {
                            let _ = socket.send(Message::Close(None)).await;
                            break;
                        }
                        let reply = if reject {
                            json!({"id": id, "error": {"code": -32603, "message": "subscribe denied"}})
                        } else {
                            let channels = payload
                                .get("params")
                                .and_then(|p| p.get("channels"))
                                .cloned()
                                .unwrap_or_else(|| json!([]));

                            let reconnect_status = if subscribe_count > 1 {
                                state.subscribe_status_after_first.lock().await.clone()
                            } else {
                                None
                            };
                            let status = match reconnect_status {
                                Some(status) => Some(status),
                                None => state.subscribe_status.lock().await.clone(),
                            };

                            if let Some(status) = status {
                                let current_subscriptions = channels
                                    .as_array()
                                    .into_iter()
                                    .flatten()
                                    .filter(|channel| {
                                        channel
                                            .as_str()
                                            .and_then(|channel| status.get(channel))
                                            .is_some_and(|status| status == "ok")
                                    })
                                    .cloned()
                                    .collect::<Vec<_>>();
                                json!({
                                    "id": id,
                                    "result": {
                                        "current_subscriptions": current_subscriptions,
                                        "status": status,
                                    },
                                })
                            } else if *state.subscribe_with_current_subscriptions.lock().await {
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
                            && (!state.push_notification_after_first.load(Ordering::SeqCst)
                                || subscribe_count > 1)
                            && let Some(notification) =
                                state.push_notification_on_subscribe.lock().await.clone()
                            && socket
                                .send(Message::Text(notification.to_string().into()))
                                .await
                                .is_err()
                        {
                            break;
                        }

                        let disconnect_after_response = state
                            .disconnect_after_subscribe_responses
                            .try_update(Ordering::SeqCst, Ordering::SeqCst, |remaining| {
                                remaining.checked_sub(1)
                            })
                            .is_ok();

                        if disconnect_after_response
                            || state
                                .disconnect_after_subscribe
                                .swap(false, Ordering::SeqCst)
                        {
                            let _ = socket.send(Message::Close(None)).await;
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
                    "private/cancel_all" | "private/cancel_by_instrument" => {
                        state.private_frames.lock().await.push(payload);
                        if state
                            .disconnect_before_private_reply
                            .swap(false, Ordering::SeqCst)
                        {
                            let _ = socket.send(Message::Close(None)).await;
                            break;
                        }
                        let result = if method == "private/cancel_by_instrument" {
                            json!({"cancelled_orders": 0})
                        } else {
                            json!({})
                        };
                        let reply = json!({"id": id, "result": result});
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
    let http_client = HttpClient::builder().build().unwrap();
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

async fn next_reconnected_subscription(
    client: &mut DeriveWebSocketClient,
) -> WsSubscriptionPayload {
    tokio::time::timeout(Duration::from_secs(5), async {
        let mut payload = None;
        let mut reconnected = false;

        loop {
            match client.next_event().await {
                Some(DeriveWsMessage::Reconnected) => reconnected = true,
                Some(DeriveWsMessage::Subscription(update)) => payload = Some(update),
                Some(_) => {}
                None => panic!("event stream closed before reconnect completed"),
            }

            if reconnected && let Some(payload) = payload.take() {
                break payload;
            }
        }
    })
    .await
    .expect("reconnected subscription timed out")
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
        None,
        None,
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
        None,
        None,
    );
    client.connect().await.expect("connect failed");
    wait_for_active(&client, Duration::from_secs(2)).await;

    assert!(client.is_active());
    assert!(client.is_authenticated());

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_connect_rejects_unsuccessful_login_result() {
    let state = ServerState::new();
    *state.login_result.lock().await = Some(json!({"success": false}));
    let addr = start_server(state).await;

    let mut client = DeriveWebSocketClient::with_credentials(
        Some(ws_url(addr)),
        DeriveEnvironment::Mainnet,
        TransportBackend::default(),
        None,
        test_credentials(),
        None,
        None,
    );
    let error = client
        .connect()
        .await
        .expect_err("unsuccessful login result must reject connect");

    assert!(matches!(error, DeriveWsError::Authentication { .. }));
    assert!(!client.is_active());
    assert!(!client.is_authenticated());
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
        None,
        None,
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
async fn test_cancel_by_instrument_sends_exact_request_and_decodes_count() {
    let state = ServerState::new();
    let addr = start_server(state.clone()).await;

    let mut client = DeriveWebSocketClient::with_credentials(
        Some(ws_url(addr)),
        DeriveEnvironment::Mainnet,
        TransportBackend::default(),
        None,
        test_credentials(),
        None,
        None,
    );
    let execution = client.execution_handle();
    client.connect().await.expect("connect failed");

    let result = execution
        .cancel_by_instrument(&DeriveCancelByInstrumentParams::new(30769, "ETH-PERP"))
        .await
        .expect("cancel_by_instrument failed");

    assert_eq!(result.cancelled_orders, 0);
    assert_eq!(
        state.private_frames.lock().await.as_slice(),
        &[json!({
            "id": 2,
            "jsonrpc": "2.0",
            "method": "private/cancel_by_instrument",
            "params": {
                "subaccount_id": 30769,
                "instrument_name": "ETH-PERP",
            },
        })],
    );

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
async fn test_bulk_subscribe_records_only_channels_with_ok_status() {
    let state = ServerState::new();
    *state.subscribe_status.lock().await = Some(HashMap::from([
        ("30769.orders".to_string(), "ok".to_string()),
        ("30769.trades".to_string(), "unauthorized".to_string()),
        ("30769.balances".to_string(), "ok".to_string()),
    ]));
    let addr = start_server(state).await;

    let mut client = DeriveWebSocketClient::new(
        Some(ws_url(addr)),
        DeriveEnvironment::Mainnet,
        TransportBackend::default(),
        None,
    );
    client.connect().await.expect("connect failed");

    let err = client
        .subscribe_channels(vec![
            DeriveWsChannel::orders(30769),
            DeriveWsChannel::private_trades(30769),
            DeriveWsChannel::balances(30769),
        ])
        .await
        .expect_err("mixed subscribe status must fail");

    assert!(matches!(err, DeriveWsError::Subscription { .. }));
    assert!(err.to_string().contains("30769.trades: unauthorized"));
    assert_eq!(client.subscription_count(), 2);

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
    let data: serde_json::Value = serde_json::from_str(data.get()).unwrap();

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
async fn test_reconnect_retries_login_before_signaling_reconnected() {
    let state = ServerState::new();
    state.login_failures_after_first.store(2, Ordering::SeqCst);
    state
        .disconnect_after_subscribe
        .store(true, Ordering::SeqCst);
    let addr = start_server(state.clone()).await;

    let mut client = DeriveWebSocketClient::with_credentials(
        Some(ws_url(addr)),
        DeriveEnvironment::Mainnet,
        TransportBackend::default(),
        None,
        test_credentials(),
        None,
        None,
    );
    let execution = client.execution_handle();
    client.connect().await.expect("connect failed");
    client
        .subscribe_ticker("ETH-PERP", "1000")
        .await
        .expect("subscribe failed");

    wait_until_async(
        || {
            let state = state.clone();
            async move { state.login_frames.lock().await.len() >= 2 }
        },
        Duration::from_secs(5),
    )
    .await;
    assert!(!client.is_authenticated());

    let recovered = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            match client.next_event().await {
                Some(DeriveWsMessage::Reconnected) => break,
                Some(_) => {}
                None => panic!("event stream closed before reconnect completed"),
            }
        }
    })
    .await;

    assert!(recovered.is_ok(), "reconnect completion timed out");
    assert!(client.is_authenticated());
    assert_eq!(state.login_frames.lock().await.len(), 4);
    assert_eq!(state.subscribe_frames.lock().await.len(), 2);
    execution
        .cancel_all_orders(&DeriveCancelAllParams::new(30769))
        .await
        .expect("private request should succeed after reconnect");

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_reconnect_restores_public_session_without_credentials() {
    let state = ServerState::new();
    state
        .disconnect_after_subscribe
        .store(true, Ordering::SeqCst);
    *state.push_notification_on_subscribe.lock().await = Some(json!({
        "method": "subscription",
        "params": {
            "channel": "ticker_slim.ETH-PERP.1000",
            "data": {"instrument_name": "ETH-PERP", "mark_price": "3500.5"},
        },
    }));
    state
        .push_notification_after_first
        .store(true, Ordering::SeqCst);
    let addr = start_server(state.clone()).await;

    let mut client = DeriveWebSocketClient::new(
        Some(ws_url(addr)),
        DeriveEnvironment::Mainnet,
        TransportBackend::default(),
        None,
    );
    client.connect().await.expect("connect failed");
    client
        .subscribe_ticker("ETH-PERP", "1000")
        .await
        .expect("initial subscribe failed");

    let payload = next_reconnected_subscription(&mut client).await;

    assert!(client.is_active());
    assert!(!client.is_authenticated());
    assert!(state.login_frames.lock().await.is_empty());
    assert_eq!(state.subscribe_frames.lock().await.len(), 2);
    assert_eq!(payload.channel.as_str(), "ticker_slim.ETH-PERP.1000");
    let data: serde_json::Value = serde_json::from_str(payload.data.get()).unwrap();
    assert_eq!(data["instrument_name"], "ETH-PERP");
    assert_eq!(data["mark_price"], "3500.5");

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_reconnect_accepts_already_subscribed_replay_status() {
    let state = ServerState::new();
    *state.subscribe_status_after_first.lock().await = Some(HashMap::from([(
        "ticker_slim.ETH-PERP.1000".to_string(),
        "already subscribed".to_string(),
    )]));
    state
        .disconnect_after_subscribe
        .store(true, Ordering::SeqCst);
    let addr = start_server(state.clone()).await;

    let mut client = DeriveWebSocketClient::new(
        Some(ws_url(addr)),
        DeriveEnvironment::Mainnet,
        TransportBackend::default(),
        None,
    );
    client.connect().await.expect("connect failed");
    client
        .subscribe_ticker("ETH-PERP", "1000")
        .await
        .expect("initial subscribe failed");

    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            match client.next_event().await {
                Some(DeriveWsMessage::Reconnected) => break,
                Some(DeriveWsMessage::SessionRecoveryFailed(reason)) => {
                    panic!("idempotent replay failed: {reason}")
                }
                Some(_) => {}
                None => panic!("event stream closed before recovery completed"),
            }
        }
    })
    .await
    .expect("idempotent replay timed out");

    assert!(client.is_active());
    assert_eq!(client.subscription_count(), 1);
    assert_eq!(state.subscribe_frames.lock().await.len(), 2);

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_reconnect_surfaces_exhausted_login_retries_and_closes_transport() {
    let state = ServerState::new();
    state.login_failures_after_first.store(10, Ordering::SeqCst);
    state
        .disconnect_after_subscribe
        .store(true, Ordering::SeqCst);
    let addr = start_server(state.clone()).await;

    let mut client = DeriveWebSocketClient::with_credentials(
        Some(ws_url(addr)),
        DeriveEnvironment::Mainnet,
        TransportBackend::default(),
        None,
        test_credentials(),
        None,
        None,
    );
    let execution = client.execution_handle();
    client.connect().await.expect("connect failed");
    client
        .subscribe_ticker("ETH-PERP", "1000")
        .await
        .expect("subscribe failed");

    let reason = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            match client.next_event().await {
                Some(DeriveWsMessage::SessionRecoveryFailed(reason)) => break reason,
                Some(_) => {}
                None => panic!("event stream closed before recovery failure was surfaced"),
            }
        }
    })
    .await
    .expect("recovery failure timed out");
    let error = execution
        .cancel_all_orders(&DeriveCancelAllParams::new(30769))
        .await
        .expect_err("private request must fail after exhausted re-login");

    assert!(reason.contains("bad signature"));
    assert!(matches!(error, DeriveWsError::Authentication { .. }));
    assert_eq!(state.login_frames.lock().await.len(), 4);
    wait_for_inactive(&client, Duration::from_secs(2)).await;
    assert!(!client.is_active());
    assert!(!client.is_authenticated());
}

#[rstest]
#[tokio::test]
async fn test_reconnect_retries_complete_session_after_subscription_failure() {
    let state = ServerState::new();
    state
        .subscribe_failures_after_first
        .store(1, Ordering::SeqCst);
    state
        .disconnect_after_subscribe
        .store(true, Ordering::SeqCst);
    *state.push_notification_on_subscribe.lock().await = Some(json!({
        "method": "subscription",
        "params": {
            "channel": "ticker_slim.ETH-PERP.1000",
            "data": {"instrument_name": "ETH-PERP", "mark_price": "3500.5"},
        },
    }));
    state
        .push_notification_after_first
        .store(true, Ordering::SeqCst);
    let addr = start_server(state.clone()).await;

    let mut client = DeriveWebSocketClient::with_credentials(
        Some(ws_url(addr)),
        DeriveEnvironment::Mainnet,
        TransportBackend::default(),
        None,
        test_credentials(),
        None,
        None,
    );
    let execution = client.execution_handle();
    client.connect().await.expect("connect failed");
    client
        .subscribe_ticker("ETH-PERP", "1000")
        .await
        .expect("initial subscribe failed");

    wait_until_async(
        || {
            let state = state.clone();
            async move { state.subscribe_frames.lock().await.len() >= 2 }
        },
        Duration::from_secs(5),
    )
    .await;
    assert!(!client.is_authenticated());

    let pending_private = tokio::spawn(async move {
        execution
            .cancel_all_orders(&DeriveCancelAllParams::new(30769))
            .await
    });
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(
        !pending_private.is_finished(),
        "private requests must remain fenced until subscription replay succeeds",
    );

    let payload = next_reconnected_subscription(&mut client).await;
    pending_private
        .await
        .expect("private request task failed")
        .expect("private request should succeed after full session recovery");

    assert!(client.is_active());
    assert!(client.is_authenticated());
    assert_eq!(state.login_frames.lock().await.len(), 3);
    assert_eq!(state.subscribe_frames.lock().await.len(), 3);
    assert_eq!(payload.channel.as_str(), "ticker_slim.ETH-PERP.1000");
    let data: serde_json::Value = serde_json::from_str(payload.data.get()).unwrap();
    assert_eq!(data["instrument_name"], "ETH-PERP");
    assert_eq!(data["mark_price"], "3500.5");

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_reconnect_retries_complete_session_after_subscription_timeout() {
    let state = ServerState::new();
    state
        .subscribe_timeouts_after_first
        .store(1, Ordering::SeqCst);
    state
        .disconnect_after_subscribe
        .store(true, Ordering::SeqCst);
    let addr = start_server(state.clone()).await;

    let mut client = DeriveWebSocketClient::with_credentials(
        Some(ws_url(addr)),
        DeriveEnvironment::Mainnet,
        TransportBackend::default(),
        None,
        test_credentials(),
        None,
        None,
    );
    client.set_request_timeout(Duration::from_millis(100));
    client.connect().await.expect("connect failed");
    client
        .subscribe_ticker("ETH-PERP", "1000")
        .await
        .expect("initial subscribe failed");

    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            match client.next_event().await {
                Some(DeriveWsMessage::Reconnected) => break,
                Some(_) => {}
                None => panic!("event stream closed before reconnect completed"),
            }
        }
    })
    .await
    .expect("timeout recovery did not complete");

    assert!(client.is_active());
    assert!(client.is_authenticated());
    assert_eq!(state.login_frames.lock().await.len(), 3);
    assert_eq!(state.subscribe_frames.lock().await.len(), 3);

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_reconnect_retries_session_after_second_connection_loss() {
    let state = ServerState::new();
    state
        .disconnect_recovery_subscribes
        .store(1, Ordering::SeqCst);
    state
        .disconnect_after_subscribe
        .store(true, Ordering::SeqCst);
    let addr = start_server(state.clone()).await;

    let mut client = DeriveWebSocketClient::with_credentials(
        Some(ws_url(addr)),
        DeriveEnvironment::Mainnet,
        TransportBackend::default(),
        None,
        test_credentials(),
        None,
        None,
    );
    let execution = client.execution_handle();
    client.connect().await.expect("connect failed");
    client
        .subscribe_ticker("ETH-PERP", "1000")
        .await
        .expect("initial subscribe failed");

    tokio::time::timeout(Duration::from_secs(8), async {
        loop {
            match client.next_event().await {
                Some(DeriveWsMessage::Reconnected) => break,
                Some(_) => {}
                None => panic!("event stream closed before reconnect completed"),
            }
        }
    })
    .await
    .expect("second-loss recovery timed out");
    execution
        .cancel_all_orders(&DeriveCancelAllParams::new(30769))
        .await
        .expect("private request should succeed after repeated reconnect");

    assert!(client.is_active());
    assert!(client.is_authenticated());
    assert_eq!(state.login_frames.lock().await.len(), 3);
    assert_eq!(state.subscribe_frames.lock().await.len(), 3);

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_reconnect_recovers_loss_after_replay_ack() {
    let state = ServerState::new();
    state
        .disconnect_after_subscribe_responses
        .store(2, Ordering::SeqCst);
    let addr = start_server(state.clone()).await;

    let mut client = DeriveWebSocketClient::with_credentials(
        Some(ws_url(addr)),
        DeriveEnvironment::Mainnet,
        TransportBackend::default(),
        None,
        test_credentials(),
        None,
        None,
    );
    let execution = client.execution_handle();
    client.connect().await.expect("connect failed");
    client
        .subscribe_ticker("ETH-PERP", "1000")
        .await
        .expect("initial subscribe failed");

    tokio::time::timeout(Duration::from_secs(8), async {
        loop {
            match client.next_event().await {
                Some(DeriveWsMessage::Reconnected)
                    if client.is_authenticated()
                        && state.subscribe_frames.lock().await.len() >= 3 =>
                {
                    break;
                }
                Some(_) => {}
                None => panic!("event stream closed before repeated recovery completed"),
            }
        }
    })
    .await
    .expect("post-ack connection loss was not recovered");
    execution
        .cancel_all_orders(&DeriveCancelAllParams::new(30769))
        .await
        .expect("private request should succeed after repeated reconnect");

    assert!(client.is_active());
    assert!(client.is_authenticated());
    assert_eq!(client.subscription_count(), 1);
    assert_eq!(state.login_frames.lock().await.len(), 3);
    assert_eq!(state.subscribe_frames.lock().await.len(), 3);

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_reconnect_does_not_replay_acknowledged_unsubscribe() {
    let state = ServerState::new();
    state
        .disconnect_after_subscribe
        .store(true, Ordering::SeqCst);
    state
        .subscribe_timeouts_after_first
        .store(1, Ordering::SeqCst);
    let addr = start_server(state.clone()).await;

    let mut client = DeriveWebSocketClient::with_credentials(
        Some(ws_url(addr)),
        DeriveEnvironment::Mainnet,
        TransportBackend::default(),
        None,
        test_credentials(),
        None,
        None,
    );
    client.set_request_timeout(Duration::from_millis(100));
    let subscriptions = client.subscription_handle();
    client.connect().await.expect("connect failed");
    client
        .subscribe_ticker("ETH-PERP", "1000")
        .await
        .expect("initial subscribe failed");

    wait_until_async(
        || {
            let state = state.clone();
            async move { state.subscribe_frames.lock().await.len() >= 2 }
        },
        Duration::from_secs(5),
    )
    .await;
    let unsubscribe = {
        let pending = async move { subscriptions.unsubscribe_ticker("ETH-PERP", "1000").await };
        tokio::spawn(pending)
    };
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(
        !unsubscribe.is_finished(),
        "unsubscribe must wait for the recovery transaction",
    );

    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            match client.next_event().await {
                Some(DeriveWsMessage::Reconnected) => break,
                Some(_) => {}
                None => panic!("event stream closed before recovery completed"),
            }
        }
    })
    .await
    .expect("unsubscribe recovery timed out");
    unsubscribe
        .await
        .expect("unsubscribe task failed")
        .expect("concurrent unsubscribe failed");

    assert!(client.is_active());
    assert!(client.is_authenticated());
    assert_eq!(client.subscription_count(), 0);
    assert_eq!(state.login_frames.lock().await.len(), 3);
    assert_eq!(state.subscribe_frames.lock().await.len(), 3);
    assert_eq!(state.unsubscribe_frames.lock().await.len(), 1);

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_subscribe_waits_for_recovery_after_second_connection_loss() {
    let state = ServerState::new();
    state
        .subscribe_timeouts_after_first
        .store(1, Ordering::SeqCst);
    state
        .disconnect_after_subscribe
        .store(true, Ordering::SeqCst);
    let addr = start_server(state.clone()).await;

    let mut client = DeriveWebSocketClient::new(
        Some(ws_url(addr)),
        DeriveEnvironment::Mainnet,
        TransportBackend::default(),
        None,
    );
    client.set_request_timeout(Duration::from_secs(2));
    let subscriptions = client.subscription_handle();
    client.connect().await.expect("connect failed");
    client
        .subscribe_ticker("ETH-PERP", "1000")
        .await
        .expect("initial subscribe failed");

    wait_until_async(
        || {
            let state = state.clone();
            async move { state.subscribe_frames.lock().await.len() >= 2 }
        },
        Duration::from_secs(5),
    )
    .await;
    state
        .disconnect_after_subscribe
        .store(true, Ordering::SeqCst);
    subscriptions
        .subscribe_orderbook("ETH-PERP", "1", "10")
        .await
        .expect("concurrent subscribe failed");

    tokio::time::timeout(Duration::from_secs(8), async {
        loop {
            match client.next_event().await {
                Some(DeriveWsMessage::Reconnected) => break,
                Some(_) => {}
                None => panic!("event stream closed before reconnect completed"),
            }
        }
    })
    .await
    .expect("second-loss recovery timed out");

    let frames = state.captured_subscribes().await;
    let replayed_channels = frames[3]["params"]["channels"]
        .as_array()
        .expect("final recovery frame should contain channels");
    let subscribed_channels = frames[4]["params"]["channels"]
        .as_array()
        .expect("post-recovery subscribe frame should contain channels");

    assert!(client.is_active());
    assert_eq!(client.subscription_count(), 2);
    assert_eq!(frames.len(), 5);
    assert_eq!(replayed_channels, &vec![json!("ticker_slim.ETH-PERP.1000")],);
    assert_eq!(subscribed_channels, &vec![json!("orderbook.ETH-PERP.1.10")],);

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_reconnect_does_not_replay_pending_private_request() {
    let state = ServerState::new();
    state
        .disconnect_before_private_reply
        .store(true, Ordering::SeqCst);
    let addr = start_server(state.clone()).await;

    let mut client = DeriveWebSocketClient::with_credentials(
        Some(ws_url(addr)),
        DeriveEnvironment::Mainnet,
        TransportBackend::default(),
        None,
        test_credentials(),
        None,
        None,
    );
    let execution = client.execution_handle();
    client.connect().await.expect("connect failed");

    let first_error = execution
        .cancel_by_instrument(&DeriveCancelByInstrumentParams::new(30769, "ETH-PERP"))
        .await
        .expect_err("request interrupted by reconnect must fail");
    assert!(matches!(
        first_error,
        DeriveWsError::Transport(_) | DeriveWsError::RequestCancelled { .. }
    ));

    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            match client.next_event().await {
                Some(DeriveWsMessage::Reconnected) => break,
                Some(_) => {}
                None => panic!("event stream closed before reconnect completed"),
            }
        }
    })
    .await
    .expect("reconnect completion timed out");

    let private_frames = state.private_frames.lock().await;
    assert_eq!(
        private_frames.len(),
        1,
        "the interrupted private request must not replay on the replacement connection",
    );
    assert_eq!(
        private_frames[0]["method"].as_str(),
        Some("private/cancel_by_instrument"),
    );
    drop(private_frames);
    execution
        .cancel_by_instrument(&DeriveCancelByInstrumentParams::new(30769, "ETH-PERP"))
        .await
        .expect("new private request should succeed after recovery");
    assert_eq!(state.private_frames.lock().await.len(), 2);

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_reconnect_surfaces_exhausted_subscription_retries() {
    let state = ServerState::new();
    state
        .subscribe_failures_after_first
        .store(10, Ordering::SeqCst);
    state
        .disconnect_after_subscribe
        .store(true, Ordering::SeqCst);
    let addr = start_server(state.clone()).await;

    let mut client = DeriveWebSocketClient::with_credentials(
        Some(ws_url(addr)),
        DeriveEnvironment::Mainnet,
        TransportBackend::default(),
        None,
        test_credentials(),
        None,
        None,
    );
    let execution = client.execution_handle();
    client.connect().await.expect("connect failed");
    client
        .subscribe_ticker("ETH-PERP", "1000")
        .await
        .expect("initial subscribe failed");

    let reason = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            match client.next_event().await {
                Some(DeriveWsMessage::SessionRecoveryFailed(reason)) => break reason,
                Some(_) => {}
                None => panic!("event stream closed before recovery failure was surfaced"),
            }
        }
    })
    .await
    .expect("recovery failure timed out");
    let error = execution
        .cancel_all_orders(&DeriveCancelAllParams::new(30769))
        .await
        .expect_err("private request must fail after rejected resubscribe");

    assert!(reason.contains("subscribe denied"));
    assert!(matches!(error, DeriveWsError::Authentication { .. }));
    assert_eq!(state.login_frames.lock().await.len(), 4);
    assert_eq!(state.subscribe_frames.lock().await.len(), 4);
    wait_for_inactive(&client, Duration::from_secs(2)).await;
    assert!(!client.is_active());
    assert!(!client.is_authenticated());
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
