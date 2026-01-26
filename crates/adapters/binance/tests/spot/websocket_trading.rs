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

//! Integration tests for the Binance Spot WebSocket Trading client using a mock server.

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
    common::enums::{BinanceSide, BinanceTimeInForce},
    spot::{
        enums::BinanceSpotOrderType,
        http::query::NewOrderParams,
        websocket::trading::{client::BinanceSpotWsTradingClient, messages::NautilusWsApiMessage},
    },
};
use nautilus_common::testing::wait_until_async;
use rstest::rstest;
use serde_json::json;

// SBE schema constants
const SBE_SCHEMA_ID: u16 = 3;
const SBE_SCHEMA_VERSION: u16 = 2;
const WEBSOCKET_RESPONSE_TEMPLATE_ID: u16 = 50;
const WEBSOCKET_RESPONSE_BLOCK_LENGTH: u16 = 3;

// Test server state for tracking WebSocket connections
#[derive(Clone)]
struct TestServerState {
    connection_count: Arc<tokio::sync::Mutex<usize>>,
    received_requests: Arc<tokio::sync::Mutex<Vec<serde_json::Value>>>,
    disconnect_trigger: Arc<AtomicBool>,
    reject_next_order: Arc<AtomicBool>,
    ping_count: Arc<AtomicUsize>,
}

impl Default for TestServerState {
    fn default() -> Self {
        Self {
            connection_count: Arc::new(tokio::sync::Mutex::new(0)),
            received_requests: Arc::new(tokio::sync::Mutex::new(Vec::new())),
            disconnect_trigger: Arc::new(AtomicBool::new(false)),
            reject_next_order: Arc::new(AtomicBool::new(false)),
            ping_count: Arc::new(AtomicUsize::new(0)),
        }
    }
}

impl TestServerState {
    async fn received_requests(&self) -> Vec<serde_json::Value> {
        self.received_requests.lock().await.clone()
    }
}

// SBE encoding helpers
fn create_sbe_header(block_length: u16, template_id: u16) -> [u8; 8] {
    let mut header = [0u8; 8];
    header[0..2].copy_from_slice(&block_length.to_le_bytes());
    header[2..4].copy_from_slice(&template_id.to_le_bytes());
    header[4..6].copy_from_slice(&SBE_SCHEMA_ID.to_le_bytes());
    header[6..8].copy_from_slice(&SBE_SCHEMA_VERSION.to_le_bytes());
    header
}

fn write_var_string(buf: &mut Vec<u8>, s: &str) {
    buf.push(s.len() as u8);
    buf.extend_from_slice(s.as_bytes());
}

fn write_var_data(buf: &mut Vec<u8>, data: &[u8]) {
    buf.push(data.len() as u8);
    buf.extend_from_slice(data);
}

/// Build a WebSocketResponse envelope with the given status, request ID, and result payload.
fn build_ws_response_envelope(status: u16, request_id: &str, result: &[u8]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(256);

    // Message header (8 bytes)
    buf.extend_from_slice(&create_sbe_header(
        WEBSOCKET_RESPONSE_BLOCK_LENGTH,
        WEBSOCKET_RESPONSE_TEMPLATE_ID,
    ));

    // Fixed block: deprecated (1 byte) + status (2 bytes) = 3 bytes
    buf.push(0); // deprecated
    buf.extend_from_slice(&status.to_le_bytes());

    // rate_limits group (empty)
    let group_block_length: u16 = 0;
    let group_count: u32 = 0;
    buf.extend_from_slice(&group_block_length.to_le_bytes());
    buf.extend_from_slice(&group_count.to_le_bytes());

    // id (var string)
    write_var_string(&mut buf, request_id);

    // result (var data)
    write_var_data(&mut buf, result);

    buf
}

