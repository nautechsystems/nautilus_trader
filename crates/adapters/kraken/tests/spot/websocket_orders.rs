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

//! Integration tests for Kraken Spot WebSocket order submission using a mock server.
//!
//! These tests verify the WS-trade routing path by:
//! 1. Spinning up an in-process axum WebSocket server on a random port.
//! 2. Connecting a [`KrakenSpotWebSocketClient`] to it.
//! 3. Exercising [`OrderRequestState::submit`] and verifying that the
//!    `add_order` JSON envelope arrives at the mock server.
//! 4. Verifying that when the WS connection is not active no message is
//!    forwarded to the mock server.

use std::{net::SocketAddr, sync::Arc, time::Duration};

use axum::{
    Router,
    extract::ws::{Message, WebSocket, WebSocketUpgrade},
    response::IntoResponse,
    routing::get,
};
use futures_util::StreamExt;
use nautilus_common::testing::wait_until_async;
use nautilus_kraken::{
    KrakenDataClientConfig, KrakenSpotWebSocketClient,
    common::enums::{KrakenOrderSide, KrakenOrderType},
    websocket::{
        dispatch::{
            WsDispatchState,
            spot_orders::{OrderRequestState, PendingOperation, PendingRequest},
        },
        spot_v2::messages::KrakenWsAddOrderParams,
    },
};
use nautilus_model::identifiers::{AccountId, ClientOrderId, TraderId};
use rstest::rstest;
use tokio_util::sync::CancellationToken;

/// Shared state for the mock WebSocket server.
#[derive(Clone, Default)]
struct MockServerState {
    received_messages: Arc<tokio::sync::Mutex<Vec<serde_json::Value>>>,
    connection_count: Arc<tokio::sync::Mutex<usize>>,
}

impl MockServerState {
    async fn messages(&self) -> Vec<serde_json::Value> {
        self.received_messages.lock().await.clone()
    }
}

async fn ws_handler(ws: WebSocketUpgrade, state: Arc<MockServerState>) -> impl IntoResponse {
    ws.on_upgrade(|socket| handle_socket(socket, state))
}

async fn handle_socket(socket: WebSocket, state: Arc<MockServerState>) {
    *state.connection_count.lock().await += 1;

    let (_sender, mut receiver) = socket.split();

    while let Some(Ok(msg)) = receiver.next().await {
        match msg {
            Message::Text(text) => {
                if let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) {
                    state.received_messages.lock().await.push(value);
                }
            }
            Message::Close(_) => break,
            _ => {}
        }
    }
}

/// Starts a mock WebSocket server and returns its address and shared state.
async fn start_mock_server() -> (SocketAddr, Arc<MockServerState>) {
    let state = Arc::new(MockServerState::default());
    let state_clone = Arc::clone(&state);

    let app = Router::new().route(
        "/v2",
        get(move |ws| ws_handler(ws, Arc::clone(&state_clone))),
    );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    tokio::time::sleep(Duration::from_millis(100)).await;
    (addr, state)
}

fn make_add_order_params(symbol: &str) -> KrakenWsAddOrderParams {
    KrakenWsAddOrderParams {
        order_type: KrakenOrderType::Limit,
        side: KrakenOrderSide::Buy,
        order_qty: 0.001,
        symbol: symbol.to_string(),
        token: "test-token".to_string(),
        limit_price: Some(50_000.0),
        time_in_force: None,
        expire_time: None,
        cl_ord_id: Some("O-TEST-001".to_string()),
        post_only: None,
        reduce_only: None,
        leverage: None,
        trigger: None,
        conditional: None,
    }
}

fn make_pending_request() -> PendingRequest {
    PendingRequest {
        operation: PendingOperation::Submit,
        client_order_ids: vec![ClientOrderId::new("O-TEST-001")],
        venue_order_ids: vec![None],
        ts_sent_ns: 0,
        new_quantity: None,
        new_price: None,
        new_trigger_price: None,
    }
}

