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

//! Integration tests for the Binance Futures WebSocket Trading client using a mock server.

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
    common::enums::{BinanceFuturesOrderType, BinanceSide, BinanceTimeInForce},
    futures::{
        http::query::{
            BinanceCancelOrderParamsBuilder, BinanceModifyOrderParamsBuilder,
            BinanceNewOrderParamsBuilder,
        },
        websocket::trading::{
            client::BinanceFuturesWsTradingClient, messages::BinanceFuturesWsTradingMessage,
        },
    },
};
use nautilus_common::testing::wait_until_async;
use nautilus_network::websocket::TransportBackend;
use rstest::rstest;
use serde_json::json;

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

fn build_order_response(request_id: &str, order_id: i64, symbol: &str, status: &str) -> String {
    json!({
        "id": request_id,
        "status": 200,
        "result": {
            "symbol": symbol,
            "orderId": order_id,
            "clientOrderId": "test-client-order-1",
            "origQty": "0.001",
            "executedQty": "0",
            "cumQuote": "0",
            "price": "50000.00",
            "avgPrice": "0",
            "status": status,
            "timeInForce": "GTC",
            "type": "LIMIT",
            "side": "BUY",
            "positionSide": "BOTH",
            "reduceOnly": false,
            "closePosition": false,
            "workingType": "CONTRACT_PRICE",
            "priceProtect": false,
            "updateTime": 1700000000000_i64,
            "time": 1700000000000_i64,
        },
        "rateLimits": []
    })
    .to_string()
}

fn build_error_response(request_id: &str, code: i32, msg: &str) -> String {
    json!({
        "id": request_id,
        "status": 400,
        "error": {
            "code": code,
            "msg": msg
        },
        "rateLimits": []
    })
    .to_string()
}

fn build_cancel_all_response(request_id: &str) -> String {
    json!({
        "id": request_id,
        "status": 200,
        "result": {
            "code": 200,
            "msg": "The operation of cancel all open order is done."
        },
        "rateLimits": []
    })
    .to_string()
}

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

                state.received_requests.lock().await.push(value.clone());

                let method = value.get("method").and_then(|v| v.as_str());
                let request_id = value.get("id").and_then(|v| v.as_str()).unwrap_or("");

                let response = match method {
                    Some("order.place") => {
                        if state.reject_next_order.swap(false, Ordering::Relaxed) {
                            build_error_response(
                                request_id,
                                -2010,
                                "Order rejected: insufficient balance",
                            )
                        } else {
                            let symbol = value
                                .get("params")
                                .and_then(|p| p.get("symbol"))
                                .and_then(|v| v.as_str())
                                .unwrap_or("BTCUSDT");
                            build_order_response(request_id, 12345, symbol, "NEW")
                        }
                    }
                    Some("order.cancel") => {
                        if state.reject_next_order.swap(false, Ordering::Relaxed) {
                            build_error_response(request_id, -2011, "Unknown order sent")
                        } else {
                            let symbol = value
                                .get("params")
                                .and_then(|p| p.get("symbol"))
                                .and_then(|v| v.as_str())
                                .unwrap_or("BTCUSDT");
                            build_order_response(request_id, 12345, symbol, "CANCELED")
                        }
                    }
                    Some("order.modify") => {
                        if state.reject_next_order.swap(false, Ordering::Relaxed) {
                            build_error_response(request_id, -4028, "Price or quantity not changed")
                        } else {
                            let symbol = value
                                .get("params")
                                .and_then(|p| p.get("symbol"))
                                .and_then(|v| v.as_str())
                                .unwrap_or("BTCUSDT");
                            build_order_response(request_id, 12345, symbol, "NEW")
                        }
                    }
                    Some("openOrders.cancelAll") => build_cancel_all_response(request_id),
                    _ => continue,
                };

                if socket.send(Message::Text(response.into())).await.is_err() {
                    break;
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
    }

    let mut count = state.connection_count.lock().await;
    *count = count.saturating_sub(1);
}