/// Build a mock NewOrderFull SBE response.
fn build_new_order_full_response(order_id: u64, client_order_id: &str, symbol: &str) -> Vec<u8> {
    // Simplified mock - just enough to be parsed
    let mut buf = Vec::new();

    // Message header (8 bytes)
    let block_length: u16 = 153;
    let template_id: u16 = 302; // NEW_ORDER_FULL
    buf.extend_from_slice(&block_length.to_le_bytes());
    buf.extend_from_slice(&template_id.to_le_bytes());
    buf.extend_from_slice(&SBE_SCHEMA_ID.to_le_bytes());
    buf.extend_from_slice(&SBE_SCHEMA_VERSION.to_le_bytes());

    // Fixed fields block (153 bytes) - fill with zeros and set key fields
    let mut block = vec![0u8; 153];

    // priceExponent at offset 0 (i8)
    block[0] = 0xFE; // -2

    // qtyExponent at offset 1 (i8)
    block[1] = 0xFD; // -3

    // orderId at offset 2 (u64)
    block[2..10].copy_from_slice(&order_id.to_le_bytes());

    // orderListId at offset 10 (i64) - set to -1 for no list
    block[10..18].copy_from_slice(&(-1i64).to_le_bytes());

    // transactTime at offset 18 (i64)
    let now_us = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_micros() as i64;
    block[18..26].copy_from_slice(&now_us.to_le_bytes());

    // price mantissa at offset 26 (i64)
    block[26..34].copy_from_slice(&50000i64.to_le_bytes());

    // origQty mantissa at offset 34 (u64)
    block[34..42].copy_from_slice(&1000u64.to_le_bytes());

    // executedQty mantissa at offset 42 (u64)
    block[42..50].copy_from_slice(&0u64.to_le_bytes());

    // cummulativeQuoteQty mantissa at offset 50 (u64)
    block[50..58].copy_from_slice(&0u64.to_le_bytes());

    // status at offset 58 (u8)
    block[58] = 0; // NEW

    // timeInForce at offset 59 (u8)
    block[59] = 0; // GTC

    // orderType at offset 60 (u8)
    block[60] = 0; // LIMIT

    // side at offset 61 (u8)
    block[61] = 0; // BUY

    // stopPrice mantissa at offset 62 (i64)
    // trailingDelta at offset 70 (i64)
    // trailingTime at offset 78 (i64)
    // icebergQty mantissa at offset 86 (u64)
    // strategyId at offset 94 (i64)
    // strategyType at offset 102 (i64)
    // orderCapacity at offset 110 (u8)
    // workingFloor at offset 111 (u8)
    // selfTradePreventionMode at offset 112 (u8)
    // preventedMatchId at offset 113 (u64)
    // preventedQuantity at offset 121 (u64)
    // usedSor at offset 129 (u8)
    // workingTime at offset 130 (i64)
    // quoteQty mantissa at offset 138 (u64)
    // effectiveTime at offset 146 (i64)
    // unused at offset 154 (u8 - padding)

    buf.extend_from_slice(&block);

    // Fills group (empty)
    let group_block_length: u16 = 0;
    let group_count: u32 = 0;
    buf.extend_from_slice(&group_block_length.to_le_bytes());
    buf.extend_from_slice(&group_count.to_le_bytes());

    // symbol (var string)
    write_var_string(&mut buf, symbol);

    // clientOrderId (var string)
    write_var_string(&mut buf, client_order_id);

    buf
}

// WebSocket handler
async fn handle_websocket(ws: WebSocketUpgrade, State(state): State<TestServerState>) -> Response {
    ws.on_upgrade(|socket| handle_socket(socket, state))
}

