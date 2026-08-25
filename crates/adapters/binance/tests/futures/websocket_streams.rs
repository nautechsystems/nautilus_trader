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

//! Integration tests for the Binance Futures WebSocket client using a mock server.

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
use nautilus_binance::{
    common::{
        consts::{BINANCE_CLIENT_ID, BINANCE_VENUE},
        enums::{BinanceEnvironment, BinanceProductType},
    },
    futures::websocket::streams::client::BinanceFuturesWebSocketClient,
};
use nautilus_common::{
    live::runner::replace_system_event_sender,
    messages::{SystemEvent, system::SocketState},
    testing::wait_until_async,
};
use nautilus_live::{SocketControlFactory, SocketReconnectRegistry, SocketReconnectRequestOutcome};
use nautilus_network::websocket::TransportBackend;
use rstest::rstest;
use serde_json::json;

#[derive(Clone)]
struct TestServerState {
    connection_count: Arc<tokio::sync::Mutex<usize>>,
    total_connections: Arc<AtomicUsize>,
    subscribed_streams: Arc<tokio::sync::Mutex<Vec<String>>>,
    received_messages: Arc<tokio::sync::Mutex<Vec<serde_json::Value>>>,
    disconnect_trigger: Arc<AtomicBool>,
    drop_next_connection: Arc<AtomicBool>,
    fail_next_subscriptions: Arc<tokio::sync::Mutex<Vec<String>>>,
    delay_next_subscription_response: Arc<AtomicBool>,
    ping_count: Arc<AtomicUsize>,
}

impl Default for TestServerState {
    fn default() -> Self {
        Self {
            connection_count: Arc::new(tokio::sync::Mutex::new(0)),
            total_connections: Arc::new(AtomicUsize::new(0)),
            subscribed_streams: Arc::new(tokio::sync::Mutex::new(Vec::new())),
            received_messages: Arc::new(tokio::sync::Mutex::new(Vec::new())),
            disconnect_trigger: Arc::new(AtomicBool::new(false)),
            drop_next_connection: Arc::new(AtomicBool::new(false)),
            fail_next_subscriptions: Arc::new(tokio::sync::Mutex::new(Vec::new())),
            delay_next_subscription_response: Arc::new(AtomicBool::new(false)),
            ping_count: Arc::new(AtomicUsize::new(0)),
        }
    }
}

impl TestServerState {
    async fn subscribed_streams(&self) -> Vec<String> {
        self.subscribed_streams.lock().await.clone()
    }

    async fn received_messages(&self) -> Vec<serde_json::Value> {
        self.received_messages.lock().await.clone()
    }