fn create_test_router(state: TestServerState) -> Router {
    Router::new()
        .route("/ws-fapi/v1", get(handle_websocket))
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

fn create_test_client(addr: &SocketAddr) -> BinanceFuturesWsTradingClient {
    let ws_url = format!("ws://{addr}/ws-fapi/v1");
    BinanceFuturesWsTradingClient::new(
        Some(ws_url),
        "test-api-key".to_string(),
        "test-api-secret".to_string(),
        None,
        TransportBackend::default(),
    )
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

    tokio::time::sleep(Duration::from_millis(100)).await;
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
async fn test_connection_failure_invalid_url() {
    let mut client = BinanceFuturesWsTradingClient::new(
        Some("ws://127.0.0.1:9999/invalid".to_string()),
        "test-api-key".to_string(),
        "test-api-secret".to_string(),
        None,
        TransportBackend::default(),
    );

    let result = client.connect().await;
    assert!(result.is_err());
}

#[rstest]
#[tokio::test]
async fn test_place_order_request_format() {
    let (addr, state) = start_test_server().await.unwrap();
    let mut client = create_test_client(&addr);

    client.connect().await.unwrap();

    wait_until_async(
        || async { *state.connection_count.lock().await > 0 },
        Duration::from_secs(5),
    )
    .await;

    // Wait for Connected message
    let msg = client.recv().await;
    assert!(matches!(
        msg,
        Some(BinanceFuturesWsTradingMessage::Connected)
    ));

    let params = BinanceNewOrderParamsBuilder::default()
        .symbol("BTCUSDT")
        .side(BinanceSide::Buy)
        .order_type(BinanceFuturesOrderType::Limit)
        .time_in_force(BinanceTimeInForce::Gtc)
        .quantity("0.001")
        .price("50000.00")
        .new_client_order_id("test-client-order-1")
        .build()
        .unwrap();

    client.place_order(params).await.unwrap();

    wait_until_async(
        || async { !state.received_requests().await.is_empty() },
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
    assert!(request.get("id").is_some());

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
async fn test_place_order_accepted() {
    let (addr, state) = start_test_server().await.unwrap();
    let mut client = create_test_client(&addr);

    client.connect().await.unwrap();

    wait_until_async(
        || async { *state.connection_count.lock().await > 0 },
        Duration::from_secs(5),
    )
    .await;

    // Drain Connected message
    let _ = client.recv().await;

    let params = BinanceNewOrderParamsBuilder::default()
        .symbol("BTCUSDT")
        .side(BinanceSide::Buy)
        .order_type(BinanceFuturesOrderType::Limit)
        .time_in_force(BinanceTimeInForce::Gtc)
        .quantity("0.001")
        .price("50000.00")
        .build()
        .unwrap();

    client.place_order(params).await.unwrap();

    let msg = client.recv().await;

    match msg {
        Some(BinanceFuturesWsTradingMessage::OrderAccepted {
            request_id,
            response,
        }) => {
            assert!(request_id.starts_with("req-"));
            assert_eq!(response.order_id, 12345);
            assert_eq!(response.symbol.as_str(), "BTCUSDT");
        }
        other => panic!("Expected OrderAccepted, was {other:?}"),
    }

    client.disconnect().await;
}

#[rstest]
#[tokio::test]
async fn test_place_order_rejected() {
    let (addr, state) = start_test_server().await.unwrap();
    let mut client = create_test_client(&addr);

    client.connect().await.unwrap();

    wait_until_async(
        || async { *state.connection_count.lock().await > 0 },
        Duration::from_secs(5),
    )
    .await;

    // Drain Connected message
    let _ = client.recv().await;

    state.reject_next_order.store(true, Ordering::Relaxed);

    let params = BinanceNewOrderParamsBuilder::default()
        .symbol("BTCUSDT")
        .side(BinanceSide::Buy)
        .order_type(BinanceFuturesOrderType::Limit)
        .time_in_force(BinanceTimeInForce::Gtc)
        .quantity("0.001")
        .price("50000.00")
        .build()
        .unwrap();

    client.place_order(params).await.unwrap();

    let msg = client.recv().await;

    match msg {
        Some(BinanceFuturesWsTradingMessage::OrderRejected {
            request_id,
            code,
            msg,
        }) => {
            assert!(request_id.starts_with("req-"));
            assert_eq!(code, -2010);
            assert!(msg.contains("insufficient balance"));
        }
        other => panic!("Expected OrderRejected, was {other:?}"),
    }

    client.disconnect().await;
}

#[rstest]
#[tokio::test]
async fn test_cancel_order_request_format() {
    let (addr, state) = start_test_server().await.unwrap();
    let mut client = create_test_client(&addr);

    client.connect().await.unwrap();

    wait_until_async(
        || async { *state.connection_count.lock().await > 0 },
        Duration::from_secs(5),
    )
    .await;

    // Drain Connected message
    let _ = client.recv().await;

    let params = BinanceCancelOrderParamsBuilder::default()
        .symbol("BTCUSDT")
        .order_id(12345_i64)
        .build()
        .unwrap();

    client.cancel_order(params).await.unwrap();

    wait_until_async(
        || async { !state.received_requests().await.is_empty() },
        Duration::from_secs(5),
    )
    .await;

    let requests = state.received_requests().await;
    let request = &requests[0];
    assert_eq!(
        request.get("method").and_then(|v| v.as_str()),
        Some("order.cancel")
    );

    let params = request.get("params").unwrap();
    assert_eq!(
        params.get("symbol").and_then(|v| v.as_str()),
        Some("BTCUSDT")
    );
    assert!(params.get("signature").is_some());

    client.disconnect().await;
}

#[rstest]
#[tokio::test]
async fn test_cancel_order_accepted() {
    let (addr, state) = start_test_server().await.unwrap();
    let mut client = create_test_client(&addr);

    client.connect().await.unwrap();

    wait_until_async(
        || async { *state.connection_count.lock().await > 0 },
        Duration::from_secs(5),
    )
    .await;

    // Drain Connected message
    let _ = client.recv().await;

    let params = BinanceCancelOrderParamsBuilder::default()
        .symbol("BTCUSDT")
        .order_id(12345_i64)
        .build()
        .unwrap();

    client.cancel_order(params).await.unwrap();

    let msg = client.recv().await;

    match msg {
        Some(BinanceFuturesWsTradingMessage::OrderCanceled {
            request_id,
            response,
        }) => {
            assert!(request_id.starts_with("req-"));
            assert_eq!(response.order_id, 12345);
        }
        other => panic!("Expected OrderCanceled, was {other:?}"),
    }

    client.disconnect().await;
}

#[rstest]
#[tokio::test]
async fn test_cancel_order_rejected() {
    let (addr, state) = start_test_server().await.unwrap();
    let mut client = create_test_client(&addr);

    client.connect().await.unwrap();

    wait_until_async(
        || async { *state.connection_count.lock().await > 0 },
        Duration::from_secs(5),
    )
    .await;

    // Drain Connected message
    let _ = client.recv().await;

    state.reject_next_order.store(true, Ordering::Relaxed);

    let params = BinanceCancelOrderParamsBuilder::default()
        .symbol("BTCUSDT")
        .order_id(99999_i64)
        .build()
        .unwrap();

    client.cancel_order(params).await.unwrap();

    let msg = client.recv().await;

    match msg {
        Some(BinanceFuturesWsTradingMessage::CancelRejected {
            request_id,
            code,
            msg,
        }) => {
            assert!(request_id.starts_with("req-"));
            assert_eq!(code, -2011);
            assert!(msg.contains("Unknown order"));
        }
        other => panic!("Expected CancelRejected, was {other:?}"),
    }

    client.disconnect().await;
}

#[rstest]
#[tokio::test]
async fn test_modify_order_request_format() {
    let (addr, state) = start_test_server().await.unwrap();
    let mut client = create_test_client(&addr);

    client.connect().await.unwrap();

    wait_until_async(
        || async { *state.connection_count.lock().await > 0 },
        Duration::from_secs(5),
    )
    .await;

    // Drain Connected message
    let _ = client.recv().await;

    let params = BinanceModifyOrderParamsBuilder::default()
        .symbol("BTCUSDT")
        .order_id(12345_i64)
        .side(BinanceSide::Buy)
        .quantity("0.002")
        .price("51000.00")
        .build()
        .unwrap();

    client.modify_order(params).await.unwrap();

    wait_until_async(
        || async { !state.received_requests().await.is_empty() },
        Duration::from_secs(5),
    )
    .await;

    let requests = state.received_requests().await;
    let request = &requests[0];
    assert_eq!(
        request.get("method").and_then(|v| v.as_str()),
        Some("order.modify")
    );

    let params = request.get("params").unwrap();
    assert_eq!(
        params.get("symbol").and_then(|v| v.as_str()),
        Some("BTCUSDT")
    );
    assert_eq!(
        params.get("quantity").and_then(|v| v.as_str()),
        Some("0.002")
    );
    assert_eq!(
        params.get("price").and_then(|v| v.as_str()),
        Some("51000.00")
    );
    assert!(params.get("signature").is_some());

    client.disconnect().await;
}

#[rstest]
#[tokio::test]
async fn test_modify_order_accepted() {
    let (addr, state) = start_test_server().await.unwrap();
    let mut client = create_test_client(&addr);

    client.connect().await.unwrap();

    wait_until_async(
        || async { *state.connection_count.lock().await > 0 },
        Duration::from_secs(5),
    )
    .await;

    // Drain Connected message
    let _ = client.recv().await;

    let params = BinanceModifyOrderParamsBuilder::default()
        .symbol("ETHUSDT")
        .order_id(12345_i64)
        .side(BinanceSide::Sell)
        .quantity("1.0")
        .price("3000.00")
        .build()
        .unwrap();

    client.modify_order(params).await.unwrap();

    let msg = client.recv().await;

    match msg {
        Some(BinanceFuturesWsTradingMessage::OrderModified {
            request_id,
            response,
        }) => {
            assert!(request_id.starts_with("req-"));
            assert_eq!(response.order_id, 12345);
        }
        other => panic!("Expected OrderModified, was {other:?}"),
    }

    client.disconnect().await;
}

#[rstest]
#[tokio::test]
async fn test_modify_order_rejected() {
    let (addr, state) = start_test_server().await.unwrap();
    let mut client = create_test_client(&addr);

    client.connect().await.unwrap();

    wait_until_async(
        || async { *state.connection_count.lock().await > 0 },
        Duration::from_secs(5),
    )
    .await;

    // Drain Connected message
    let _ = client.recv().await;

    state.reject_next_order.store(true, Ordering::Relaxed);

    let params = BinanceModifyOrderParamsBuilder::default()
        .symbol("BTCUSDT")
        .order_id(12345_i64)
        .side(BinanceSide::Buy)
        .quantity("0.001")
        .price("50000.00")
        .build()
        .unwrap();

    client.modify_order(params).await.unwrap();

    let msg = client.recv().await;

    match msg {
        Some(BinanceFuturesWsTradingMessage::ModifyRejected {
            request_id,
            code,
            msg,
        }) => {
            assert!(request_id.starts_with("req-"));
            assert_eq!(code, -4028);
            assert!(msg.contains("not changed"));
        }
        other => panic!("Expected ModifyRejected, was {other:?}"),
    }

    client.disconnect().await;
}

#[rstest]
#[tokio::test]
async fn test_request_id_increments() {
    let (addr, state) = start_test_server().await.unwrap();
    let mut client = create_test_client(&addr);

    client.connect().await.unwrap();

    wait_until_async(
        || async { *state.connection_count.lock().await > 0 },
        Duration::from_secs(5),
    )
    .await;

    // Drain Connected message
    let _ = client.recv().await;

    let params = BinanceNewOrderParamsBuilder::default()
        .symbol("BTCUSDT")
        .side(BinanceSide::Buy)
        .order_type(BinanceFuturesOrderType::Limit)
        .time_in_force(BinanceTimeInForce::Gtc)
        .quantity("0.001")
        .price("50000.00")
        .build()
        .unwrap();

    let id1 = client.place_order(params.clone()).await.unwrap();
    let _ = client.recv().await; // Drain response

    let id2 = client.place_order(params).await.unwrap();

    assert_ne!(id1, id2);
    assert!(id1.starts_with("req-"));
    assert!(id2.starts_with("req-"));

    client.disconnect().await;
}

#[rstest]
#[tokio::test]
async fn test_default_client_creation() {
    let client = BinanceFuturesWsTradingClient::new(
        None,
        "test-key".to_string(),
        "test-secret".to_string(),
        None,
        TransportBackend::default(),
    );

    assert!(!client.is_active());
    assert!(client.is_closed());
}