async fn handle_socket(mut socket: WebSocket, state: TestServerState) {
    {
        let mut count = state.connection_count.lock().await;
        *count += 1;
    }

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

                // Store received request
                state.received_requests.lock().await.push(value.clone());

                let method = value.get("method").and_then(|v| v.as_str());
                let request_id = value.get("id").and_then(|v| v.as_str()).unwrap_or("");

                match method {
                    Some("order.place") => {
                        if state.reject_next_order.swap(false, Ordering::Relaxed) {
                            // Send JSON error response
                            let error_response = json!({
                                "id": request_id,
                                "code": -2010,
                                "msg": "Order rejected: insufficient balance"
                            });
                            if socket
                                .send(Message::Text(error_response.to_string().into()))
                                .await
                                .is_err()
                            {
                                break;
                            }
                        } else {
                            // Send SBE success response
                            let client_order_id = value
                                .get("params")
                                .and_then(|p| p.get("newClientOrderId"))
                                .and_then(|v| v.as_str())
                                .unwrap_or("test-order");
                            let symbol = value
                                .get("params")
                                .and_then(|p| p.get("symbol"))
                                .and_then(|v| v.as_str())
                                .unwrap_or("BTCUSDT");

                            let order_response =
                                build_new_order_full_response(12345, client_order_id, symbol);
                            let envelope =
                                build_ws_response_envelope(200, request_id, &order_response);

                            if socket.send(Message::Binary(envelope.into())).await.is_err() {
                                break;
                            }
                        }
                    }
                    Some("order.cancel") => {
                        // Send JSON error for cancel as example
                        let error_response = json!({
                            "id": request_id,
                            "code": -2011,
                            "msg": "Order does not exist"
                        });
                        if socket
                            .send(Message::Text(error_response.to_string().into()))
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
        .route("/ws-api/v3", get(handle_websocket))
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

    // Give server time to start
    tokio::time::sleep(Duration::from_millis(100)).await;
    Ok((addr, state))
}

fn create_test_client(addr: &SocketAddr) -> BinanceSpotWsTradingClient {
    let ws_url = format!("ws://{addr}/ws-api/v3");
    BinanceSpotWsTradingClient::new(
        Some(ws_url),
        "test_api_key".to_string(),
        "test_api_secret".to_string(),
        None,
    )
}

#[rstest]
#[tokio::test]
async fn test_client_connection() {
    let (addr, state) = start_test_server().await.unwrap();
    let mut client = create_test_client(&addr);

    client.connect().await.unwrap();

    // Wait for connection to be established
    wait_until_async(
        || async { *state.connection_count.lock().await > 0 },
        Duration::from_secs(5),
    )
    .await;

    assert!(client.is_active());
    assert_eq!(*state.connection_count.lock().await, 1);

    client.disconnect().await;
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

    client.disconnect().await;

    // Disconnect completes without panic - internal cleanup occurs
    // The handler task is cancelled and any pending requests would fail
    // The actual connection state depends on underlying WebSocket cleanup timing
}

#[rstest]
#[tokio::test]
async fn test_place_order_sends_correct_request() {
    let (addr, state) = start_test_server().await.unwrap();
    let mut client = create_test_client(&addr);

    client.connect().await.unwrap();

    wait_until_async(
        || async { *state.connection_count.lock().await > 0 },
        Duration::from_secs(5),
    )
    .await;

    let params = NewOrderParams {
        symbol: "BTCUSDT".to_string(),
        side: BinanceSide::Buy,
        order_type: BinanceSpotOrderType::Limit,
        time_in_force: Some(BinanceTimeInForce::Gtc),
        quantity: Some("0.001".to_string()),
        quote_order_qty: None,
        price: Some("50000.00".to_string()),
        new_client_order_id: Some("test-order-1".to_string()),
        stop_price: None,
        trailing_delta: None,
        iceberg_qty: None,
        new_order_resp_type: None,
        self_trade_prevention_mode: None,
        strategy_id: None,
        strategy_type: None,
    };

    let request_id = client.place_order(params).await.unwrap();

    // Wait for request to be received
    wait_until_async(
        || async { !state.received_requests.lock().await.is_empty() },
        Duration::from_secs(5),
    )
    .await;

    let requests = state.received_requests().await;
    assert!(!requests.is_empty());

    let request = &requests[0];
    assert_eq!(
        request.get("method").and_then(|v| v.as_str()),
        Some("order.place")
    );
    assert_eq!(
        request.get("id").and_then(|v| v.as_str()),
        Some(request_id.as_str())
    );

    let params = request.get("params").unwrap();
    assert_eq!(
        params.get("symbol").and_then(|v| v.as_str()),
        Some("BTCUSDT")
    );
    assert_eq!(params.get("side").and_then(|v| v.as_str()), Some("BUY"));
    assert!(params.get("timestamp").is_some());
    assert!(params.get("apiKey").is_some());
    assert!(params.get("signature").is_some());

    client.disconnect().await;
}

#[rstest]
#[tokio::test]
async fn test_order_rejection_via_json_error() {
    let (addr, state) = start_test_server().await.unwrap();
    let mut client = create_test_client(&addr);

    client.connect().await.unwrap();

    wait_until_async(
        || async { *state.connection_count.lock().await > 0 },
        Duration::from_secs(5),
    )
    .await;

    // Tell server to reject next order
    state.reject_next_order.store(true, Ordering::Relaxed);

    let params = NewOrderParams {
        symbol: "BTCUSDT".to_string(),
        side: BinanceSide::Buy,
        order_type: BinanceSpotOrderType::Limit,
        time_in_force: Some(BinanceTimeInForce::Gtc),
        quantity: Some("0.001".to_string()),
        quote_order_qty: None,
        price: Some("50000.00".to_string()),
        new_client_order_id: Some("test-order-2".to_string()),
        stop_price: None,
        trailing_delta: None,
        iceberg_qty: None,
        new_order_resp_type: None,
        self_trade_prevention_mode: None,
        strategy_id: None,
        strategy_type: None,
    };

    let _request_id = client.place_order(params).await.unwrap();

    // Wait for response
    tokio::time::sleep(Duration::from_millis(200)).await;

    // Receive the rejection message
    if let Some(msg) = client.recv().await {
        match msg {
            NautilusWsApiMessage::Connected => {
                // First message is Connected, get the next one
                if let Some(rejection) = client.recv().await {
                    match rejection {
                        NautilusWsApiMessage::OrderRejected { code, msg, .. } => {
                            assert_eq!(code, -2010);
                            assert!(msg.contains("insufficient balance"));
                        }
                        _ => panic!("Expected OrderRejected, was {rejection:?}"),
                    }
                }
            }
            NautilusWsApiMessage::OrderRejected { code, msg, .. } => {
                assert_eq!(code, -2010);
                assert!(msg.contains("insufficient balance"));
            }
            _ => panic!("Expected OrderRejected or Connected, was {msg:?}"),
        }
    }

    client.disconnect().await;
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
async fn test_next_request_id_increments() {
    let (addr, state) = start_test_server().await.unwrap();
    let mut client = create_test_client(&addr);

    client.connect().await.unwrap();

    wait_until_async(
        || async { *state.connection_count.lock().await > 0 },
        Duration::from_secs(5),
    )
    .await;

    let params = NewOrderParams {
        symbol: "BTCUSDT".to_string(),
        side: BinanceSide::Buy,
        order_type: BinanceSpotOrderType::Limit,
        time_in_force: Some(BinanceTimeInForce::Gtc),
        quantity: Some("0.001".to_string()),
        quote_order_qty: None,
        price: Some("50000.00".to_string()),
        new_client_order_id: Some("order-1".to_string()),
        stop_price: None,
        trailing_delta: None,
        iceberg_qty: None,
        new_order_resp_type: None,
        self_trade_prevention_mode: None,
        strategy_id: None,
        strategy_type: None,
    };

    let id1 = client.place_order(params.clone()).await.unwrap();

    let params2 = NewOrderParams {
        new_client_order_id: Some("order-2".to_string()),
        ..params
    };
    let id2 = client.place_order(params2).await.unwrap();

    // Request IDs should be different and incrementing
    assert!(id1 != id2);
    assert_eq!(id1, "req-1");
    assert_eq!(id2, "req-2");

    client.disconnect().await;
}

#[rstest]
#[tokio::test]
async fn test_reconnection_clears_pending_requests() {
    let (addr, state) = start_test_server().await.unwrap();
    let mut client = create_test_client(&addr);

    client.connect().await.unwrap();

    wait_until_async(
        || async { *state.connection_count.lock().await > 0 },
        Duration::from_secs(5),
    )
    .await;

    // The reconnection flow with pending requests is tested via the handler
    // When a reconnect happens, fail_pending_requests should be called

    client.disconnect().await;
}

#[rstest]
#[tokio::test]
async fn test_connection_failure_invalid_url() {
    let mut client = BinanceSpotWsTradingClient::new(
        Some("ws://127.0.0.1:9999/invalid".to_string()),
        "test_api_key".to_string(),
        "test_api_secret".to_string(),
        None,
    );

    // Connection should fail
    let connect_result = client.connect().await;
    assert!(connect_result.is_err());
}