    fn total_connections(&self) -> usize {
        self.total_connections.load(Ordering::Relaxed)
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
    state.total_connections.fetch_add(1, Ordering::Relaxed);
    let mut delayed_subscription_response = None;

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

                state.received_messages.lock().await.push(value.clone());

                let method = value.get("method").and_then(|v| v.as_str());
                let id = value.get("id").and_then(|v| v.as_u64()).unwrap_or(0);

                match method {
                    Some("SUBSCRIBE") => {
                        let params = value
                            .get("params")
                            .and_then(|v| v.as_array())
                            .map(|arr| {
                                arr.iter()
                                    .filter_map(|v| v.as_str().map(String::from))
                                    .collect::<Vec<_>>()
                            })
                            .unwrap_or_default();

                        {
                            let mut fail_list = state.fail_next_subscriptions.lock().await;
                            if !fail_list.is_empty() {
                                fail_list.clear();

                                let error_response = json!({
                                    "code": -1,
                                    "msg": "Forced subscription failure",
                                    "id": id
                                });
                                let _result = socket
                                    .send(Message::Text(error_response.to_string().into()))
                                    .await;
                                break;
                            }
                        }

                        state.subscribed_streams.lock().await.extend(params);

                        let response = json!({
                            "result": null,
                            "id": id
                        });

                        if state
                            .delay_next_subscription_response
                            .swap(false, Ordering::Relaxed)
                        {
                            delayed_subscription_response = Some(response);
                        } else if socket
                            .send(Message::Text(response.to_string().into()))
                            .await
                            .is_err()
                        {
                            break;
                        }

                        if state.drop_next_connection.swap(false, Ordering::Relaxed) {
                            break;
                        }
                    }
                    Some("UNSUBSCRIBE") => {
                        let params = value
                            .get("params")
                            .and_then(|v| v.as_array())
                            .map(|arr| {
                                arr.iter()
                                    .filter_map(|v| v.as_str().map(String::from))
                                    .collect::<Vec<_>>()
                            })
                            .unwrap_or_default();

                        let mut streams = state.subscribed_streams.lock().await;
                        streams.retain(|s| !params.contains(s));

                        let response = json!({
                            "result": null,
                            "id": id
                        });

                        if socket
                            .send(Message::Text(response.to_string().into()))
                            .await
                            .is_err()
                        {
                            break;
                        }

                        if let Some(delayed_response) = delayed_subscription_response.take()
                            && socket
                                .send(Message::Text(delayed_response.to_string().into()))
                                .await
                                .is_err()
                        {
                            break;
                        }
                    }
                    Some("LIST_SUBSCRIPTIONS") => {
                        let streams = state.subscribed_streams.lock().await.clone();
                        let response = json!({
                            "result": streams,
                            "id": id
                        });

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
            Message::Pong(_) => {}
            Message::Close(_) => {
                break;
            }
            _ => {}
        }

        if state.disconnect_trigger.load(Ordering::Relaxed) {
            break;
        }
    }

    let mut count = state.connection_count.lock().await;
    *count = count.saturating_sub(1);
}

fn create_test_router(state: TestServerState) -> Router {
    Router::new()
        .route("/ws", get(handle_websocket))
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

    tokio::time::sleep(Duration::from_millis(100)).await;
    Ok((addr, state))
}

fn create_test_client(addr: &SocketAddr) -> BinanceFuturesWebSocketClient {
    let ws_url = format!("ws://{addr}/ws");
    BinanceFuturesWebSocketClient::new(
        BinanceProductType::UsdM,
        BinanceEnvironment::Live,
        None,
        None,
        Some(ws_url),
        None,
        TransportBackend::default(),
    )
    .unwrap()
}

#[rstest]
fn test_client_accepts_demo_environment() {
    let result = BinanceFuturesWebSocketClient::new(
        BinanceProductType::UsdM,
        BinanceEnvironment::Demo,
        None,
        None,
        Some("ws://127.0.0.1:1/ws".to_string()),
        None,
        TransportBackend::default(),
    );

    assert!(result.is_ok());
}

#[rstest]
fn test_client_debug_redacts_url() {
    let client = BinanceFuturesWebSocketClient::new(
        BinanceProductType::CoinM,
        BinanceEnvironment::Testnet,
        None,
        None,
        Some("wss://dstream.binancefuture.com/ws/redacted".to_string()),
        None,
        TransportBackend::default(),
    )
    .unwrap();

    let output = format!("{client:?}");

    assert!(output.contains("url: \"<redacted>\""));
    assert!(!output.contains("dstream.binancefuture.com"));
}

#[rstest]
#[tokio::test]
async fn test_client_connection() {
    let (addr, state) = start_test_server().await.unwrap();
    let mut client = create_test_client(&addr);

    client.connect().await.unwrap();

    wait_until_async(
        || async { *state.connection_count.lock().await > 0 },
        Duration::from_secs(5),
    )
    .await;

    assert!(client.is_active());
    assert_eq!(*state.connection_count.lock().await, 1);

    client.close().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_client_disconnect() {
    let (addr, state) = start_test_server().await.unwrap();
    let mut client = create_test_client(&addr);

    client.connect().await.unwrap();

    wait_until_async(
        || async { *state.connection_count.lock().await > 0 },
        Duration::from_secs(5),
    )
    .await;

    assert!(client.is_active());

    client.close().await.unwrap();

    tokio::time::sleep(Duration::from_millis(100)).await;
}

#[rstest]
#[tokio::test]
async fn test_subscribe_single_stream() {
    let (addr, state) = start_test_server().await.unwrap();
    let mut client = create_test_client(&addr);

    client.connect().await.unwrap();

    wait_until_async(
        || async { *state.connection_count.lock().await > 0 },
        Duration::from_secs(5),
    )
    .await;

    client
        .subscribe(vec!["btcusdt@aggTrade".to_string()])
        .await
        .unwrap();

    wait_until_async(
        || async { !state.subscribed_streams().await.is_empty() },
        Duration::from_secs(5),
    )
    .await;

    let streams = state.subscribed_streams().await;
    assert!(streams.contains(&"btcusdt@aggTrade".to_string()));

    client.close().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_subscribe_multiple_streams() {
    let (addr, state) = start_test_server().await.unwrap();
    let mut client = create_test_client(&addr);

    client.connect().await.unwrap();

    wait_until_async(
        || async { *state.connection_count.lock().await > 0 },
        Duration::from_secs(5),
    )
    .await;

    let streams_to_subscribe = vec![
        "btcusdt@aggTrade".to_string(),
        "ethusdt@aggTrade".to_string(),
        "btcusdt@depth@100ms".to_string(),
    ];

    client
        .subscribe(streams_to_subscribe.clone())
        .await
        .unwrap();

    wait_until_async(
        || async { state.subscribed_streams().await.len() >= 3 },
        Duration::from_secs(5),
    )
    .await;

    let streams = state.subscribed_streams().await;
    assert!(streams.contains(&"btcusdt@aggTrade".to_string()));
    assert!(streams.contains(&"ethusdt@aggTrade".to_string()));
    assert!(streams.contains(&"btcusdt@depth@100ms".to_string()));

    client.close().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_unsubscribe_stream() {
    let (addr, state) = start_test_server().await.unwrap();
    let mut client = create_test_client(&addr);

    client.connect().await.unwrap();

    wait_until_async(
        || async { *state.connection_count.lock().await > 0 },
        Duration::from_secs(5),
    )
    .await;

    client
        .subscribe(vec![
            "btcusdt@aggTrade".to_string(),
            "ethusdt@aggTrade".to_string(),
        ])
        .await
        .unwrap();

    wait_until_async(
        || async { state.subscribed_streams().await.len() >= 2 },
        Duration::from_secs(5),
    )
    .await;

    client
        .unsubscribe(vec!["btcusdt@aggTrade".to_string()])
        .await
        .unwrap();

    wait_until_async(
        || async {
            let streams = state.subscribed_streams().await;
            !streams.contains(&"btcusdt@aggTrade".to_string())
        },
        Duration::from_secs(5),
    )
    .await;

    let streams = state.subscribed_streams().await;
    assert!(!streams.contains(&"btcusdt@aggTrade".to_string()));
    assert!(streams.contains(&"ethusdt@aggTrade".to_string()));

    client.close().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_delayed_subscribe_response_does_not_restore_unsubscribed_stream() {
    let (addr, state) = start_test_server().await.unwrap();
    let mut client = create_test_client(&addr);

    client.connect().await.unwrap();
    wait_until_async(
        || async { *state.connection_count.lock().await > 0 },
        Duration::from_secs(5),
    )
    .await;

    state
        .delay_next_subscription_response
        .store(true, Ordering::Relaxed);

    client
        .subscribe(vec!["btcusdt@aggTrade".to_string()])
        .await
        .unwrap();
    wait_until_async(
        || async { state.received_messages().await.len() == 1 },
        Duration::from_secs(5),
    )
    .await;

    client
        .unsubscribe(vec!["btcusdt@aggTrade".to_string()])
        .await
        .unwrap();
    client
        .subscribe(vec!["ethusdt@aggTrade".to_string()])
        .await
        .unwrap();

    wait_until_async(
        || async { state.received_messages().await.len() == 3 },
        Duration::from_secs(5),
    )
    .await;
    wait_until_async(
        || async { client.subscription_count() == 1 },
        Duration::from_secs(5),
    )
    .await;
    tokio::time::sleep(Duration::from_millis(100)).await;

    let messages = state.received_messages().await;
    let methods: Vec<_> = messages
        .iter()
        .map(|message| message["method"].as_str().unwrap())
        .collect();
    assert_eq!(methods, ["SUBSCRIBE", "UNSUBSCRIBE", "SUBSCRIBE"]);
    assert_eq!(state.subscribed_streams().await, ["ethusdt@aggTrade"]);
    assert_eq!(client.subscription_count(), 1);

    client.close().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_subscription_count() {
    let (addr, state) = start_test_server().await.unwrap();
    let mut client = create_test_client(&addr);

    client.connect().await.unwrap();

    wait_until_async(
        || async { *state.connection_count.lock().await > 0 },
        Duration::from_secs(5),
    )
    .await;

    assert_eq!(client.subscription_count(), 0);

    client
        .subscribe(vec![
            "btcusdt@aggTrade".to_string(),
            "ethusdt@aggTrade".to_string(),
        ])
        .await
        .unwrap();

    wait_until_async(
        || async { !state.received_messages().await.is_empty() },
        Duration::from_secs(5),
    )
    .await;

    let messages = state.received_messages().await;
    assert!(!messages.is_empty());

    client.close().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_is_active_false_before_connect() {
    let (addr, _state) = start_test_server().await.unwrap();
    let client = create_test_client(&addr);

    assert!(!client.is_active());
    assert!(client.is_closed());
}

#[rstest]
#[tokio::test]
async fn test_subscribe_request_format() {
    let (addr, state) = start_test_server().await.unwrap();
    let mut client = create_test_client(&addr);

    client.connect().await.unwrap();

    wait_until_async(
        || async { *state.connection_count.lock().await > 0 },
        Duration::from_secs(5),
    )
    .await;

    client
        .subscribe(vec!["btcusdt@aggTrade".to_string()])
        .await
        .unwrap();

    wait_until_async(
        || async { !state.received_messages().await.is_empty() },
        Duration::from_secs(5),
    )
    .await;

    let messages = state.received_messages().await;
    assert!(!messages.is_empty());

    let subscribe_msg = &messages[0];
    assert_eq!(
        subscribe_msg.get("method").and_then(|v| v.as_str()),
        Some("SUBSCRIBE")
    );
    assert!(subscribe_msg.get("id").is_some());
    assert!(subscribe_msg.get("params").is_some());

    let params = subscribe_msg.get("params").and_then(|v| v.as_array());
    assert!(params.is_some());
    let params = params.unwrap();
    assert!(
        params
            .iter()
            .any(|v| v.as_str() == Some("btcusdt@aggTrade"))
    );

    client.close().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_unsubscribe_request_format() {
    let (addr, state) = start_test_server().await.unwrap();
    let mut client = create_test_client(&addr);

    client.connect().await.unwrap();

    wait_until_async(
        || async { *state.connection_count.lock().await > 0 },
        Duration::from_secs(5),
    )
    .await;

    client
        .subscribe(vec!["btcusdt@aggTrade".to_string()])
        .await
        .unwrap();

    wait_until_async(
        || async { !state.received_messages().await.is_empty() },
        Duration::from_secs(5),
    )
    .await;

    client
        .unsubscribe(vec!["btcusdt@aggTrade".to_string()])
        .await
        .unwrap();

    wait_until_async(
        || async { state.received_messages().await.len() >= 2 },
        Duration::from_secs(5),
    )
    .await;

    let messages = state.received_messages().await;
    let unsubscribe_msg = &messages[1];

    assert_eq!(
        unsubscribe_msg.get("method").and_then(|v| v.as_str()),
        Some("UNSUBSCRIBE")
    );
    assert!(unsubscribe_msg.get("id").is_some());

    client.close().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_connection_failure_invalid_url() {
    let result = BinanceFuturesWebSocketClient::new(
        BinanceProductType::UsdM,
        BinanceEnvironment::Live,
        None,
        None,
        Some("ws://127.0.0.1:9999/invalid".to_string()),
        None,
        TransportBackend::default(),
    );

    let mut client = result.unwrap();

    let connect_result = client.connect().await;
    connect_result.unwrap_err();
}

#[rstest]
#[tokio::test]
async fn test_default_client_creation_usdm() {
    let client = BinanceFuturesWebSocketClient::new(
        BinanceProductType::UsdM,
        BinanceEnvironment::Live,
        None,
        None,
        None,
        None,
        TransportBackend::default(),
    )
    .unwrap();

    assert!(!client.is_active());
    assert!(client.is_closed());
    assert_eq!(client.product_type(), BinanceProductType::UsdM);
}

#[rstest]
#[tokio::test]
async fn test_default_client_creation_coinm() {
    let client = BinanceFuturesWebSocketClient::new(
        BinanceProductType::CoinM,
        BinanceEnvironment::Live,
        None,
        None,
        None,
        None,
        TransportBackend::default(),
    )
    .unwrap();

    assert!(!client.is_active());
    assert!(client.is_closed());
    assert_eq!(client.product_type(), BinanceProductType::CoinM);
}

#[rstest]
#[tokio::test]
async fn test_invalid_product_type_rejected() {
    let result = BinanceFuturesWebSocketClient::new(
        BinanceProductType::Spot,
        BinanceEnvironment::Live,
        None,
        None,
        None,
        None,
        TransportBackend::default(),
    );

    let err_msg = result.unwrap_err().to_string();
    assert!(err_msg.contains("UsdM or CoinM"));
}

#[rstest]
#[tokio::test]
async fn test_pool_creates_second_connection_on_overflow() {
    let (addr, state) = start_test_server().await.unwrap();
    let registry = SocketReconnectRegistry::default();
    let endpoint = "binance-futures-pool-streams";
    let factory =
        SocketControlFactory::with_registry(*BINANCE_CLIENT_ID, Some(*BINANCE_VENUE), &registry);
    let mut client = create_test_client(&addr).with_socket_control(factory, endpoint);

    client.connect().await.unwrap();

    wait_until_async(
        || async { *state.connection_count.lock().await > 0 },
        Duration::from_secs(5),
    )
    .await;

    // 201 streams exceeds the 200-per-connection limit, so the pool
    // should create a second connection automatically
    let streams: Vec<String> = (0..201).map(|i| format!("stream{i}@aggTrade")).collect();

    let result = client.subscribe(streams).await;
    result.unwrap();

    wait_until_async(
        || async { *state.connection_count.lock().await >= 2 },
        Duration::from_secs(5),
    )
    .await;

    let primary = registry
        .handle(*BINANCE_CLIENT_ID, ustr::Ustr::from(endpoint))
        .unwrap();
    let secondary = registry
        .handle(
            *BINANCE_CLIENT_ID,
            ustr::Ustr::from("binance-futures-pool-streams-1"),
        )
        .unwrap();

    assert_eq!(*state.connection_count.lock().await, 2);
    assert_eq!(
        primary.request_reconnect(),
        SocketReconnectRequestOutcome::Accepted
    );
    assert_eq!(
        secondary.request_reconnect(),
        SocketReconnectRequestOutcome::Accepted
    );

    client.close().await.unwrap();
    assert!(
        registry
            .handle(*BINANCE_CLIENT_ID, ustr::Ustr::from(endpoint))
            .is_none()
    );
    assert!(
        registry
            .handle(
                *BINANCE_CLIENT_ID,
                ustr::Ustr::from("binance-futures-pool-streams-1"),
            )
            .is_none()
    );
}

#[rstest]
#[tokio::test]
async fn test_pool_streams_distributed_across_slots() {
    let (addr, state) = start_test_server().await.unwrap();
    let mut client = create_test_client(&addr);

    client.connect().await.unwrap();

    wait_until_async(
        || async { *state.connection_count.lock().await > 0 },
        Duration::from_secs(5),
    )
    .await;

    // Subscribe 150 streams (fits in slot 0)
    let batch1: Vec<String> = (0..150).map(|i| format!("sym{i}@aggTrade")).collect();
    client.subscribe(batch1).await.unwrap();

    // Subscribe another 100 (50 fit in slot 0, 50 overflow to slot 1)
    let batch2: Vec<String> = (150..250).map(|i| format!("sym{i}@aggTrade")).collect();
    client.subscribe(batch2).await.unwrap();

    wait_until_async(
        || async { *state.connection_count.lock().await >= 2 },
        Duration::from_secs(5),
    )
    .await;

    assert_eq!(*state.connection_count.lock().await, 2);

    // All 250 streams should be subscribed across the two connections
    wait_until_async(
        || async { state.subscribed_streams().await.len() >= 250 },
        Duration::from_secs(5),
    )
    .await;

    client.close().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_pool_duplicate_subscribe_ignored() {
    let (addr, state) = start_test_server().await.unwrap();
    let mut client = create_test_client(&addr);

    client.connect().await.unwrap();

    wait_until_async(
        || async { *state.connection_count.lock().await > 0 },
        Duration::from_secs(5),
    )
    .await;

    let streams = vec!["btcusdt@aggTrade".to_string()];
    client.subscribe(streams.clone()).await.unwrap();

    wait_until_async(
        || async { !state.subscribed_streams().await.is_empty() },
        Duration::from_secs(5),
    )
    .await;

    // Subscribing the same stream again should be a no-op
    client.subscribe(streams).await.unwrap();

    // Still only one connection
    assert_eq!(*state.connection_count.lock().await, 1);

    client.close().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_pool_unsubscribe_frees_capacity() {
    let (addr, state) = start_test_server().await.unwrap();
    let mut client = create_test_client(&addr);

    client.connect().await.unwrap();

    wait_until_async(
        || async { *state.connection_count.lock().await > 0 },
        Duration::from_secs(5),
    )
    .await;

    // Fill slot 0 to exactly 200 streams
    let streams: Vec<String> = (0..200).map(|i| format!("sym{i}@aggTrade")).collect();
    client.subscribe(streams).await.unwrap();

    wait_until_async(
        || async { state.subscribed_streams().await.len() >= 200 },
        Duration::from_secs(5),
    )
    .await;

    // Unsubscribe 10 streams from slot 0
    let unsub: Vec<String> = (0..10).map(|i| format!("sym{i}@aggTrade")).collect();
    client.unsubscribe(unsub).await.unwrap();

    // Now subscribing 10 new streams should fit in slot 0 (no new connection)
    let new_streams: Vec<String> = (200..210).map(|i| format!("sym{i}@aggTrade")).collect();
    client.subscribe(new_streams).await.unwrap();

    // Should still be just 1 connection
    assert_eq!(*state.connection_count.lock().await, 1);

    client.close().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_pool_single_batch_under_limit_uses_one_connection() {
    let (addr, state) = start_test_server().await.unwrap();
    let mut client = create_test_client(&addr);

    client.connect().await.unwrap();

    wait_until_async(
        || async { *state.connection_count.lock().await > 0 },
        Duration::from_secs(5),
    )
    .await;

    // 200 streams exactly fits in one connection
    let streams: Vec<String> = (0..200).map(|i| format!("sym{i}@aggTrade")).collect();
    client.subscribe(streams).await.unwrap();

    wait_until_async(
        || async { state.subscribed_streams().await.len() >= 200 },
        Duration::from_secs(5),
    )
    .await;

    assert_eq!(*state.connection_count.lock().await, 1);

    client.close().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_subscribe_futures_specific_streams() {
    let (addr, state) = start_test_server().await.unwrap();
    let mut client = create_test_client(&addr);

    client.connect().await.unwrap();

    wait_until_async(
        || async { *state.connection_count.lock().await > 0 },
        Duration::from_secs(5),
    )
    .await;

    let streams = vec![
        "btcusdt@markPrice".to_string(),
        "btcusdt@kline_1m".to_string(),
        "btcusdt@bookTicker".to_string(),
    ];

    client.subscribe(streams.clone()).await.unwrap();

    wait_until_async(
        || async { state.subscribed_streams().await.len() >= 3 },
        Duration::from_secs(5),
    )
    .await;

    let subscribed = state.subscribed_streams().await;
    assert!(subscribed.contains(&"btcusdt@markPrice".to_string()));
    assert!(subscribed.contains(&"btcusdt@kline_1m".to_string()));
    assert!(subscribed.contains(&"btcusdt@bookTicker".to_string()));

    client.close().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_reconnection_after_server_drop() {
    let (addr, state) = start_test_server().await.unwrap();
    let (system_tx, mut system_rx) = tokio::sync::mpsc::unbounded_channel();
    replace_system_event_sender(system_tx);
    let registry = SocketReconnectRegistry::default();
    let endpoint = ustr::Ustr::from("binance-futures-test-streams");
    let factory =
        SocketControlFactory::with_registry(*BINANCE_CLIENT_ID, Some(*BINANCE_VENUE), &registry);

    let mut client = create_test_client(&addr).with_socket_control(factory, endpoint.as_str());

    client.connect().await.unwrap();

    wait_until_async(
        || async { *state.connection_count.lock().await > 0 },
        Duration::from_secs(5),
    )
    .await;

    client
        .subscribe(vec!["btcusdt@aggTrade".to_string()])
        .await
        .unwrap();

    wait_until_async(
        || async { !state.subscribed_streams().await.is_empty() },
        Duration::from_secs(5),
    )
    .await;

    let initial_total = state.total_connections();

    // Drop the connection after the next subscribe
    state.drop_next_connection.store(true, Ordering::Relaxed);
    let _result = client.subscribe(vec!["ethusdt@aggTrade".to_string()]).await;

    // Client should reconnect (total connections increases)
    wait_until_async(
        || async { state.total_connections() > initial_total },
        Duration::from_secs(10),
    )
    .await;

    assert!(
        state.total_connections() > initial_total,
        "Expected at least one reconnection"
    );
    let mut socket_states = Vec::new();
    wait_until_async(
        || {
            while let Ok(event) = system_rx.try_recv() {
                let SystemEvent::SocketState(change) = event;
                assert_eq!(change.client_id, *BINANCE_CLIENT_ID);
                assert_eq!(change.venue, Some(*BINANCE_VENUE));
                assert_eq!(change.endpoint, endpoint);
                socket_states.push(change.state);
            }
            let done = socket_states.len() == 3;
            async move { done }
        },
        Duration::from_secs(5),
    )
    .await;
    assert_eq!(
        socket_states,
        vec![
            SocketState::Connected,
            SocketState::Disconnected,
            SocketState::Connected,
        ]
    );
    assert!(registry.handle(*BINANCE_CLIENT_ID, endpoint).is_some());

    client.close().await.unwrap();
    assert!(system_rx.try_recv().is_err());
    assert!(registry.handle(*BINANCE_CLIENT_ID, endpoint).is_none());
}

#[rstest]
#[tokio::test]
async fn test_is_active_lifecycle() {
    let (addr, _state) = start_test_server().await.unwrap();
    let mut client = create_test_client(&addr);

    assert!(!client.is_active(), "Should not be active before connect");
    assert!(client.is_closed(), "Should be closed before connect");

    client.connect().await.unwrap();

    wait_until_async(|| async { client.is_active() }, Duration::from_secs(5)).await;

    assert!(client.is_active(), "Should be active after connect");
    assert!(!client.is_closed(), "Should not be closed after connect");

    client.close().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_is_active_false_during_reconnection() {
    let (addr, state) = start_test_server().await.unwrap();
    let mut client = create_test_client(&addr);

    client.connect().await.unwrap();

    wait_until_async(|| async { client.is_active() }, Duration::from_secs(5)).await;

    // Subscribe to establish baseline
    client
        .subscribe(vec!["btcusdt@aggTrade".to_string()])
        .await
        .unwrap();

    wait_until_async(
        || async { !state.subscribed_streams().await.is_empty() },
        Duration::from_secs(5),
    )
    .await;

    // Trigger disconnect after next subscription
    state.drop_next_connection.store(true, Ordering::Relaxed);
    let _result = client.subscribe(vec!["ethusdt@aggTrade".to_string()]).await;

    // Client should become inactive during reconnection
    wait_until_async(|| async { !client.is_active() }, Duration::from_secs(5)).await;

    // Then become active again after reconnection
    wait_until_async(|| async { client.is_active() }, Duration::from_secs(10)).await;

    assert!(client.is_active(), "Should be active after reconnection");

    client.close().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_rapid_consecutive_reconnections() {
    let (addr, state) = start_test_server().await.unwrap();
    let mut client = create_test_client(&addr);

    client.connect().await.unwrap();

    wait_until_async(|| async { client.is_active() }, Duration::from_secs(5)).await;

    let initial_total = state.total_connections();

    // Trigger 3 rapid reconnection cycles
    for i in 0..3 {
        state.drop_next_connection.store(true, Ordering::Relaxed);
        let _result = client.subscribe(vec![format!("stream{i}@aggTrade")]).await;

        let expected = initial_total + i + 1;
        wait_until_async(
            || async { state.total_connections() >= expected },
            Duration::from_secs(10),
        )
        .await;

        // Wait for client to become active again before next cycle
        wait_until_async(|| async { client.is_active() }, Duration::from_secs(10)).await;
    }

    assert!(
        state.total_connections() >= initial_total + 3,
        "Expected at least 3 reconnections, total={}",
        state.total_connections()
    );

    client.close().await.unwrap();
}