fn build_order_request_state(
    cmd_tx_handle: Arc<
        tokio::sync::RwLock<
            tokio::sync::mpsc::UnboundedSender<
                nautilus_kraken::websocket::spot_v2::handler::SpotHandlerCommand,
            >,
        >,
    >,
) -> Arc<OrderRequestState> {
    let (event_tx, _event_rx) = tokio::sync::mpsc::unbounded_channel();
    let dispatch_state = Arc::new(WsDispatchState::new());
    let req_id_counter = Arc::new(std::sync::atomic::AtomicU64::new(0));

    Arc::new(OrderRequestState::new(
        cmd_tx_handle,
        event_tx,
        dispatch_state,
        req_id_counter,
        Duration::from_secs(5),
        TraderId::new("TESTER-001"),
        AccountId::new("KRAKEN-001"),
        Arc::new(tokio::sync::RwLock::new(None)),
        tokio_util::sync::CancellationToken::new(),
        nautilus_core::time::get_atomic_clock_realtime(),
    ))
}

/// Regression: build the dispatcher before `connect()`, like
/// `KrakenSpotExecutionClient::new` does, and verify the swapped-in live
/// cmd_tx is observed at send time.
#[rstest]
#[tokio::test]
async fn test_submit_order_via_ws_sends_add_order_message() {
    let (addr, server_state) = start_mock_server().await;
    let ws_url = format!("ws://{addr}/v2");

    let config = KrakenDataClientConfig {
        ws_private_url: Some(ws_url),
        ..Default::default()
    };

    let mut ws_client = KrakenSpotWebSocketClient::new(config, CancellationToken::new(), None);

    let cmd_tx_handle = ws_client.handler_command_handle();
    let order_state = build_order_request_state(cmd_tx_handle);

    ws_client.connect().await.unwrap();

    wait_until_async(
        || {
            let state = Arc::clone(&server_state);
            async move { *state.connection_count.lock().await > 0 }
        },
        Duration::from_secs(5),
    )
    .await;

    assert!(ws_client.is_active() || ws_client.is_connected());

    let params = make_add_order_params("BTC/USD");
    let pending = make_pending_request();

    order_state
        .submit(params, pending, 0)
        .expect("submit should succeed when WS is connected");

    wait_until_async(
        || {
            let state = Arc::clone(&server_state);
            async move { !state.received_messages.lock().await.is_empty() }
        },
        Duration::from_secs(5),
    )
    .await;

    let messages = server_state.messages().await;
    assert!(
        !messages.is_empty(),
        "Mock server should have received at least one message"
    );

    let first = &messages[0];
    assert_eq!(
        first.get("method").and_then(|v| v.as_str()),
        Some("add_order"),
        "Expected method 'add_order', was {first}"
    );

    let params_json = first.get("params").expect("Expected 'params' in message");
    assert_eq!(
        params_json.get("symbol").and_then(|v| v.as_str()),
        Some("BTC/USD"),
    );
    assert_eq!(
        params_json.get("side").and_then(|v| v.as_str()),
        Some("buy"),
    );
    assert_eq!(
        params_json.get("order_type").and_then(|v| v.as_str()),
        Some("limit"),
    );

    ws_client.disconnect().await.unwrap();
}

/// Verifies that when the WebSocket client is not connected (WS inactive),
/// [`OrderRequestState::submit`] returns an error because the handler
/// command channel's receiver is dropped before `connect()` is called.
/// This is the same observable outcome as `use_ws_trade=false`: the WS
/// path fails and the execution client falls back to REST.
#[rstest]
#[tokio::test]
async fn test_submit_order_falls_back_to_rest_when_ws_inactive() {
    let (addr, server_state) = start_mock_server().await;
    let ws_url = format!("ws://{addr}/v2");

    let config = KrakenDataClientConfig {
        ws_private_url: Some(ws_url),
        ..Default::default()
    };

    let ws_client = KrakenSpotWebSocketClient::new(config, CancellationToken::new(), None);

    assert!(
        !ws_client.is_active(),
        "Client should be inactive before connect"
    );
    assert!(ws_client.is_closed());

    let cmd_tx_handle = ws_client.handler_command_handle();
    let order_state = build_order_request_state(cmd_tx_handle);

    let params = make_add_order_params("BTC/USD");
    let pending = make_pending_request();

    let result = order_state.submit(params, pending, 0);
    assert!(
        result.is_err(),
        "submit should fail when WS is not connected (channel closed)"
    );

    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("channel closed") || err_msg.contains("closed"),
        "Error should mention channel closed, was: {err_msg}"
    );

    let messages = server_state.messages().await;
    assert!(
        messages.is_empty(),
        "Mock server should not have received any messages when WS is inactive; was {messages:?}"
    );
}
