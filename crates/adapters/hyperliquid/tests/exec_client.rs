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

//! Integration tests for Hyperliquid execution client trading flows.
//!
//! These tests focus on order submission, cancellation, and modification flows
//! using mock HTTP and WebSocket servers. WebSocket execution parsing is tested in websocket.rs.

use std::{
    cell::RefCell,
    collections::HashMap,
    net::SocketAddr,
    path::PathBuf,
    rc::Rc,
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
    response::{IntoResponse, Json, Response},
    routing::post,
};
use futures_util::StreamExt;
use nautilus_common::{
    cache::Cache,
    clients::ExecutionClient,
    live::runner::set_exec_event_sender,
    messages::{
        ExecutionEvent, ExecutionReport,
        execution::{
            BatchCancelOrders, CancelAllOrders, CancelOrder, GenerateFillReports,
            GenerateOrderStatusReport, GenerateOrderStatusReports, ModifyOrder, QueryAccount,
            QueryOrder, SubmitOrder, SubmitOrderList,
        },
    },
    testing::wait_until_async,
};
use nautilus_core::{Params, UUID4, UnixNanos};
use nautilus_hyperliquid::{
    HyperliquidHttpClient, HyperliquidWebSocketClient,
    common::{
        consts::{HYPERLIQUID_CLIENT_ID, HYPERLIQUID_VENUE, NAUTILUS_BUILDER_ADDRESS},
        enums::HyperliquidEnvironment,
    },
    config::HyperliquidExecClientConfig,
    execution::HyperliquidExecutionClient,
    http::models::Cloid,
};
use nautilus_live::ExecutionClientCore;
use nautilus_model::{
    accounts::{AccountAny, MarginAccount},
    data::QuoteTick,
    enums::{
        AccountType, ContingencyType, LiquiditySide, OmsType, OrderSide, OrderStatus, OrderType,
        TimeInForce, TriggerType,
    },
    events::{
        AccountState, OrderAccepted, OrderEventAny, OrderFilled, OrderInitialized, OrderSubmitted,
    },
    identifiers::{
        AccountId, ClientOrderId, InstrumentId, OrderListId, StrategyId, TradeId, TraderId,
        VenueOrderId,
    },
    orders::{LimitOrder, MarketOrder, Order, OrderAny, OrderList, StopMarketOrder},
    reports::OrderStatusReport,
    types::{AccountBalance, Currency, Money, Price, Quantity},
};
use nautilus_network::{
    http::{HttpClient, Method},
    websocket::TransportBackend,
};
use rstest::rstest;
use serde_json::{Value, json};

#[derive(Clone)]
struct TestServerState {
    exchange_request_count: Arc<tokio::sync::Mutex<usize>>,
    last_exchange_action: Arc<tokio::sync::Mutex<Option<Value>>>,
    reject_next_order: Arc<std::sync::atomic::AtomicBool>,
    /// Returns a `status="ok"` envelope whose inner `statuses[0]` carries a
    /// per-order `error` object on the next exchange call. This exercises
    /// the `extract_inner_error` path in the execution client, which is
    /// distinct from the top-level `status="err"` handled by `reject_next_order`.
    inner_order_error_next: Arc<std::sync::atomic::AtomicBool>,
    /// Optional override for the `cancel` response payload on the next
    /// exchange call (e.g. to simulate per-item errors in batch cancel).
    cancel_response_override: Arc<tokio::sync::Mutex<Option<Value>>>,
    /// Optional override for the `order` response payload on the next
    /// exchange call (e.g. to simulate per-order mixed status arrays).
    order_response_override: Arc<tokio::sync::Mutex<Option<Value>>>,
    /// Fail the next exchange call with an upstream error.
    fail_next_exchange: Arc<std::sync::atomic::AtomicBool>,
    /// Fail `frontendOpenOrders` info calls with a transport error (503) while
    /// positive, decrementing once per call. Use a large value to simulate a
    /// sustained outage.
    fail_frontend_open_orders_count: Arc<AtomicUsize>,
    /// Optional override for `frontendOpenOrders` info responses.
    frontend_open_orders_response: Arc<tokio::sync::Mutex<Option<Value>>>,
    /// Optional override for `orderStatus` info responses.
    order_status_response: Arc<tokio::sync::Mutex<Option<Value>>>,
    /// Optional override for `spotClearinghouseState` info responses;
    /// defaults to `{"balances": []}` when unset.
    spot_clearinghouse_response: Arc<tokio::sync::Mutex<Option<Value>>>,
    /// Optional override for `clearinghouseState` (perp) info responses.
    perp_clearinghouse_response: Arc<tokio::sync::Mutex<Option<Value>>>,
    /// Captures the `user` field from the most recent `clearinghouseState`
    /// request so tests can verify the address sent to the venue.
    last_clearinghouse_user: Arc<tokio::sync::Mutex<Option<String>>>,
    /// Optional override for `userFills` info responses; defaults to `[]`.
    user_fills_response: Arc<tokio::sync::Mutex<Option<Value>>>,
    rate_limit_after: Arc<AtomicUsize>,
    /// When set, `handle_exchange` awaits `pause_release` before returning.
    /// Lets a test hold the response so it can assert on synchronous state
    /// established by the calling thread before the spawned action task can
    /// observe the response.
    pause_next_exchange: Arc<std::sync::atomic::AtomicBool>,
    pause_release: Arc<tokio::sync::Notify>,
}

impl Default for TestServerState {
    fn default() -> Self {
        Self {
            exchange_request_count: Arc::new(tokio::sync::Mutex::new(0)),
            last_exchange_action: Arc::new(tokio::sync::Mutex::new(None)),
            reject_next_order: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            inner_order_error_next: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            cancel_response_override: Arc::new(tokio::sync::Mutex::new(None)),
            order_response_override: Arc::new(tokio::sync::Mutex::new(None)),
            fail_next_exchange: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            fail_frontend_open_orders_count: Arc::new(AtomicUsize::new(0)),
            frontend_open_orders_response: Arc::new(tokio::sync::Mutex::new(None)),
            order_status_response: Arc::new(tokio::sync::Mutex::new(None)),
            spot_clearinghouse_response: Arc::new(tokio::sync::Mutex::new(None)),
            perp_clearinghouse_response: Arc::new(tokio::sync::Mutex::new(None)),
            last_clearinghouse_user: Arc::new(tokio::sync::Mutex::new(None)),
            user_fills_response: Arc::new(tokio::sync::Mutex::new(None)),
            rate_limit_after: Arc::new(AtomicUsize::new(usize::MAX)),
            pause_next_exchange: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            pause_release: Arc::new(tokio::sync::Notify::new()),
        }
    }
}

fn data_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test_data")
}

fn load_json(filename: &str) -> Value {
    let content = std::fs::read_to_string(data_path().join(filename))
        .unwrap_or_else(|_| panic!("failed to read {filename}"));
    serde_json::from_str(&content).expect("invalid json")
}

async fn wait_for_server(addr: SocketAddr, path: &str) {
    let health_url = format!("http://{addr}{path}");
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

async fn handle_info(State(state): State<TestServerState>, body: axum::body::Bytes) -> Response {
    let Ok(request_body): Result<Value, _> = serde_json::from_slice(&body) else {
        return (
            axum::http::StatusCode::BAD_REQUEST,
            Json(json!({"error": "Invalid JSON"})),
        )
            .into_response();
    };

    let request_type = request_body
        .get("type")
        .and_then(|t| t.as_str())
        .unwrap_or("");

    match request_type {
        "meta" => {
            let meta = load_json("http_meta_perp_sample.json");
            Json(meta).into_response()
        }
        "allPerpMetas" => {
            let meta = load_json("http_meta_perp_sample.json");
            Json(json!([meta])).into_response()
        }
        "metaAndAssetCtxs" => {
            let meta = load_json("http_meta_perp_sample.json");
            Json(json!([meta, []])).into_response()
        }
        "spotMeta" => Json(json!({"universe": [], "tokens": []})).into_response(),
        "spotMetaAndAssetCtxs" => Json(json!([{"universe": [], "tokens": []}, []])).into_response(),
        "openOrders" => Json(json!([])).into_response(),
        "frontendOpenOrders" => {
            if state
                .fail_frontend_open_orders_count
                .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| {
                    if n > 0 { Some(n - 1) } else { None }
                })
                .is_ok()
            {
                return (
                    axum::http::StatusCode::SERVICE_UNAVAILABLE,
                    Json(json!({"error": "upstream unavailable"})),
                )
                    .into_response();
            }

            if let Some(body) = state.frontend_open_orders_response.lock().await.clone() {
                Json(body).into_response()
            } else {
                Json(json!([])).into_response()
            }
        }
        "orderStatus" => {
            if let Some(body) = state.order_status_response.lock().await.clone() {
                Json(body).into_response()
            } else {
                Json(json!({"status": "unknownOid"})).into_response()
            }
        }
        "userFills" => {
            if let Some(body) = state.user_fills_response.lock().await.clone() {
                Json(body).into_response()
            } else {
                Json(json!([])).into_response()
            }
        }
        "userFees" => Json(json!({
            "userCrossRate": "0.00045",
            "userAddRate": "0.00015"
        }))
        .into_response(),
        "clearinghouseState" => {
            if let Some(user) = request_body.get("user").and_then(|u| u.as_str()) {
                *state.last_clearinghouse_user.lock().await = Some(user.to_string());
            }

            if let Some(body) = state.perp_clearinghouse_response.lock().await.clone() {
                return Json(body).into_response();
            }

            Json(json!({
                "marginSummary": {
                    "accountValue": "10000.0",
                    "totalMarginUsed": "0.0",
                    "totalNtlPos": "0.0",
                    "totalRawUsd": "10000.0"
                },
                "crossMarginSummary": {
                    "accountValue": "10000.0",
                    "totalMarginUsed": "0.0",
                    "totalNtlPos": "0.0",
                    "totalRawUsd": "10000.0"
                },
                "crossMaintenanceMarginUsed": "0.0",
                "withdrawable": "10000.0",
                "assetPositions": []
            }))
            .into_response()
        }
        "spotClearinghouseState" => {
            if let Some(body) = state.spot_clearinghouse_response.lock().await.clone() {
                Json(body).into_response()
            } else {
                Json(json!({"balances": []})).into_response()
            }
        }
        _ => Json(json!({})).into_response(),
    }
}

async fn handle_exchange(
    State(state): State<TestServerState>,
    body: axum::body::Bytes,
) -> Response {
    // Increment the counter and release the lock immediately so an
    // arbitrarily long pause inside this handler (see `pause_next_exchange`)
    // does not block tests that read the counter.
    let count_snapshot = {
        let mut count = state.exchange_request_count.lock().await;
        *count += 1;
        *count
    };

    let limit_after = state.rate_limit_after.load(Ordering::Relaxed);
    if count_snapshot > limit_after {
        return (
            axum::http::StatusCode::TOO_MANY_REQUESTS,
            Json(json!({
                "status": "err",
                "response": {
                    "type": "error",
                    "data": "Rate limit exceeded"
                }
            })),
        )
            .into_response();
    }

    let Ok(request_body): Result<Value, _> = serde_json::from_slice(&body) else {
        return (
            axum::http::StatusCode::BAD_REQUEST,
            Json(json!({
                "status": "err",
                "response": {
                    "type": "error",
                    "data": "Invalid JSON body"
                }
            })),
        )
            .into_response();
    };

    if let Some(action) = request_body.get("action") {
        *state.last_exchange_action.lock().await = Some(action.clone());
    }

    // Validate signed request format
    if request_body.get("action").is_none()
        || request_body.get("nonce").is_none()
        || request_body.get("signature").is_none()
    {
        return (
            axum::http::StatusCode::BAD_REQUEST,
            Json(json!({
                "status": "err",
                "response": {
                    "type": "error",
                    "data": "Missing required fields: action, nonce, or signature"
                }
            })),
        )
            .into_response();
    }

    if state.fail_next_exchange.swap(false, Ordering::Relaxed) {
        return (
            axum::http::StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": "upstream unavailable"})),
        )
            .into_response();
    }

    if state.reject_next_order.swap(false, Ordering::Relaxed) {
        return Json(json!({
            "status": "err",
            "response": {
                "type": "error",
                "data": "Order rejected: insufficient margin"
            }
        }))
        .into_response();
    }

    let action = request_body.get("action").unwrap();
    let action_type = action.get("type").and_then(|t| t.as_str());

    if state.inner_order_error_next.swap(false, Ordering::Relaxed) {
        // Match the response `type` to the action type so the inner-error
        // shape mirrors what the venue would actually emit.
        let response_type = match action_type {
            Some("modify") => "modify",
            Some("cancel" | "cancelByCloid") => "cancel",
            _ => "order",
        };
        return Json(json!({
            "status": "ok",
            "response": {
                "type": response_type,
                "data": {
                    "statuses": [{
                        "error": "Order rejected: insufficient margin"
                    }]
                }
            }
        }))
        .into_response();
    }

    if state.pause_next_exchange.swap(false, Ordering::Relaxed) {
        state.pause_release.notified().await;
    }

    match action_type {
        Some("order") => {
            if let Some(body) = state.order_response_override.lock().await.take() {
                return Json(body).into_response();
            }
            Json(json!({
                "status": "ok",
                "response": {
                    "type": "order",
                    "data": {
                        "statuses": [{
                            "resting": {
                                "oid": 12345
                            }
                        }]
                    }
                }
            }))
            .into_response()
        }
        Some("cancel" | "cancelByCloid") => {
            if let Some(body) = state.cancel_response_override.lock().await.take() {
                return Json(body).into_response();
            }
            Json(json!({
                "status": "ok",
                "response": {
                    "type": "cancel",
                    "data": {
                        "statuses": ["success"]
                    }
                }
            }))
            .into_response()
        }
        Some("modify") => Json(json!({
            "status": "ok",
            "response": {
                "type": "modify",
                "data": {
                    "statuses": [{
                        "resting": {
                            "oid": 12346
                        }
                    }]
                }
            }
        }))
        .into_response(),
        Some("updateLeverage") => Json(json!({
            "status": "ok",
            "response": {
                "type": "updateLeverage",
                "data": {}
            }
        }))
        .into_response(),
        _ => Json(json!({
            "status": "err",
            "response": {
                "type": "error",
                "data": format!("Unknown action type: {action_type:?}")
            }
        }))
        .into_response(),
    }
}

async fn handle_health() -> impl IntoResponse {
    axum::http::StatusCode::OK
}

async fn handle_ws_upgrade(ws: WebSocketUpgrade, State(state): State<TestServerState>) -> Response {
    ws.on_upgrade(move |socket| handle_ws_socket(socket, state))
}

async fn handle_ws_post(socket: &mut WebSocket, state: &TestServerState, payload: &Value) -> bool {
    let id = payload.get("id").and_then(|v| v.as_u64()).unwrap_or(0);
    let Some(action) = payload
        .get("request")
        .and_then(|v| v.get("payload"))
        .and_then(|v| v.get("action"))
    else {
        let response = json!({
            "channel": "post",
            "data": {
                "id": id,
                "response": {
                    "type": "error",
                    "payload": "Missing signed action payload"
                }
            }
        });
        return socket
            .send(Message::Text(response.to_string().into()))
            .await
            .is_ok();
    };

    let count_snapshot = {
        let mut count = state.exchange_request_count.lock().await;
        *count += 1;
        *count
    };

    let limit_after = state.rate_limit_after.load(Ordering::Relaxed);
    if count_snapshot > limit_after {
        return send_ws_post_error_response(socket, id, "429 Too Many Requests").await;
    }

    *state.last_exchange_action.lock().await = Some(action.clone());

    if state.fail_next_exchange.swap(false, Ordering::Relaxed) {
        return send_ws_post_error_response(socket, id, "503 Service Unavailable").await;
    }

    if state.reject_next_order.swap(false, Ordering::Relaxed) {
        let response = json!({
            "status": "err",
            "response": {
                "type": "error",
                "data": "Order rejected: insufficient margin"
            }
        });
        return send_ws_post_action_response(socket, id, response).await;
    }

    let action_type = action.get("type").and_then(|t| t.as_str());

    if state.inner_order_error_next.swap(false, Ordering::Relaxed) {
        let response_type = match action_type {
            Some("modify") => "modify",
            Some("cancel" | "cancelByCloid") => "cancel",
            _ => "order",
        };
        let response = json!({
            "status": "ok",
            "response": {
                "type": response_type,
                "data": {
                    "statuses": [{
                        "error": "Order rejected: insufficient margin"
                    }]
                }
            }
        });
        return send_ws_post_action_response(socket, id, response).await;
    }

    if state.pause_next_exchange.swap(false, Ordering::Relaxed) {
        state.pause_release.notified().await;
    }

    let response = match action_type {
        Some("order") => {
            if let Some(body) = state.order_response_override.lock().await.take() {
                body
            } else {
                json!({
                    "status": "ok",
                    "response": {
                        "type": "order",
                        "data": {
                            "statuses": [{
                                "resting": {
                                    "oid": 12345
                                }
                            }]
                        }
                    }
                })
            }
        }
        Some("cancel" | "cancelByCloid") => {
            if let Some(body) = state.cancel_response_override.lock().await.take() {
                body
            } else {
                json!({
                    "status": "ok",
                    "response": {
                        "type": "cancel",
                        "data": {
                            "statuses": ["success"]
                        }
                    }
                })
            }
        }
        Some("modify") => json!({
            "status": "ok",
            "response": {
                "type": "modify",
                "data": {
                    "statuses": [{
                        "resting": {
                            "oid": 12346
                        }
                    }]
                }
            }
        }),
        Some("updateLeverage") => json!({
            "status": "ok",
            "response": {
                "type": "updateLeverage",
                "data": {}
            }
        }),
        _ => json!({
            "status": "err",
            "response": {
                "type": "error",
                "data": format!("Unknown action type: {action_type:?}")
            }
        }),
    };

    send_ws_post_action_response(socket, id, response).await
}

async fn send_ws_post_action_response(socket: &mut WebSocket, id: u64, payload: Value) -> bool {
    let response = json!({
        "channel": "post",
        "data": {
            "id": id,
            "response": {
                "type": "action",
                "payload": payload
            }
        }
    });
    socket
        .send(Message::Text(response.to_string().into()))
        .await
        .is_ok()
}

async fn send_ws_post_error_response(socket: &mut WebSocket, id: u64, payload: &str) -> bool {
    let response = json!({
        "channel": "post",
        "data": {
            "id": id,
            "response": {
                "type": "error",
                "payload": payload
            }
        }
    });
    socket
        .send(Message::Text(response.to_string().into()))
        .await
        .is_ok()
}

async fn handle_ws_socket(mut socket: WebSocket, state: TestServerState) {
    while let Some(message) = socket.next().await {
        let Ok(message) = message else { break };

        match message {
            Message::Text(text) => {
                if let Ok(payload) = serde_json::from_str::<Value>(&text) {
                    let method = payload.get("method").and_then(|m| m.as_str());
                    match method {
                        Some("ping") => {
                            let pong = json!({"channel": "pong"});

                            if socket
                                .send(Message::Text(pong.to_string().into()))
                                .await
                                .is_err()
                            {
                                break;
                            }
                        }
                        Some("subscribe") => {
                            // Acknowledge subscription silently
                        }
                        Some("unsubscribe") => {}
                        Some("post") if !handle_ws_post(&mut socket, &state, &payload).await => {
                            break;
                        }
                        Some("post") => {}
                        _ => {}
                    }
                }
            }
            // Inner if consumes `data`, cannot hoist into a match guard
            #[expect(clippy::collapsible_match)]
            Message::Ping(data) => {
                if socket.send(Message::Pong(data)).await.is_err() {
                    break;
                }
            }
            Message::Close(_) => break,
            _ => {}
        }
    }
}

fn create_test_router(state: TestServerState) -> Router {
    Router::new()
        .route("/info", post(handle_info))
        .route("/exchange", post(handle_exchange))
        .route("/health", axum::routing::get(handle_health))
        .route("/ws", axum::routing::get(handle_ws_upgrade))
        .with_state(state)
}

async fn start_mock_server(state: TestServerState) -> SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let router = create_test_router(state);

    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });

    wait_for_server(addr, "/health").await;
    addr
}

struct TestExchangeClient {
    client: HttpClient,
    base_url: String,
}

impl TestExchangeClient {
    fn new(base_url: String) -> Self {
        let client = HttpClient::new(
            HashMap::from([("Content-Type".to_string(), "application/json".to_string())]),
            vec![],
            vec![],
            None,
            None,
            None,
        )
        .unwrap();

        Self { client, base_url }
    }

    async fn send_exchange_action(&self, action: Value) -> Result<Value, String> {
        let url = format!("{}/exchange", self.base_url);

        let request = json!({
            "action": action,
            "nonce": 1234567890000u64,
            "signature": {
                "r": "0x1234567890abcdef",
                "s": "0xfedcba0987654321",
                "v": 27
            }
        });

        let body = serde_json::to_vec(&request).map_err(|e| e.to_string())?;

        let response = self
            .client
            .request(Method::POST, url, None, None, Some(body), None, None)
            .await
            .map_err(|e| e.to_string())?;

        serde_json::from_slice(&response.body).map_err(|e| e.to_string())
    }

    async fn submit_order(
        &self,
        asset: u32,
        is_buy: bool,
        sz: &str,
        limit_px: &str,
    ) -> Result<Value, String> {
        let action = json!({
            "type": "order",
            "orders": [{
                "a": asset,
                "b": is_buy,
                "p": limit_px,
                "s": sz,
                "r": false,
                "t": {"limit": {"tif": "Gtc"}}
            }],
            "grouping": "na"
        });

        self.send_exchange_action(action).await
    }

    async fn cancel_order(&self, asset: u32, oid: u64) -> Result<Value, String> {
        let action = json!({
            "type": "cancel",
            "cancels": [{
                "a": asset,
                "o": oid
            }]
        });

        self.send_exchange_action(action).await
    }

    async fn cancel_by_cloid(&self, asset: u32, cloid: &str) -> Result<Value, String> {
        let action = json!({
            "type": "cancelByCloid",
            "cancels": [{
                "asset": asset,
                "cloid": cloid
            }]
        });

        self.send_exchange_action(action).await
    }

    async fn modify_order(
        &self,
        oid: u64,
        asset: u32,
        is_buy: bool,
        sz: &str,
        limit_px: &str,
    ) -> Result<Value, String> {
        let action = json!({
            "type": "modify",
            "oid": oid,
            "order": {
                "a": asset,
                "b": is_buy,
                "p": limit_px,
                "s": sz,
                "r": false,
                "t": {"limit": {"tif": "Gtc"}}
            }
        });

        self.send_exchange_action(action).await
    }
}

#[rstest]
#[tokio::test]
async fn test_submit_order_success() {
    let state = TestServerState::default();
    let addr = start_mock_server(state.clone()).await;

    let client = TestExchangeClient::new(format!("http://{addr}"));
    let result = client.submit_order(0, true, "0.1", "50000.0").await;

    assert!(result.is_ok());
    let response = result.unwrap();
    assert_eq!(response.get("status").unwrap().as_str().unwrap(), "ok");

    let data = &response["response"]["data"];
    assert!(data["statuses"][0]["resting"]["oid"].is_u64());
}

#[rstest]
#[tokio::test]
async fn test_submit_order_rejection() {
    let state = TestServerState::default();
    state.reject_next_order.store(true, Ordering::Relaxed);
    let addr = start_mock_server(state.clone()).await;

    let client = TestExchangeClient::new(format!("http://{addr}"));
    let result = client.submit_order(0, true, "0.1", "50000.0").await;

    assert!(result.is_ok());
    let response = result.unwrap();
    assert_eq!(response.get("status").unwrap().as_str().unwrap(), "err");
}

#[rstest]
#[tokio::test]
async fn test_submit_order_stores_action() {
    let state = TestServerState::default();
    let addr = start_mock_server(state.clone()).await;

    let client = TestExchangeClient::new(format!("http://{addr}"));
    let _ = client.submit_order(0, true, "0.5", "48000.0").await;

    let last_action = state.last_exchange_action.lock().await;
    let action = last_action.as_ref().unwrap();

    assert_eq!(action.get("type").unwrap().as_str().unwrap(), "order");
    assert!(action.get("orders").is_some());
}

#[rstest]
#[tokio::test]
async fn test_cancel_order_by_oid() {
    let state = TestServerState::default();
    let addr = start_mock_server(state.clone()).await;

    let client = TestExchangeClient::new(format!("http://{addr}"));
    let result = client.cancel_order(0, 12345).await;

    assert!(result.is_ok());
    let response = result.unwrap();
    assert_eq!(response.get("status").unwrap().as_str().unwrap(), "ok");
}

#[rstest]
#[tokio::test]
async fn test_cancel_order_by_cloid() {
    let state = TestServerState::default();
    let addr = start_mock_server(state.clone()).await;

    let client = TestExchangeClient::new(format!("http://{addr}"));
    let result = client.cancel_by_cloid(0, "my-order-123").await;

    assert!(result.is_ok());
    let response = result.unwrap();
    assert_eq!(response.get("status").unwrap().as_str().unwrap(), "ok");
}

#[rstest]
#[tokio::test]
async fn test_cancel_stores_action() {
    let state = TestServerState::default();
    let addr = start_mock_server(state.clone()).await;

    let client = TestExchangeClient::new(format!("http://{addr}"));
    let _ = client.cancel_order(0, 99999).await;

    let last_action = state.last_exchange_action.lock().await;
    let action = last_action.as_ref().unwrap();

    assert_eq!(action.get("type").unwrap().as_str().unwrap(), "cancel");
}

#[rstest]
#[tokio::test]
async fn test_modify_order_success() {
    let state = TestServerState::default();
    let addr = start_mock_server(state.clone()).await;

    let client = TestExchangeClient::new(format!("http://{addr}"));
    let result = client.modify_order(12345, 0, true, "0.2", "51000.0").await;

    assert!(result.is_ok());
    let response = result.unwrap();
    assert_eq!(response.get("status").unwrap().as_str().unwrap(), "ok");

    let data = &response["response"]["data"];
    assert!(data["statuses"][0]["resting"]["oid"].is_u64());
}

#[rstest]
#[tokio::test]
async fn test_modify_stores_action() {
    let state = TestServerState::default();
    let addr = start_mock_server(state.clone()).await;

    let client = TestExchangeClient::new(format!("http://{addr}"));
    let _ = client.modify_order(12345, 0, false, "0.3", "52000.0").await;

    let last_action = state.last_exchange_action.lock().await;
    let action = last_action.as_ref().unwrap();

    assert_eq!(action.get("type").unwrap().as_str().unwrap(), "modify");
    assert_eq!(action.get("oid").unwrap().as_u64().unwrap(), 12345);
}

#[rstest]
#[tokio::test]
async fn test_exchange_rate_limiting() {
    let state = TestServerState::default();
    state.rate_limit_after.store(2, Ordering::Relaxed);
    let addr = start_mock_server(state.clone()).await;

    let client = TestExchangeClient::new(format!("http://{addr}"));

    assert!(client.submit_order(0, true, "0.1", "50000.0").await.is_ok());
    assert!(client.submit_order(0, true, "0.1", "50000.0").await.is_ok());

    // Third triggers rate limit
    let result = client.submit_order(0, true, "0.1", "50000.0").await;
    assert!(result.is_ok());
    let response = result.unwrap();
    assert_eq!(response.get("status").unwrap().as_str().unwrap(), "err");
}

#[rstest]
#[tokio::test]
async fn test_exchange_request_count() {
    let state = TestServerState::default();
    let addr = start_mock_server(state.clone()).await;

    let client = TestExchangeClient::new(format!("http://{addr}"));

    let _ = client.submit_order(0, true, "0.1", "50000.0").await;
    let _ = client.cancel_order(0, 12345).await;
    let _ = client.modify_order(12345, 0, true, "0.2", "51000.0").await;

    assert_eq!(*state.exchange_request_count.lock().await, 3);
}

#[rstest]
#[tokio::test]
async fn test_multiple_orders_in_sequence() {
    let state = TestServerState::default();
    let addr = start_mock_server(state.clone()).await;

    let client = TestExchangeClient::new(format!("http://{addr}"));

    for i in 0..5 {
        let result = client
            .submit_order(i, i % 2 == 0, "0.1", &format!("{}.0", 50000 + i * 100))
            .await;
        assert!(result.is_ok());
    }

    assert_eq!(*state.exchange_request_count.lock().await, 5);
}

const TEST_PRIVATE_KEY: &str = "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef";

fn create_test_exec_config(addr: SocketAddr) -> HyperliquidExecClientConfig {
    HyperliquidExecClientConfig {
        private_key: Some(TEST_PRIVATE_KEY.to_string()),
        base_url_http: Some(format!("http://{addr}/info")),
        base_url_exchange: Some(format!("http://{addr}/exchange")),
        base_url_ws: Some(format!("ws://{addr}/ws")),
        environment: HyperliquidEnvironment::Mainnet,
        ws_post_timeout_secs: 1,
        ..HyperliquidExecClientConfig::default()
    }
}

async fn create_test_trade_signer(addr: SocketAddr) -> HyperliquidHttpClient {
    let mut signer = HyperliquidHttpClient::from_credentials(
        TEST_PRIVATE_KEY,
        None,
        HyperliquidEnvironment::Mainnet,
        1,
        None,
    )
    .unwrap();
    signer.set_base_info_url(format!("http://{addr}/info"));
    signer.set_base_exchange_url(format!("http://{addr}/exchange"));

    let instruments = signer.request_instruments().await.unwrap();
    for instrument in instruments {
        signer.cache_instrument(&instrument);
    }

    signer
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn test_ws_trading_submit_order_sends_builder_and_cloid() {
    let state = TestServerState::default();
    let addr = start_mock_server(state.clone()).await;
    let signer = create_test_trade_signer(addr).await;
    let mut ws_client = HyperliquidWebSocketClient::new(
        Some(format!("ws://{addr}/ws")),
        HyperliquidEnvironment::Mainnet,
        None,
        TransportBackend::default(),
        None,
    );
    ws_client.set_post_timeout(Duration::from_secs(1));
    ws_client.connect().await.unwrap();

    let client_order_id = ClientOrderId::new("O-WS-HIGH-LEVEL");
    ws_client
        .submit_order(
            &signer,
            InstrumentId::from(HYPERLIQUID_TEST_INSTRUMENT),
            client_order_id,
            OrderSide::Buy,
            OrderType::Limit,
            Quantity::from("0.0001"),
            TimeInForce::Gtc,
            Some(Price::from("56730.0")),
            None,
            false,
            false,
        )
        .await
        .unwrap();

    let action = state
        .last_exchange_action
        .lock()
        .await
        .clone()
        .expect("missing WS action");
    let order = &action["orders"][0];
    let expected_cloid = Cloid::from_client_order_id(client_order_id).to_hex();

    assert_eq!(action.get("type").and_then(|v| v.as_str()), Some("order"));
    assert_eq!(
        action
            .get("builder")
            .and_then(|v| v.get("b"))
            .and_then(|v| v.as_str()),
        Some(NAUTILUS_BUILDER_ADDRESS),
    );
    assert_eq!(
        action
            .get("builder")
            .and_then(|v| v.get("f"))
            .and_then(|v| v.as_u64()),
        Some(0),
    );
    assert_eq!(
        order.get("c").and_then(|v| v.as_str()),
        Some(expected_cloid.as_str()),
    );

    ws_client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn test_ws_trading_cancel_and_modify_send_expected_actions() {
    let state = TestServerState::default();
    let addr = start_mock_server(state.clone()).await;
    let signer = create_test_trade_signer(addr).await;
    let mut ws_client = HyperliquidWebSocketClient::new(
        Some(format!("ws://{addr}/ws")),
        HyperliquidEnvironment::Mainnet,
        None,
        TransportBackend::default(),
        None,
    );
    ws_client.set_post_timeout(Duration::from_secs(1));
    ws_client.connect().await.unwrap();

    let cancel_coid = ClientOrderId::new("O-WS-CANCEL");
    ws_client
        .cancel_order(
            &signer,
            InstrumentId::from(HYPERLIQUID_TEST_INSTRUMENT),
            Some(cancel_coid),
            None,
        )
        .await
        .unwrap();

    let cancel_action = state
        .last_exchange_action
        .lock()
        .await
        .clone()
        .expect("missing cancel action");
    let cancel_cloid = Cloid::from_client_order_id(cancel_coid).to_hex();

    assert_eq!(
        cancel_action.get("type").and_then(|v| v.as_str()),
        Some("cancelByCloid"),
    );
    assert_eq!(
        cancel_action["cancels"][0]
            .get("asset")
            .and_then(|v| v.as_u64()),
        Some(0),
    );
    assert_eq!(
        cancel_action["cancels"][0]
            .get("cloid")
            .and_then(|v| v.as_str()),
        Some(cancel_cloid.as_str()),
    );

    let modify_coid = ClientOrderId::new("O-WS-MODIFY");
    ws_client
        .modify_order(
            &signer,
            InstrumentId::from(HYPERLIQUID_TEST_INSTRUMENT),
            VenueOrderId::from("12345"),
            OrderSide::Sell,
            OrderType::Limit,
            Price::from("56800.0"),
            Quantity::from("0.0002"),
            None,
            true,
            true,
            TimeInForce::Gtc,
            Some(modify_coid),
        )
        .await
        .unwrap();

    let modify_action = state
        .last_exchange_action
        .lock()
        .await
        .clone()
        .expect("missing modify action");
    let modify_order = &modify_action["order"];
    let modify_cloid = Cloid::from_client_order_id(modify_coid).to_hex();

    assert_eq!(
        modify_action.get("type").and_then(|v| v.as_str()),
        Some("modify"),
    );
    assert_eq!(
        modify_action.get("oid").and_then(|v| v.as_u64()),
        Some(12345)
    );
    assert_eq!(modify_order.get("a").and_then(|v| v.as_u64()), Some(0));
    assert_eq!(modify_order.get("b").and_then(|v| v.as_bool()), Some(false));
    assert_eq!(
        modify_order.get("p").and_then(|v| v.as_str()),
        Some("56800")
    );
    assert_eq!(
        modify_order.get("s").and_then(|v| v.as_str()),
        Some("0.0002"),
    );
    assert_eq!(modify_order.get("r").and_then(|v| v.as_bool()), Some(true));
    assert_eq!(
        modify_order.get("c").and_then(|v| v.as_str()),
        Some(modify_cloid.as_str()),
    );

    ws_client.disconnect().await.unwrap();
}

fn create_test_execution_client(
    addr: SocketAddr,
) -> (
    HyperliquidExecutionClient,
    tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
    Rc<RefCell<Cache>>,
) {
    let trader_id = TraderId::from("TESTER-001");
    let account_id = AccountId::from("HYPERLIQUID-001");
    let client_id = *HYPERLIQUID_CLIENT_ID;

    let cache = Rc::new(RefCell::new(Cache::default()));

    let core = ExecutionClientCore::new(
        trader_id,
        client_id,
        *HYPERLIQUID_VENUE,
        OmsType::Netting,
        account_id,
        AccountType::Margin,
        None,
        cache.clone(),
    );

    let config = create_test_exec_config(addr);

    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    set_exec_event_sender(tx);

    let client = HyperliquidExecutionClient::new(core, config).unwrap();

    (client, rx, cache)
}

fn add_test_account_to_cache(cache: &Rc<RefCell<Cache>>, account_id: AccountId) {
    let account_state = AccountState::new(
        account_id,
        AccountType::Margin,
        vec![AccountBalance::new(
            Money::from("10000.0 USDC"),
            Money::from("0 USDC"),
            Money::from("10000.0 USDC"),
        )],
        vec![],
        true,
        UUID4::new(),
        UnixNanos::default(),
        UnixNanos::default(),
        None,
    );

    let account = AccountAny::Margin(MarginAccount::new(account_state, true));
    cache.borrow_mut().add_account(account).unwrap();
}

#[rstest]
#[tokio::test]
async fn test_exec_client_creation() {
    let state = TestServerState::default();
    let addr = start_mock_server(state).await;
    let (client, _rx, _cache) = create_test_execution_client(addr);

    assert_eq!(client.client_id(), *HYPERLIQUID_CLIENT_ID);
    assert_eq!(client.venue(), *HYPERLIQUID_VENUE);
    assert_eq!(client.oms_type(), OmsType::Netting);
    assert!(!client.is_connected());
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn test_exec_client_connect_disconnect() {
    let state = TestServerState::default();
    let addr = start_mock_server(state).await;
    let (mut client, _rx, cache) = create_test_execution_client(addr);
    add_test_account_to_cache(&cache, AccountId::from("HYPERLIQUID-001"));

    client.connect().await.unwrap();
    assert!(client.is_connected());

    client.disconnect().await.unwrap();
    assert!(!client.is_connected());
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn test_query_account_does_not_block_within_runtime() {
    let state = TestServerState::default();
    let addr = start_mock_server(state).await;
    let (mut client, mut rx, cache) = create_test_execution_client(addr);
    add_test_account_to_cache(&cache, AccountId::from("HYPERLIQUID-001"));

    client.start().unwrap();

    let cmd = QueryAccount::new(
        TraderId::from("TESTER-001"),
        Some(*HYPERLIQUID_CLIENT_ID),
        AccountId::from("HYPERLIQUID-001"),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None, // correlation_id
    );

    let result = client.query_account(cmd);
    assert!(result.is_ok());

    let event = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("timed out waiting for account event")
        .expect("channel closed without event");

    assert!(matches!(event, ExecutionEvent::Account(_)));
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn test_query_account_emits_spot_balances() {
    let state = TestServerState::default();
    // Populate spot response with non-USDC holdings that the execution client
    // must surface on the emitted AccountState.
    *state.spot_clearinghouse_response.lock().await = Some(json!({
        "balances": [
            {"coin": "USDC", "token": 0, "total": "150", "hold": "0", "entryNtl": "0"},
            {"coin": "PURR", "token": 1, "total": "2000", "hold": "100", "entryNtl": "1234.56"},
            {"coin": "HYPE", "token": 150, "total": "5.2", "hold": "0", "entryNtl": "75.4"}
        ]
    }));
    let addr = start_mock_server(state).await;
    let (mut client, mut rx, cache) = create_test_execution_client(addr);
    add_test_account_to_cache(&cache, AccountId::from("HYPERLIQUID-001"));

    client.start().unwrap();

    let cmd = QueryAccount::new(
        TraderId::from("TESTER-001"),
        Some(*HYPERLIQUID_CLIENT_ID),
        AccountId::from("HYPERLIQUID-001"),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None, // correlation_id
    );
    client.query_account(cmd).unwrap();

    let event = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("timed out waiting for account event")
        .expect("channel closed without event");

    let ExecutionEvent::Account(account_state) = event else {
        panic!("expected ExecutionEvent::Account, was {event:?}");
    };

    let codes: Vec<&str> = account_state
        .balances
        .iter()
        .map(|b| b.currency.code.as_str())
        .collect();

    // Perp summary is present (10000 USDC) so spot USDC must be skipped,
    // but non-USDC spot tokens must appear
    assert!(codes.contains(&"USDC"), "USDC missing, found: {codes:?}");
    assert!(codes.contains(&"PURR"), "PURR missing, found: {codes:?}");
    assert!(codes.contains(&"HYPE"), "HYPE missing, found: {codes:?}");

    let purr = account_state
        .balances
        .iter()
        .find(|b| b.currency.code.as_str() == "PURR")
        .unwrap();
    assert_eq!(purr.total.as_f64(), 2000.0);
    assert_eq!(purr.locked.as_f64(), 100.0);
    assert_eq!(purr.free.as_f64(), 1900.0);

    let usdc_count = codes.iter().filter(|c| **c == "USDC").count();
    assert_eq!(usdc_count, 1, "USDC must not be duplicated");
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn test_query_account_propagates_spot_endpoint_failure() {
    // Force spotClearinghouseState to return a shape-mismatched payload.
    // serde_json::from_value fails in the execution client's task, and
    // the spawned future should bail out before emitting an AccountState.
    let state = TestServerState::default();
    *state.spot_clearinghouse_response.lock().await = Some(json!({
        "balances": "this-should-be-an-array"
    }));

    let addr = start_mock_server(state).await;
    let (mut client, mut rx, cache) = create_test_execution_client(addr);
    add_test_account_to_cache(&cache, AccountId::from("HYPERLIQUID-001"));

    client.start().unwrap();

    let cmd = QueryAccount::new(
        TraderId::from("TESTER-001"),
        Some(*HYPERLIQUID_CLIENT_ID),
        AccountId::from("HYPERLIQUID-001"),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None, // correlation_id
    );
    client.query_account(cmd).unwrap();

    // With the spot payload malformed, the spawned task should log and
    // bail out before emitting an AccountState. Allow a short window for
    // any stray event to arrive; none should.
    let event = tokio::time::timeout(Duration::from_millis(500), rx.recv()).await;

    assert!(
        event.is_err(),
        "no AccountState must be emitted when spot state fails to parse; got {event:?}",
    );
}

const HYPERLIQUID_TEST_INSTRUMENT: &str = "BTC-USD-PERP.HYPERLIQUID";

fn make_limit_order(id: &str) -> OrderAny {
    OrderAny::Limit(LimitOrder::new(
        TraderId::from("TESTER-001"),
        StrategyId::from("S-001"),
        InstrumentId::from(HYPERLIQUID_TEST_INSTRUMENT),
        ClientOrderId::from(id),
        OrderSide::Buy,
        Quantity::from("0.0001"),
        Price::from("56730.0"),
        TimeInForce::Gtc,
        None,
        false,
        false,
        false,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        UUID4::new(),
        UnixNanos::default(),
    ))
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn test_submit_order_inner_error_cleans_up_dispatch_state() {
    // When the exchange accepts the request envelope but rejects the
    // individual order via `statuses[0].error`, the submit-order spawn task
    // must run `cleanup_terminal` on the dispatch state so the identity
    // registered at submission time is not left behind. A regression here
    // would leak an order identity per failed submission in long-running
    // sessions.
    //
    // The top-level `status="err"` envelope (`reject_next_order`) is
    // intentionally NOT used: `post_action_exec` converts that shape into
    // a request-level `Err` which is left alone because the venue may
    // still have accepted the order (periodic reconciliation resolves it).
    let state = TestServerState::default();
    state.inner_order_error_next.store(true, Ordering::Relaxed);
    let addr = start_mock_server(state).await;

    let (mut client, _rx, cache) = create_test_execution_client(addr);
    add_test_account_to_cache(&cache, AccountId::from("HYPERLIQUID-001"));
    client.connect().await.unwrap();

    let order = make_limit_order("O-SUB-REJ");
    cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .unwrap();

    assert!(
        client
            .ws_dispatch_state()
            .lookup_identity(&order.client_order_id())
            .is_none(),
        "identity should not be registered before submit",
    );

    let cmd = SubmitOrder::from_order(
        &order,
        order.trader_id(),
        Some(*HYPERLIQUID_CLIENT_ID),
        None,
        UUID4::new(),
        UnixNanos::default(),
    );

    client.submit_order(cmd).unwrap();

    // Identity is registered synchronously inside submit_order before the
    // spawn_task fires.
    assert!(
        client
            .ws_dispatch_state()
            .lookup_identity(&order.client_order_id())
            .is_some(),
        "identity should be registered immediately on submit",
    );

    // The spawn task runs the action call and, on rejection, invokes
    // `cleanup_terminal`. Poll until the identity is gone.
    let dispatch = client.ws_dispatch_state().clone();
    let cid = order.client_order_id();
    wait_until_async(
        move || {
            let dispatch = dispatch.clone();
            async move { dispatch.lookup_identity(&cid).is_none() }
        },
        Duration::from_secs(5),
    )
    .await;

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn test_submit_order_ws_post_includes_builder_attribution() {
    let state = TestServerState::default();
    let exchange_count = state.exchange_request_count.clone();
    let last_action = state.last_exchange_action.clone();
    let addr = start_mock_server(state).await;

    let (mut client, _rx, cache) = create_test_execution_client(addr);
    add_test_account_to_cache(&cache, AccountId::from("HYPERLIQUID-001"));
    client.connect().await.unwrap();

    let order = make_limit_order("O-BUILDER-WS");
    cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .unwrap();

    let cmd = SubmitOrder::from_order(
        &order,
        order.trader_id(),
        Some(*HYPERLIQUID_CLIENT_ID),
        None,
        UUID4::new(),
        UnixNanos::default(),
    );

    client.submit_order(cmd).unwrap();

    wait_until_async(
        move || {
            let exchange_count = exchange_count.clone();
            async move { *exchange_count.lock().await >= 1 }
        },
        Duration::from_secs(5),
    )
    .await;

    let action = last_action.lock().await.clone().expect("missing WS action");
    assert_eq!(action.get("type").and_then(|v| v.as_str()), Some("order"));
    assert_eq!(
        action
            .get("builder")
            .and_then(|v| v.get("b"))
            .and_then(|v| v.as_str()),
        Some(NAUTILUS_BUILDER_ADDRESS),
    );
    assert_eq!(
        action
            .get("builder")
            .and_then(|v| v.get("f"))
            .and_then(|v| v.as_u64()),
        Some(0),
    );

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn test_modify_order_marks_pending_modify_before_post_completes() {
    // The pending-modify marker must be visible while the modify post call
    // is still in flight. Otherwise a `CANCELED(old_voi)` arriving via WS
    // during the post window slips past the cancel-before-accept
    // suppression and emits a spurious `OrderCanceled`.
    //
    // Pause the mock server so the spawned action task is held mid-await
    // when we assert. A regression that defers the marker until after the
    // post success path would leave the marker unset under that hold and
    // fail this test.
    let state = TestServerState::default();
    state.pause_next_exchange.store(true, Ordering::Relaxed);
    let pause_release = state.pause_release.clone();
    let exchange_request_count = state.exchange_request_count.clone();
    let addr = start_mock_server(state).await;

    let (mut client, _rx, cache) = create_test_execution_client(addr);
    add_test_account_to_cache(&cache, AccountId::from("HYPERLIQUID-001"));
    client.connect().await.unwrap();

    let order = make_limit_order("O-MOD-PRE");
    cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .unwrap();

    let old_voi = VenueOrderId::from("88888");
    let cmd = ModifyOrder::new(
        order.trader_id(),
        Some(*HYPERLIQUID_CLIENT_ID),
        order.strategy_id(),
        order.instrument_id(),
        order.client_order_id(),
        Some(old_voi),
        Some(Quantity::from("0.0002")),
        Some(Price::from("56800.0")),
        None,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None, // correlation_id
    );

    client.modify_order(cmd).unwrap();

    // Wait until the mock server has actually received the modify request,
    // proving the spawned action task is suspended in `post_action_exec`.
    // Polling state set synchronously by `modify_order` (such as
    // `pending_modify`) would not prove the marker is observable while the
    // post call is in flight: a regression that defers the marker until
    // after the post success path could still pass on a fast runtime.
    wait_until_async(
        move || {
            let count = exchange_request_count.clone();
            async move { *count.lock().await >= 1 }
        },
        Duration::from_secs(5),
    )
    .await;

    assert_eq!(
        client
            .ws_dispatch_state()
            .pending_modify(&order.client_order_id()),
        Some(old_voi),
        "marker must be set while the modify post is still in flight",
    );

    pause_release.notify_one();

    wait_until_async(
        || async { client.pending_tasks_all_finished() },
        Duration::from_secs(5),
    )
    .await;

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn test_modify_order_success_marks_pending_modify() {
    // After a successful modify action round-trip, the dispatch state must
    // carry a pending-modify marker keyed on `client_order_id` and pointing
    // at the OLD venue order id. The cancel-before-accept branch in dispatch
    // relies on this marker to suppress an early CANCELED(old_voi); a
    // regression here would let the stale cancel leg leak to strategies.
    let state = TestServerState::default();
    let addr = start_mock_server(state).await;

    let (mut client, _rx, cache) = create_test_execution_client(addr);
    add_test_account_to_cache(&cache, AccountId::from("HYPERLIQUID-001"));
    client.connect().await.unwrap();

    let order = make_limit_order("O-MOD-OK");
    cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .unwrap();

    let old_voi = VenueOrderId::from("99999");
    let cmd = ModifyOrder::new(
        order.trader_id(),
        Some(*HYPERLIQUID_CLIENT_ID),
        order.strategy_id(),
        order.instrument_id(),
        order.client_order_id(),
        Some(old_voi),
        Some(Quantity::from("0.0002")),
        Some(Price::from("56800.0")),
        None,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None, // correlation_id
    );

    client.modify_order(cmd).unwrap();

    let dispatch = client.ws_dispatch_state().clone();
    let cid = order.client_order_id();
    wait_until_async(
        move || {
            let dispatch = dispatch.clone();
            async move { dispatch.pending_modify(&cid).is_some() }
        },
        Duration::from_secs(5),
    )
    .await;

    assert_eq!(
        client.ws_dispatch_state().pending_modify(&cid),
        Some(old_voi),
    );

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn test_modify_order_rejection_does_not_mark_pending_modify() {
    // A rejected modify (post error branch) must leave no marker, so that a
    // later legitimate CANCELED for the same `client_order_id` is not
    // wrongly suppressed by the cancel-before-accept branch.
    let state = TestServerState::default();
    state.reject_next_order.store(true, Ordering::Relaxed);
    let addr = start_mock_server(state).await;

    let (mut client, _rx, cache) = create_test_execution_client(addr);
    add_test_account_to_cache(&cache, AccountId::from("HYPERLIQUID-001"));
    client.connect().await.unwrap();

    let order = make_limit_order("O-MOD-REJ");
    cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .unwrap();

    let old_voi = VenueOrderId::from("77777");
    let cmd = ModifyOrder::new(
        order.trader_id(),
        Some(*HYPERLIQUID_CLIENT_ID),
        order.strategy_id(),
        order.instrument_id(),
        order.client_order_id(),
        Some(old_voi),
        Some(Quantity::from("0.0002")),
        Some(Price::from("56800.0")),
        None,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None, // correlation_id
    );

    client.modify_order(cmd).unwrap();

    // Wait for the spawn task to drain fully so we know the action round-trip
    // AND the client's response-handling continuation have both run. Only
    // then is a negative assertion on the marker meaningful: asserting
    // earlier could race past the rejection branch and silently accept a
    // bug that erroneously set the marker on failure.
    wait_until_async(
        || async { client.pending_tasks_all_finished() },
        Duration::from_secs(5),
    )
    .await;

    assert!(
        client
            .ws_dispatch_state()
            .pending_modify(&order.client_order_id())
            .is_none(),
        "failed modify must not leave a pending-modify marker",
    );

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn test_modify_order_inner_error_clears_pending_modify() {
    // The exchange returned status="ok" but the inner statuses[0] carries an
    // error object. The marker is set before the post await, so the
    // extract_inner_error branch must clear it; otherwise a later legitimate
    // CANCELED for the same client_order_id is wrongly suppressed.
    let state = TestServerState::default();
    state.inner_order_error_next.store(true, Ordering::Relaxed);
    let addr = start_mock_server(state).await;

    let (mut client, _rx, cache) = create_test_execution_client(addr);
    add_test_account_to_cache(&cache, AccountId::from("HYPERLIQUID-001"));
    client.connect().await.unwrap();

    let order = make_limit_order("O-MOD-INNER-ERR");
    cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .unwrap();

    let old_voi = VenueOrderId::from("66666");
    let cmd = ModifyOrder::new(
        order.trader_id(),
        Some(*HYPERLIQUID_CLIENT_ID),
        order.strategy_id(),
        order.instrument_id(),
        order.client_order_id(),
        Some(old_voi),
        Some(Quantity::from("0.0002")),
        Some(Price::from("56800.0")),
        None,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None, // correlation_id
    );

    client.modify_order(cmd).unwrap();

    wait_until_async(
        || async { client.pending_tasks_all_finished() },
        Duration::from_secs(5),
    )
    .await;

    assert!(
        client
            .ws_dispatch_state()
            .pending_modify(&order.client_order_id())
            .is_none(),
        "inner-error modify must not leave a pending-modify marker",
    );

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn test_modify_order_post_error_clears_pending_modify() {
    // The post request returns an upstream error. The marker is set before
    // the post await, so the Err branch must clear it; otherwise a later
    // legitimate CANCELED for the same client_order_id is wrongly suppressed.
    let state = TestServerState::default();
    state.fail_next_exchange.store(true, Ordering::Relaxed);
    let addr = start_mock_server(state).await;

    let (mut client, _rx, cache) = create_test_execution_client(addr);
    add_test_account_to_cache(&cache, AccountId::from("HYPERLIQUID-001"));
    client.connect().await.unwrap();

    let order = make_limit_order("O-MOD-TRANSPORT");
    cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .unwrap();

    let old_voi = VenueOrderId::from("55555");
    let cmd = ModifyOrder::new(
        order.trader_id(),
        Some(*HYPERLIQUID_CLIENT_ID),
        order.strategy_id(),
        order.instrument_id(),
        order.client_order_id(),
        Some(old_voi),
        Some(Quantity::from("0.0002")),
        Some(Price::from("56800.0")),
        None,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None, // correlation_id
    );

    client.modify_order(cmd).unwrap();

    wait_until_async(
        || async { client.pending_tasks_all_finished() },
        Duration::from_secs(5),
    )
    .await;

    assert!(
        client
            .ws_dispatch_state()
            .pending_modify(&order.client_order_id())
            .is_none(),
        "transport-failure modify must not leave a pending-modify marker",
    );

    client.disconnect().await.unwrap();
}

fn make_status_report_cmd(
    client_order_id: Option<ClientOrderId>,
    venue_order_id: Option<VenueOrderId>,
) -> GenerateOrderStatusReport {
    GenerateOrderStatusReport {
        command_id: UUID4::new(),
        ts_init: UnixNanos::default(),
        instrument_id: Some(InstrumentId::from(HYPERLIQUID_TEST_INSTRUMENT)),
        client_order_id,
        venue_order_id,
        params: None,
        correlation_id: None,
        causation_id: None,
    }
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn test_generate_order_status_report_requires_identifier() {
    let state = TestServerState::default();
    let addr = start_mock_server(state).await;
    let (mut client, _rx, cache) = create_test_execution_client(addr);
    add_test_account_to_cache(&cache, AccountId::from("HYPERLIQUID-001"));
    client.connect().await.unwrap();

    let cmd = make_status_report_cmd(None, None);
    let report = client.generate_order_status_report(&cmd).await.unwrap();
    assert!(report.is_none());

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn test_generate_order_status_report_returns_open_order_by_cloid() {
    let coid = ClientOrderId::new("O-20240101-000001");
    let cloid_hex = Cloid::from_client_order_id(coid).to_hex();

    let state = TestServerState::default();
    *state.frontend_open_orders_response.lock().await = Some(json!([{
        "coin": "BTC",
        "side": "B",
        "limitPx": "95000.0",
        "sz": "0.1",
        "oid": 111111,
        "timestamp": 1700000000000u64,
        "origSz": "0.1",
        "cloid": cloid_hex,
    }]));
    let addr = start_mock_server(state).await;

    let (mut client, _rx, cache) = create_test_execution_client(addr);
    add_test_account_to_cache(&cache, AccountId::from("HYPERLIQUID-001"));
    client.connect().await.unwrap();

    let cmd = make_status_report_cmd(Some(coid), Some(VenueOrderId::from("111111")));
    let report = client
        .generate_order_status_report(&cmd)
        .await
        .unwrap()
        .expect("cloid-open lookup should resolve the live order");

    assert_eq!(report.client_order_id, Some(coid));
    assert_eq!(report.venue_order_id, VenueOrderId::from("111111"));

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn test_generate_order_status_report_terminal_oid_fallback_returns_report() {
    // Live order no longer in frontendOpenOrders (cloid-open miss), oid fallback
    // finds the terminal record. The returned report carries the API-reported
    // cloid (as hex) on `client_order_id`; downstream Python resolver remaps
    // it to the logical identifier.
    let coid = ClientOrderId::new("O-20240101-000002");
    let cloid_hex = Cloid::from_client_order_id(coid).to_hex();

    let state = TestServerState::default();
    *state.frontend_open_orders_response.lock().await = Some(json!([]));
    *state.order_status_response.lock().await = Some(json!({
        "status": "order",
        "order": {
            "order": {
                "coin": "BTC",
                "side": "B",
                "limitPx": "95000.0",
                "sz": "0.0",
                "oid": 222222,
                "timestamp": 1700000000000u64,
                "origSz": "0.1",
                "cloid": cloid_hex,
            },
            "status": "canceled",
            "statusTimestamp": 1700001000000u64,
        }
    }));

    let addr = start_mock_server(state).await;
    let (mut client, _rx, cache) = create_test_execution_client(addr);
    add_test_account_to_cache(&cache, AccountId::from("HYPERLIQUID-001"));
    client.connect().await.unwrap();

    let cmd = make_status_report_cmd(Some(coid), Some(VenueOrderId::from("222222")));
    let report = client
        .generate_order_status_report(&cmd)
        .await
        .unwrap()
        .expect("terminal oid match should be returned");

    assert_eq!(report.order_status, OrderStatus::Canceled);
    assert_eq!(report.venue_order_id, VenueOrderId::from("222222"));
    assert_eq!(
        report.client_order_id,
        Some(ClientOrderId::new(cloid_hex.as_str())),
        "helper leaves the API-reported cloid intact for downstream resolution",
    );

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn test_generate_order_status_report_terminal_mismatched_cloid_still_returned() {
    // A cloid mismatch no longer shortcircuits the helper. The downstream
    // Python resolver uses venue_order_id to rebind the report to the
    // correct logical client_order_id, so the helper forwards the API
    // response as-is.
    let coid = ClientOrderId::new("O-20240101-000003");
    let other_coid_hex =
        Cloid::from_client_order_id(ClientOrderId::new("O-SOMETHING-ELSE")).to_hex();

    let state = TestServerState::default();
    *state.frontend_open_orders_response.lock().await = Some(json!([]));
    *state.order_status_response.lock().await = Some(json!({
        "status": "order",
        "order": {
            "order": {
                "coin": "BTC",
                "side": "B",
                "limitPx": "95000.0",
                "sz": "0.0",
                "oid": 333333,
                "timestamp": 1700000000000u64,
                "origSz": "0.1",
                "cloid": other_coid_hex,
            },
            "status": "canceled",
            "statusTimestamp": 1700001000000u64,
        }
    }));

    let addr = start_mock_server(state).await;
    let (mut client, _rx, cache) = create_test_execution_client(addr);
    add_test_account_to_cache(&cache, AccountId::from("HYPERLIQUID-001"));
    client.connect().await.unwrap();

    let cmd = make_status_report_cmd(Some(coid), Some(VenueOrderId::from("333333")));
    let report = client
        .generate_order_status_report(&cmd)
        .await
        .unwrap()
        .expect("helper must forward valid oid matches regardless of cloid");
    assert_eq!(report.order_status, OrderStatus::Canceled);
    assert_eq!(report.venue_order_id, VenueOrderId::from("333333"));

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn test_generate_order_status_report_terminal_missing_cloid_trusts_oid() {
    // Orders placed without a cloid (or external/synthetic orders the engine
    // reconciled from the venue) have no cloid on the API response. The
    // helper must still surface the oid match so downstream reconciliation
    // can resolve the logical client_order_id by venue_order_id.
    let coid = ClientOrderId::new("O-20240101-000004");

    let state = TestServerState::default();
    *state.frontend_open_orders_response.lock().await = Some(json!([]));
    *state.order_status_response.lock().await = Some(json!({
        "status": "order",
        "order": {
            "order": {
                "coin": "BTC",
                "side": "B",
                "limitPx": "95000.0",
                "sz": "0.0",
                "oid": 444444,
                "timestamp": 1700000000000u64,
                "origSz": "0.1",
            },
            "status": "filled",
            "statusTimestamp": 1700001000000u64,
        }
    }));

    let addr = start_mock_server(state).await;
    let (mut client, _rx, cache) = create_test_execution_client(addr);
    add_test_account_to_cache(&cache, AccountId::from("HYPERLIQUID-001"));
    client.connect().await.unwrap();

    let cmd = make_status_report_cmd(Some(coid), Some(VenueOrderId::from("444444")));
    let report = client
        .generate_order_status_report(&cmd)
        .await
        .unwrap()
        .expect("oid match with no cloid on response should still resolve");
    assert_eq!(report.order_status, OrderStatus::Filled);

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn test_generate_order_status_report_oid_only_returns_terminal() {
    // When only venue_order_id is supplied, the helper must still surface a
    // terminal report (no cloid validation applies without a coid to check).
    let state = TestServerState::default();
    *state.frontend_open_orders_response.lock().await = Some(json!([]));
    *state.order_status_response.lock().await = Some(json!({
        "status": "order",
        "order": {
            "order": {
                "coin": "BTC",
                "side": "B",
                "limitPx": "95000.0",
                "sz": "0.0",
                "oid": 555555,
                "timestamp": 1700000000000u64,
                "origSz": "0.1",
            },
            "status": "canceled",
            "statusTimestamp": 1700001000000u64,
        }
    }));

    let addr = start_mock_server(state).await;
    let (mut client, _rx, cache) = create_test_execution_client(addr);
    add_test_account_to_cache(&cache, AccountId::from("HYPERLIQUID-001"));
    client.connect().await.unwrap();

    let cmd = make_status_report_cmd(None, Some(VenueOrderId::from("555555")));
    let report = client
        .generate_order_status_report(&cmd)
        .await
        .unwrap()
        .expect("terminal report without cloid guard should be returned");
    assert_eq!(report.order_status, OrderStatus::Canceled);

    client.disconnect().await.unwrap();
}

fn make_cancel_entry(coid: ClientOrderId, voi: VenueOrderId) -> CancelOrder {
    CancelOrder::new(
        TraderId::from("TESTER-001"),
        Some(*HYPERLIQUID_CLIENT_ID),
        StrategyId::from("S-001"),
        InstrumentId::from(HYPERLIQUID_TEST_INSTRUMENT),
        coid,
        Some(voi),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None, // correlation_id
    )
}

async fn drain_cancel_rejected_events(
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
    timeout: Duration,
) -> Vec<(ClientOrderId, String)> {
    let mut out = Vec::new();
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        match tokio::time::timeout_at(deadline, rx.recv()).await {
            Ok(Some(ExecutionEvent::Order(event))) => {
                let msg = format!("{event:?}");

                if msg.contains("OrderCancelRejected")
                    && let Some(coid) = extract_coid(&msg)
                {
                    let reason = extract_reason(&msg).unwrap_or_default();
                    out.push((coid, reason));
                }
            }
            Ok(Some(_)) => {}
            Ok(None) | Err(_) => break,
        }
    }
    out
}

fn extract_coid(debug: &str) -> Option<ClientOrderId> {
    // Pull "client_order_id=<value>" from the event Debug output.
    let key = "client_order_id=";
    let start = debug.find(key)? + key.len();
    let tail = &debug[start..];
    let end = tail.find([',', ' ', ')']).unwrap_or(tail.len());
    Some(ClientOrderId::new(&tail[..end]))
}

fn extract_reason(debug: &str) -> Option<String> {
    let key = "reason='";
    let start = debug.find(key)? + key.len();
    let tail = &debug[start..];
    let end = tail.find('\'')?;
    Some(tail[..end].to_string())
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn test_batch_cancel_orders_per_item_error_emits_cancel_rejected() {
    // Exchange returns top-level ok but the second cancel fails inline. The
    // client must emit OrderCancelRejected for the failing entry only.
    let state = TestServerState::default();
    *state.cancel_response_override.lock().await = Some(json!({
        "status": "ok",
        "response": {
            "type": "cancel",
            "data": {
                "statuses": [
                    "success",
                    {"error": "Order was never placed, already canceled, or filled."}
                ]
            }
        }
    }));

    let addr = start_mock_server(state).await;
    let (mut client, mut rx, cache) = create_test_execution_client(addr);
    add_test_account_to_cache(&cache, AccountId::from("HYPERLIQUID-001"));
    client.start().unwrap();
    client.connect().await.unwrap();

    let ok_coid = ClientOrderId::new("O-BATCH-OK");
    let fail_coid = ClientOrderId::new("O-BATCH-FAIL");

    let batch = BatchCancelOrders::new(
        TraderId::from("TESTER-001"),
        Some(*HYPERLIQUID_CLIENT_ID),
        StrategyId::from("S-001"),
        InstrumentId::from(HYPERLIQUID_TEST_INSTRUMENT),
        vec![
            make_cancel_entry(ok_coid, VenueOrderId::from("100")),
            make_cancel_entry(fail_coid, VenueOrderId::from("101")),
        ],
        UUID4::new(),
        UnixNanos::default(),
        None,
        None, // correlation_id
    );

    client.batch_cancel_orders(batch).unwrap();

    wait_until_async(
        || async { client.pending_tasks_all_finished() },
        Duration::from_secs(5),
    )
    .await;

    let events = drain_cancel_rejected_events(&mut rx, Duration::from_millis(250)).await;
    assert_eq!(
        events.len(),
        1,
        "only the failing cancel should be rejected"
    );
    assert_eq!(events[0].0, fail_coid);
    assert!(events[0].1.contains("already canceled"));

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn test_batch_cancel_orders_short_statuses_rejects_all_sent() {
    let state = TestServerState::default();
    *state.cancel_response_override.lock().await = Some(json!({
        "status": "ok",
        "response": {
            "type": "cancel",
            "data": {
                "statuses": ["success"]
            }
        }
    }));

    let addr = start_mock_server(state).await;
    let (mut client, mut rx, cache) = create_test_execution_client(addr);
    add_test_account_to_cache(&cache, AccountId::from("HYPERLIQUID-001"));
    client.start().unwrap();
    client.connect().await.unwrap();

    let coid_a = ClientOrderId::new("O-BATCH-SHORT-A");
    let coid_b = ClientOrderId::new("O-BATCH-SHORT-B");
    let batch = BatchCancelOrders::new(
        TraderId::from("TESTER-001"),
        Some(*HYPERLIQUID_CLIENT_ID),
        StrategyId::from("S-001"),
        InstrumentId::from(HYPERLIQUID_TEST_INSTRUMENT),
        vec![
            make_cancel_entry(coid_a, VenueOrderId::from("150")),
            make_cancel_entry(coid_b, VenueOrderId::from("151")),
        ],
        UUID4::new(),
        UnixNanos::default(),
        None,
        None, // correlation_id
    );

    client.batch_cancel_orders(batch).unwrap();

    wait_until_async(
        || async { client.pending_tasks_all_finished() },
        Duration::from_secs(5),
    )
    .await;

    let events = drain_cancel_rejected_events(&mut rx, Duration::from_millis(250)).await;

    assert_eq!(events.len(), 2);
    assert!(events.iter().any(|(coid, _)| *coid == coid_a));
    assert!(events.iter().any(|(coid, _)| *coid == coid_b));
    assert!(
        events
            .iter()
            .all(|(_, reason)| reason.contains("status count mismatch"))
    );

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn test_batch_cancel_orders_post_error_rejects_all_sent() {
    // Post error: every entry that was actually dispatched must have
    // a cancel_rejected event so the engine does not wait on ghost acks.
    let state = TestServerState::default();
    state.fail_next_exchange.store(true, Ordering::Relaxed);

    let addr = start_mock_server(state).await;
    let (mut client, mut rx, cache) = create_test_execution_client(addr);
    add_test_account_to_cache(&cache, AccountId::from("HYPERLIQUID-001"));
    client.start().unwrap();
    client.connect().await.unwrap();

    let coid_a = ClientOrderId::new("O-BATCH-A");
    let coid_b = ClientOrderId::new("O-BATCH-B");
    let batch = BatchCancelOrders::new(
        TraderId::from("TESTER-001"),
        Some(*HYPERLIQUID_CLIENT_ID),
        StrategyId::from("S-001"),
        InstrumentId::from(HYPERLIQUID_TEST_INSTRUMENT),
        vec![
            make_cancel_entry(coid_a, VenueOrderId::from("200")),
            make_cancel_entry(coid_b, VenueOrderId::from("201")),
        ],
        UUID4::new(),
        UnixNanos::default(),
        None,
        None, // correlation_id
    );

    client.batch_cancel_orders(batch).unwrap();

    wait_until_async(
        || async { client.pending_tasks_all_finished() },
        Duration::from_secs(5),
    )
    .await;

    let events = drain_cancel_rejected_events(&mut rx, Duration::from_millis(250)).await;
    assert_eq!(
        events.len(),
        2,
        "every sent cancel must be rejected on post error"
    );
    let coids: std::collections::HashSet<_> = events.iter().map(|(c, _)| *c).collect();
    assert!(coids.contains(&coid_a));
    assert!(coids.contains(&coid_b));

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn test_batch_cancel_orders_missing_asset_index_skips_and_rejects() {
    // No trading action should happen for an entry whose instrument symbol
    // is unknown; the helper must emit a cancel_rejected for the skipped
    // entry and still dispatch the remaining one.
    let state = TestServerState::default();
    let addr = start_mock_server(state.clone()).await;
    let (mut client, mut rx, cache) = create_test_execution_client(addr);
    add_test_account_to_cache(&cache, AccountId::from("HYPERLIQUID-001"));
    client.start().unwrap();
    client.connect().await.unwrap();

    let known_coid = ClientOrderId::new("O-BATCH-KNOWN");
    let unknown_coid = ClientOrderId::new("O-BATCH-UNKNOWN");
    let unknown_entry = CancelOrder::new(
        TraderId::from("TESTER-001"),
        Some(*HYPERLIQUID_CLIENT_ID),
        StrategyId::from("S-001"),
        InstrumentId::from("NOPE-USD-PERP.HYPERLIQUID"),
        unknown_coid,
        Some(VenueOrderId::from("301")),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None, // correlation_id
    );

    let batch = BatchCancelOrders::new(
        TraderId::from("TESTER-001"),
        Some(*HYPERLIQUID_CLIENT_ID),
        StrategyId::from("S-001"),
        InstrumentId::from(HYPERLIQUID_TEST_INSTRUMENT),
        vec![
            make_cancel_entry(known_coid, VenueOrderId::from("300")),
            unknown_entry,
        ],
        UUID4::new(),
        UnixNanos::default(),
        None,
        None, // correlation_id
    );

    client.batch_cancel_orders(batch).unwrap();

    wait_until_async(
        || async { client.pending_tasks_all_finished() },
        Duration::from_secs(5),
    )
    .await;

    let events = drain_cancel_rejected_events(&mut rx, Duration::from_millis(250)).await;
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].0, unknown_coid);
    assert!(
        events[0].1.contains("Asset index not found"),
        "reason should explain the skip: {}",
        events[0].1,
    );

    client.disconnect().await.unwrap();
}

// Transitions a LIMIT order from INITIALIZED -> SUBMITTED -> ACCEPTED so
// the cache routes it through `orders_open`, where `cancel_all_orders`
// looks for candidates.
fn open_limit_order_in_cache(
    cache: &Rc<RefCell<Cache>>,
    client_order_id: &str,
    venue_order_id: &str,
) -> OrderAny {
    let account_id = AccountId::from("HYPERLIQUID-001");
    let mut order = make_limit_order(client_order_id);
    cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .expect("add order");

    let submitted = OrderSubmitted::new(
        order.trader_id(),
        order.strategy_id(),
        order.instrument_id(),
        order.client_order_id(),
        account_id,
        UUID4::new(),
        UnixNanos::default(),
        UnixNanos::default(),
    );
    order = cache
        .borrow_mut()
        .update_order(&OrderEventAny::Submitted(submitted))
        .expect("submitted transition");

    let accepted = OrderAccepted::new(
        order.trader_id(),
        order.strategy_id(),
        order.instrument_id(),
        order.client_order_id(),
        VenueOrderId::from(venue_order_id),
        account_id,
        UUID4::new(),
        UnixNanos::default(),
        UnixNanos::default(),
        false,
    );
    order = cache
        .borrow_mut()
        .update_order(&OrderEventAny::Accepted(accepted))
        .expect("accepted transition");

    order
}

fn make_cancel_all_cmd(instrument_id: &str, side: OrderSide) -> CancelAllOrders {
    CancelAllOrders::new(
        TraderId::from("TESTER-001"),
        Some(*HYPERLIQUID_CLIENT_ID),
        StrategyId::from("S-001"),
        InstrumentId::from(instrument_id),
        side,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None, // correlation_id
    )
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn test_cancel_all_orders_per_item_error_emits_cancel_rejected() {
    // Exchange returns top-level ok but one of the two inline cancel statuses
    // is a MissingOrder error. The exec client must emit OrderCancelRejected
    // for the failing entry only.
    let state = TestServerState::default();
    *state.cancel_response_override.lock().await = Some(json!({
        "status": "ok",
        "response": {
            "type": "cancel",
            "data": {
                "statuses": [
                    "success",
                    {"error": "Order was never placed, already canceled, or filled. MissingOrder"}
                ]
            }
        }
    }));

    let addr = start_mock_server(state).await;
    let (mut client, mut rx, cache) = create_test_execution_client(addr);
    add_test_account_to_cache(&cache, AccountId::from("HYPERLIQUID-001"));
    client.start().unwrap();
    client.connect().await.unwrap();

    let ok_order = open_limit_order_in_cache(&cache, "O-CA-OK", "700");
    let fail_order = open_limit_order_in_cache(&cache, "O-CA-FAIL", "701");

    client
        .cancel_all_orders(make_cancel_all_cmd(
            HYPERLIQUID_TEST_INSTRUMENT,
            OrderSide::Buy,
        ))
        .unwrap();

    wait_until_async(
        || async { client.pending_tasks_all_finished() },
        Duration::from_secs(5),
    )
    .await;

    let events = drain_cancel_rejected_events(&mut rx, Duration::from_millis(250)).await;
    assert_eq!(
        events.len(),
        1,
        "only the failing cancel inside the batch should be rejected",
    );

    let (rejected_coid, _) = &events[0];
    assert!(
        *rejected_coid == ok_order.client_order_id()
            || *rejected_coid == fail_order.client_order_id(),
        "rejected coid must correspond to one of the open orders",
    );

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn test_cancel_all_orders_short_statuses_rejects_every_open_order() {
    let state = TestServerState::default();
    *state.cancel_response_override.lock().await = Some(json!({
        "status": "ok",
        "response": {
            "type": "cancel",
            "data": {
                "statuses": ["success"]
            }
        }
    }));

    let addr = start_mock_server(state).await;
    let (mut client, mut rx, cache) = create_test_execution_client(addr);
    add_test_account_to_cache(&cache, AccountId::from("HYPERLIQUID-001"));
    client.start().unwrap();
    client.connect().await.unwrap();

    let order_a = open_limit_order_in_cache(&cache, "O-CA-SHORT-A", "750");
    let order_b = open_limit_order_in_cache(&cache, "O-CA-SHORT-B", "751");

    client
        .cancel_all_orders(make_cancel_all_cmd(
            HYPERLIQUID_TEST_INSTRUMENT,
            OrderSide::Buy,
        ))
        .unwrap();

    wait_until_async(
        || async { client.pending_tasks_all_finished() },
        Duration::from_secs(5),
    )
    .await;

    let events = drain_cancel_rejected_events(&mut rx, Duration::from_millis(250)).await;

    assert_eq!(events.len(), 2);
    assert!(
        events
            .iter()
            .any(|(coid, _)| *coid == order_a.client_order_id())
    );
    assert!(
        events
            .iter()
            .any(|(coid, _)| *coid == order_b.client_order_id())
    );
    assert!(
        events
            .iter()
            .all(|(_, reason)| reason.contains("status count mismatch"))
    );

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn test_cancel_all_orders_post_error_rejects_every_open_order() {
    // Post error: every order that was dispatched in the batch must
    // get a cancel_rejected event so the engine does not wait for ghost acks.
    let state = TestServerState::default();
    state.fail_next_exchange.store(true, Ordering::Relaxed);

    let addr = start_mock_server(state).await;
    let (mut client, mut rx, cache) = create_test_execution_client(addr);
    add_test_account_to_cache(&cache, AccountId::from("HYPERLIQUID-001"));
    client.start().unwrap();
    client.connect().await.unwrap();

    let a = open_limit_order_in_cache(&cache, "O-CA-A", "800");
    let b = open_limit_order_in_cache(&cache, "O-CA-B", "801");

    client
        .cancel_all_orders(make_cancel_all_cmd(
            HYPERLIQUID_TEST_INSTRUMENT,
            OrderSide::Buy,
        ))
        .unwrap();

    wait_until_async(
        || async { client.pending_tasks_all_finished() },
        Duration::from_secs(5),
    )
    .await;

    let events = drain_cancel_rejected_events(&mut rx, Duration::from_millis(250)).await;
    assert_eq!(
        events.len(),
        2,
        "every open order must be rejected on post error",
    );
    let coids: std::collections::HashSet<_> = events.iter().map(|(c, _)| *c).collect();
    assert!(coids.contains(&a.client_order_id()));
    assert!(coids.contains(&b.client_order_id()));

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn test_cancel_all_orders_missing_asset_index_rejects_all() {
    // Instrument symbol is not registered with the asset-index map, so
    // no trading action happens. Every open order must still receive a
    // cancel_rejected event with the "Asset index not found" reason.
    const UNKNOWN_INSTRUMENT: &str = "NOPE-USD-PERP.HYPERLIQUID";
    let state = TestServerState::default();
    let addr = start_mock_server(state).await;
    let (mut client, mut rx, cache) = create_test_execution_client(addr);
    add_test_account_to_cache(&cache, AccountId::from("HYPERLIQUID-001"));
    client.start().unwrap();
    client.connect().await.unwrap();

    // Build orders on the unknown instrument so orders_open returns them but
    // asset lookup fails.
    let unknown_id = InstrumentId::from(UNKNOWN_INSTRUMENT);
    let a_coid = ClientOrderId::new("O-CA-X");
    let b_coid = ClientOrderId::new("O-CA-Y");
    for (coid, voi) in [(a_coid, "900"), (b_coid, "901")] {
        let account_id = AccountId::from("HYPERLIQUID-001");
        let mut order = OrderAny::Limit(LimitOrder::new(
            TraderId::from("TESTER-001"),
            StrategyId::from("S-001"),
            unknown_id,
            coid,
            OrderSide::Buy,
            Quantity::from("0.0001"),
            Price::from("56730.0"),
            TimeInForce::Gtc,
            None,
            false,
            false,
            false,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            UUID4::new(),
            UnixNanos::default(),
        ));

        let submitted = OrderSubmitted::new(
            order.trader_id(),
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            account_id,
            UUID4::new(),
            UnixNanos::default(),
            UnixNanos::default(),
        );
        cache
            .borrow_mut()
            .add_order(order.clone(), None, None, false)
            .unwrap();
        order = cache
            .borrow_mut()
            .update_order(&OrderEventAny::Submitted(submitted))
            .unwrap();

        let accepted = OrderAccepted::new(
            order.trader_id(),
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            VenueOrderId::from(voi),
            account_id,
            UUID4::new(),
            UnixNanos::default(),
            UnixNanos::default(),
            false,
        );
        let _order = cache
            .borrow_mut()
            .update_order(&OrderEventAny::Accepted(accepted))
            .unwrap();
    }

    client
        .cancel_all_orders(make_cancel_all_cmd(UNKNOWN_INSTRUMENT, OrderSide::Buy))
        .unwrap();

    wait_until_async(
        || async { client.pending_tasks_all_finished() },
        Duration::from_secs(5),
    )
    .await;

    let events = drain_cancel_rejected_events(&mut rx, Duration::from_millis(250)).await;
    assert_eq!(events.len(), 2, "both open orders must be rejected");
    for (_, reason) in &events {
        assert!(
            reason.contains("Asset index not found"),
            "reason should explain the skip: {reason}",
        );
    }

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn test_cancel_order_missing_emits_cancel_rejected() {
    // TC-E44: cancelling an order the venue has already finalized must emit
    // `OrderCancelRejected` (not `OrderDenied`). The mock returns a per-item
    // "MissingOrder" status wrapped in a top-level ok response.
    let state = TestServerState::default();
    *state.cancel_response_override.lock().await = Some(json!({
        "status": "ok",
        "response": {
            "type": "cancel",
            "data": {
                "statuses": [
                    {"error": "Order was never placed, already canceled, or filled. MissingOrder"}
                ]
            }
        }
    }));

    let addr = start_mock_server(state).await;
    let (mut client, mut rx, cache) = create_test_execution_client(addr);
    add_test_account_to_cache(&cache, AccountId::from("HYPERLIQUID-001"));
    client.start().unwrap();
    client.connect().await.unwrap();

    let coid = ClientOrderId::new("O-CANCEL-GONE");
    let cmd = CancelOrder::new(
        TraderId::from("TESTER-001"),
        Some(*HYPERLIQUID_CLIENT_ID),
        StrategyId::from("S-001"),
        InstrumentId::from(HYPERLIQUID_TEST_INSTRUMENT),
        coid,
        Some(VenueOrderId::from("777")),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None, // correlation_id
    );

    client.cancel_order(cmd).unwrap();

    wait_until_async(
        || async { client.pending_tasks_all_finished() },
        Duration::from_secs(5),
    )
    .await;

    let events = drain_cancel_rejected_events(&mut rx, Duration::from_millis(250)).await;
    assert_eq!(
        events.len(),
        1,
        "a MissingOrder cancel must emit exactly one OrderCancelRejected event",
    );
    assert_eq!(events[0].0, coid);
    assert!(
        events[0].1.to_lowercase().contains("missingorder")
            || events[0].1.contains("already canceled"),
        "reason should explain why the cancel failed: {}",
        events[0].1,
    );

    client.disconnect().await.unwrap();
}

async fn drain_order_status_reports(
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
    timeout: Duration,
) -> Vec<OrderStatusReport> {
    let mut out = Vec::new();
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        match tokio::time::timeout_at(deadline, rx.recv()).await {
            Ok(Some(ExecutionEvent::Report(ExecutionReport::Order(report)))) => out.push(*report),
            Ok(Some(_)) => {}
            Ok(None) | Err(_) => break,
        }
    }
    out
}

fn make_query_order_cmd(
    client_order_id: ClientOrderId,
    venue_order_id: Option<VenueOrderId>,
) -> QueryOrder {
    QueryOrder::new(
        TraderId::from("TESTER-001"),
        Some(*HYPERLIQUID_CLIENT_ID),
        StrategyId::from("S-001"),
        InstrumentId::from(HYPERLIQUID_TEST_INSTRUMENT),
        client_order_id,
        venue_order_id,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None, // correlation_id
    )
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn test_query_order_emits_report_from_cloid_open_match() {
    // cloid-open-query hits: the cached oid is authoritative and the handler
    // must forward the live report to the engine via send_order_status_report.
    let coid = ClientOrderId::new("O-QUERY-001");
    let cloid_hex = Cloid::from_client_order_id(coid).to_hex();

    let state = TestServerState::default();
    *state.frontend_open_orders_response.lock().await = Some(json!([{
        "coin": "BTC",
        "side": "B",
        "limitPx": "95000.0",
        "sz": "0.001",
        "oid": 900001,
        "timestamp": 1700000000000u64,
        "origSz": "0.001",
        "cloid": cloid_hex,
    }]));

    let addr = start_mock_server(state).await;
    let (mut client, mut rx, cache) = create_test_execution_client(addr);
    add_test_account_to_cache(&cache, AccountId::from("HYPERLIQUID-001"));
    client.start().unwrap();
    client.connect().await.unwrap();

    client
        .query_order(make_query_order_cmd(
            coid,
            Some(VenueOrderId::from("900001")),
        ))
        .unwrap();

    wait_until_async(
        || async { client.pending_tasks_all_finished() },
        Duration::from_secs(5),
    )
    .await;

    let reports = drain_order_status_reports(&mut rx, Duration::from_millis(250)).await;
    assert_eq!(
        reports.len(),
        1,
        "cloid-open match should emit exactly one report"
    );
    assert_eq!(reports[0].venue_order_id, VenueOrderId::from("900001"));
    assert_eq!(reports[0].order_status, OrderStatus::Accepted);

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn test_query_order_falls_back_to_oid_when_cloid_misses() {
    // cloid-open miss: handler must fall through to info_order_status and
    // forward any terminal report it finds to the engine.
    let coid = ClientOrderId::new("O-QUERY-002");

    let state = TestServerState::default();
    *state.frontend_open_orders_response.lock().await = Some(json!([]));
    *state.order_status_response.lock().await = Some(json!({
        "status": "order",
        "order": {
            "order": {
                "coin": "BTC",
                "side": "B",
                "limitPx": "95000.0",
                "sz": "0.0",
                "oid": 900002,
                "timestamp": 1700000000000u64,
                "origSz": "0.001",
            },
            "status": "canceled",
            "statusTimestamp": 1700001000000u64,
        }
    }));

    let addr = start_mock_server(state).await;
    let (mut client, mut rx, cache) = create_test_execution_client(addr);
    add_test_account_to_cache(&cache, AccountId::from("HYPERLIQUID-001"));
    client.start().unwrap();
    client.connect().await.unwrap();

    client
        .query_order(make_query_order_cmd(
            coid,
            Some(VenueOrderId::from("900002")),
        ))
        .unwrap();

    wait_until_async(
        || async { client.pending_tasks_all_finished() },
        Duration::from_secs(5),
    )
    .await;

    let reports = drain_order_status_reports(&mut rx, Duration::from_millis(250)).await;
    assert_eq!(
        reports.len(),
        1,
        "oid fallback should emit exactly one report"
    );
    assert_eq!(reports[0].venue_order_id, VenueOrderId::from("900002"));
    assert_eq!(reports[0].order_status, OrderStatus::Canceled);

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn test_query_order_oid_fallback_runs_when_cloid_request_errors() {
    // Sustained frontendOpenOrders outage: both the cloid-open probe and the
    // frontendOpenOrders call inside request_order_status_report must fail,
    // so the handler + HTTP helper must both tolerate the outage and still
    // resolve the order via info_order_status.
    let coid = ClientOrderId::new("O-QUERY-003");

    let state = TestServerState::default();
    // Fail enough times to cover both frontendOpenOrders requests made during
    // this query (cloid lookup + oid fallback's own prefetch).
    state
        .fail_frontend_open_orders_count
        .store(4, Ordering::Relaxed);
    *state.order_status_response.lock().await = Some(json!({
        "status": "order",
        "order": {
            "order": {
                "coin": "BTC",
                "side": "B",
                "limitPx": "95000.0",
                "sz": "0.0",
                "oid": 900003,
                "timestamp": 1700000000000u64,
                "origSz": "0.001",
            },
            "status": "filled",
            "statusTimestamp": 1700001000000u64,
        }
    }));

    let addr = start_mock_server(state).await;
    let (mut client, mut rx, cache) = create_test_execution_client(addr);
    add_test_account_to_cache(&cache, AccountId::from("HYPERLIQUID-001"));
    client.start().unwrap();
    client.connect().await.unwrap();

    client
        .query_order(make_query_order_cmd(
            coid,
            Some(VenueOrderId::from("900003")),
        ))
        .unwrap();

    wait_until_async(
        || async { client.pending_tasks_all_finished() },
        Duration::from_secs(5),
    )
    .await;

    let reports = drain_order_status_reports(&mut rx, Duration::from_millis(250)).await;
    assert_eq!(
        reports.len(),
        1,
        "cloid transport error must not abort the oid fallback",
    );
    assert_eq!(reports[0].order_status, OrderStatus::Filled);

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn test_query_order_cloid_only_without_cached_voi() {
    // Command carries no venue_order_id and the cache has no mapping either.
    // The handler must still run the cloid-open probe and forward any hit.
    let coid = ClientOrderId::new("O-QUERY-004");
    let cloid_hex = Cloid::from_client_order_id(coid).to_hex();

    let state = TestServerState::default();
    *state.frontend_open_orders_response.lock().await = Some(json!([{
        "coin": "BTC",
        "side": "B",
        "limitPx": "95000.0",
        "sz": "0.001",
        "oid": 900004,
        "timestamp": 1700000000000u64,
        "origSz": "0.001",
        "cloid": cloid_hex,
    }]));

    let addr = start_mock_server(state).await;
    let (mut client, mut rx, cache) = create_test_execution_client(addr);
    add_test_account_to_cache(&cache, AccountId::from("HYPERLIQUID-001"));
    client.start().unwrap();
    client.connect().await.unwrap();

    client
        .query_order(make_query_order_cmd(coid, None))
        .unwrap();

    wait_until_async(
        || async { client.pending_tasks_all_finished() },
        Duration::from_secs(5),
    )
    .await;

    let reports = drain_order_status_reports(&mut rx, Duration::from_millis(250)).await;
    assert_eq!(
        reports.len(),
        1,
        "cloid-only query must still resolve via frontendOpenOrders",
    );
    assert_eq!(reports[0].venue_order_id, VenueOrderId::from("900004"));

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn test_query_order_unknown_returns_silently() {
    // Order is gone from both open set and orderStatus; the handler must
    // log and emit nothing.
    let coid = ClientOrderId::new("O-QUERY-005");

    let state = TestServerState::default();
    *state.frontend_open_orders_response.lock().await = Some(json!([]));
    *state.order_status_response.lock().await = Some(json!({"status": "unknownOid"}));

    let addr = start_mock_server(state).await;
    let (mut client, mut rx, cache) = create_test_execution_client(addr);
    add_test_account_to_cache(&cache, AccountId::from("HYPERLIQUID-001"));
    client.start().unwrap();
    client.connect().await.unwrap();

    client
        .query_order(make_query_order_cmd(
            coid,
            Some(VenueOrderId::from("900005")),
        ))
        .unwrap();

    wait_until_async(
        || async { client.pending_tasks_all_finished() },
        Duration::from_secs(5),
    )
    .await;

    let reports = drain_order_status_reports(&mut rx, Duration::from_millis(250)).await;
    assert!(
        reports.is_empty(),
        "unknownOid must not emit an order status report",
    );

    client.disconnect().await.unwrap();
}

async fn drain_denied_events(
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
    timeout: Duration,
) -> Vec<(ClientOrderId, String)> {
    let mut out = Vec::new();
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        match tokio::time::timeout_at(deadline, rx.recv()).await {
            Ok(Some(ExecutionEvent::Order(OrderEventAny::Denied(denied)))) => {
                out.push((denied.client_order_id, denied.reason.to_string()));
            }
            Ok(Some(_)) => {}
            Ok(None) | Err(_) => break,
        }
    }
    out
}

async fn drain_rejected_events(
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
    timeout: Duration,
) -> Vec<(ClientOrderId, String)> {
    let mut out = Vec::new();
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        match tokio::time::timeout_at(deadline, rx.recv()).await {
            Ok(Some(ExecutionEvent::Order(OrderEventAny::Rejected(rejected)))) => {
                out.push((rejected.client_order_id, rejected.reason.to_string()));
            }
            Ok(Some(_)) => {}
            Ok(None) | Err(_) => break,
        }
    }
    out
}

async fn drain_modify_rejected_events(
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
    timeout: Duration,
) -> Vec<(ClientOrderId, String)> {
    let mut out = Vec::new();
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        match tokio::time::timeout_at(deadline, rx.recv()).await {
            Ok(Some(ExecutionEvent::Order(OrderEventAny::ModifyRejected(rejected)))) => {
                out.push((rejected.client_order_id, rejected.reason.to_string()));
            }
            Ok(Some(_)) => {}
            Ok(None) | Err(_) => break,
        }
    }
    out
}

fn make_limit_order_on_instrument(id: &str, instrument_id: InstrumentId) -> OrderAny {
    OrderAny::Limit(LimitOrder::new(
        TraderId::from("TESTER-001"),
        StrategyId::from("S-001"),
        instrument_id,
        ClientOrderId::from(id),
        OrderSide::Buy,
        Quantity::from("0.0001"),
        Price::from("56730.0"),
        TimeInForce::Gtc,
        None,
        false,
        false,
        false,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        UUID4::new(),
        UnixNanos::default(),
    ))
}

fn make_market_order(id: &str) -> OrderAny {
    OrderAny::Market(MarketOrder::new(
        TraderId::from("TESTER-001"),
        StrategyId::from("S-001"),
        InstrumentId::from(HYPERLIQUID_TEST_INSTRUMENT),
        ClientOrderId::from(id),
        OrderSide::Buy,
        Quantity::from("0.0001"),
        TimeInForce::Gtc,
        UUID4::new(),
        UnixNanos::default(),
        false,
        false,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
    ))
}

fn make_submit_cmd(order: &OrderAny) -> SubmitOrder {
    SubmitOrder::from_order(
        order,
        order.trader_id(),
        Some(*HYPERLIQUID_CLIENT_ID),
        None,
        UUID4::new(),
        UnixNanos::default(),
    )
}

fn make_submit_cmd_with_params(order: &OrderAny, params: Params) -> SubmitOrder {
    SubmitOrder::new(
        order.trader_id(),
        Some(*HYPERLIQUID_CLIENT_ID),
        order.strategy_id(),
        order.instrument_id(),
        order.client_order_id(),
        OrderInitialized::from(order),
        order.exec_algorithm_id(),
        None,
        Some(params),
        UUID4::new(),
        UnixNanos::default(),
        None, // correlation_id
    )
}

#[rstest]
#[case::unsupported_symbol("BTC-USD-FUT.HYPERLIQUID", "Unsupported instrument symbol format")]
#[tokio::test(flavor = "multi_thread")]
async fn test_submit_order_unsupported_symbol_emits_denied(
    #[case] instrument_str: &str,
    #[case] reason_substr: &str,
) {
    // Symbol does not match -PERP, -SPOT, or HIP-4 outcome wire forms, so
    // `validate_order_submission` bails before any trading action and the client
    // emits OrderDenied. A regression that drops the suffix check would land
    // orders on a venue that cannot route them.
    let state = TestServerState::default();
    let exchange_count = state.exchange_request_count.clone();
    let addr = start_mock_server(state).await;
    let (mut client, mut rx, cache) = create_test_execution_client(addr);
    add_test_account_to_cache(&cache, AccountId::from("HYPERLIQUID-001"));
    client.start().unwrap();
    client.connect().await.unwrap();

    let order = make_limit_order_on_instrument("O-BAD-SYMBOL", InstrumentId::from(instrument_str));
    cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .unwrap();

    let result = client.submit_order(make_submit_cmd(&order));
    assert!(
        result.is_err(),
        "validate_order_submission should bubble up"
    );

    let events = drain_denied_events(&mut rx, Duration::from_millis(250)).await;
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].0, order.client_order_id());
    assert!(
        events[0].1.contains(reason_substr),
        "reason: {}",
        events[0].1,
    );
    assert_eq!(*exchange_count.lock().await, 0);

    client.disconnect().await.unwrap();
}

fn make_outcome_limit_order(id: &str, reduce_only: bool) -> OrderAny {
    OrderAny::Limit(LimitOrder::new(
        TraderId::from("TESTER-001"),
        StrategyId::from("S-001"),
        InstrumentId::from("+50.HYPERLIQUID"),
        ClientOrderId::from(id),
        OrderSide::Buy,
        Quantity::from("1"),
        Price::from("0.5000"),
        TimeInForce::Gtc,
        None,
        false,
        reduce_only,
        false,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        UUID4::new(),
        UnixNanos::default(),
    ))
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn test_submit_order_list_denies_outcome_reduce_only() {
    // submit_order_list runs validate_order_for_hyperliquid before asset
    // resolution. An outcome order with reduce_only=true must be denied
    // without any exchange request being sent. A regression that drops
    // the validation gate from the list path would route the request to
    // the venue and rely on the venue to reject it.
    let state = TestServerState::default();
    let exchange_count = state.exchange_request_count.clone();
    let addr = start_mock_server(state).await;
    let (mut client, mut rx, cache) = create_test_execution_client(addr);
    add_test_account_to_cache(&cache, AccountId::from("HYPERLIQUID-001"));
    client.start().unwrap();
    client.connect().await.unwrap();

    let trader_id = TraderId::from("TESTER-001");
    let strategy_id = StrategyId::from("S-001");
    let instrument_id = InstrumentId::from("+50.HYPERLIQUID");
    let cid = ClientOrderId::new("O-OUTCOME-LIST-RO");

    let order = make_outcome_limit_order(cid.as_str(), true);
    let init = order.init_event().clone();
    cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .unwrap();

    let order_list = OrderList::new(
        OrderListId::from("outcome-ro-list"),
        instrument_id,
        strategy_id,
        vec![cid],
        UnixNanos::default(),
    );

    let cmd = SubmitOrderList::new(
        trader_id,
        Some(*HYPERLIQUID_CLIENT_ID),
        strategy_id,
        order_list,
        vec![init],
        None,
        None,
        None,
        UUID4::new(),
        UnixNanos::default(),
        None, // correlation_id
    );

    client.submit_order_list(cmd).unwrap();

    wait_until_async(
        || async { client.pending_tasks_all_finished() },
        Duration::from_secs(2),
    )
    .await;

    let denied = drain_denied_events(&mut rx, Duration::from_millis(250)).await;
    assert_eq!(denied.len(), 1, "outcome reduce_only must be denied");
    assert_eq!(denied[0].0, cid);
    assert!(
        denied[0].1.contains("Reduce-only is not supported"),
        "reason: {}",
        denied[0].1,
    );
    assert_eq!(
        *exchange_count.lock().await,
        0,
        "no trading action should reach the venue",
    );

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn test_submit_order_closed_order_returns_silently() {
    // Submitting an already-closed order is a no-op: no events, no trading action. A
    // regression that drops the `is_closed` early-return would re-submit
    // canceled or filled orders.
    let state = TestServerState::default();
    let exchange_count = state.exchange_request_count.clone();
    let addr = start_mock_server(state).await;
    let (mut client, mut rx, cache) = create_test_execution_client(addr);
    add_test_account_to_cache(&cache, AccountId::from("HYPERLIQUID-001"));
    // start() wires the emitter sender; without it, drain_denied_events would
    // be vacuous because no event could ever reach `rx`.
    client.start().unwrap();
    client.connect().await.unwrap();

    let mut order = make_limit_order("O-CLOSED");
    let account_id = AccountId::from("HYPERLIQUID-001");
    let submitted = OrderSubmitted::new(
        order.trader_id(),
        order.strategy_id(),
        order.instrument_id(),
        order.client_order_id(),
        account_id,
        UUID4::new(),
        UnixNanos::default(),
        UnixNanos::default(),
    );
    order.apply(OrderEventAny::Submitted(submitted)).unwrap();
    let accepted = OrderAccepted::new(
        order.trader_id(),
        order.strategy_id(),
        order.instrument_id(),
        order.client_order_id(),
        VenueOrderId::from("123"),
        account_id,
        UUID4::new(),
        UnixNanos::default(),
        UnixNanos::default(),
        false,
    );
    order.apply(OrderEventAny::Accepted(accepted)).unwrap();
    let filled = OrderFilled::new(
        order.trader_id(),
        order.strategy_id(),
        order.instrument_id(),
        order.client_order_id(),
        VenueOrderId::from("123"),
        account_id,
        TradeId::new("trade-1"),
        order.order_side(),
        OrderType::Limit,
        order.quantity(),
        Price::from("56730.0"),
        Currency::USD(),
        LiquiditySide::Taker,
        UUID4::new(),
        UnixNanos::default(),
        UnixNanos::default(),
        false,
        None,
        Some(Money::new(0.0, Currency::USD())),
    );
    order.apply(OrderEventAny::Filled(filled)).unwrap();
    assert!(order.is_closed(), "order should be terminal before submit");

    cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .unwrap();

    client.submit_order(make_submit_cmd(&order)).unwrap();

    let events = drain_denied_events(&mut rx, Duration::from_millis(250)).await;
    assert!(events.is_empty(), "closed order must not emit any denial");
    assert_eq!(*exchange_count.lock().await, 0);

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn test_submit_order_asset_index_missing_emits_denied() {
    // Order on an instrument the asset-index map does not know about.
    // `submit_order` must emit OrderDenied with the specific reason and not
    // dispatch to the venue.
    let state = TestServerState::default();
    let exchange_count = state.exchange_request_count.clone();
    let addr = start_mock_server(state).await;
    let (mut client, mut rx, cache) = create_test_execution_client(addr);
    add_test_account_to_cache(&cache, AccountId::from("HYPERLIQUID-001"));
    client.start().unwrap();
    client.connect().await.unwrap();

    let order = make_limit_order_on_instrument(
        "O-NO-ASSET",
        InstrumentId::from("NOPE-USD-PERP.HYPERLIQUID"),
    );
    cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .unwrap();

    client.submit_order(make_submit_cmd(&order)).unwrap();

    let events = drain_denied_events(&mut rx, Duration::from_millis(250)).await;
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].0, order.client_order_id());
    assert!(
        events[0].1.contains("Asset index not found"),
        "reason: {}",
        events[0].1,
    );
    assert_eq!(*exchange_count.lock().await, 0);

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn test_submit_order_market_no_quote_emits_denied() {
    // A MARKET order requires a cached quote to derive the slippage-adjusted
    // limit price. Without one, the client must deny so the strategy gets a
    // clear "subscribe to quote data" signal instead of a silent failure.
    let state = TestServerState::default();
    let exchange_count = state.exchange_request_count.clone();
    let addr = start_mock_server(state).await;
    let (mut client, mut rx, cache) = create_test_execution_client(addr);
    add_test_account_to_cache(&cache, AccountId::from("HYPERLIQUID-001"));
    client.start().unwrap();
    client.connect().await.unwrap();

    let order = make_market_order("O-MARKET-NO-QUOTE");
    cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .unwrap();

    client.submit_order(make_submit_cmd(&order)).unwrap();

    let events = drain_denied_events(&mut rx, Duration::from_millis(250)).await;
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].0, order.client_order_id());
    assert!(
        events[0].1.contains("subscribe to quote data"),
        "reason: {}",
        events[0].1,
    );
    assert_eq!(*exchange_count.lock().await, 0);

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn test_submit_market_order_uses_resolved_slippage_from_params() {
    // Submitting a MARKET order with `Params{"market_order_slippage_bps": 50}`
    // overrides the config default (50 in test config; pick a value clearly
    // distinct so the action's price reflects the override). The action sent
    // to the mock should derive the limit price from the cached quote using
    // the overridden bps. This closes the only weak branch in
    // `resolve_slippage_bps`.
    let state = TestServerState::default();
    let last_action = state.last_exchange_action.clone();
    let addr = start_mock_server(state).await;
    let (mut client, _rx, cache) = create_test_execution_client(addr);
    add_test_account_to_cache(&cache, AccountId::from("HYPERLIQUID-001"));
    client.connect().await.unwrap();

    let instrument_id = InstrumentId::from(HYPERLIQUID_TEST_INSTRUMENT);
    let bid = Price::from("50000.0");
    let ask = Price::from("50100.0");
    let quote = QuoteTick::new(
        instrument_id,
        bid,
        ask,
        Quantity::from("1.0"),
        Quantity::from("1.0"),
        UnixNanos::default(),
        UnixNanos::default(),
    );
    cache.borrow_mut().add_quote(quote).unwrap();

    let order = make_market_order("O-MARKET-PARAMS");
    cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .unwrap();

    let mut params = Params::new();
    params.insert("market_order_slippage_bps".to_string(), json!(2000u64));
    client
        .submit_order(make_submit_cmd_with_params(&order, params))
        .unwrap();

    wait_until_async(
        || async { client.pending_tasks_all_finished() },
        Duration::from_secs(5),
    )
    .await;

    let action = last_action
        .lock()
        .await
        .clone()
        .expect("an order action should have been sent");
    let price = action["orders"][0]["p"]
        .as_str()
        .expect("price field on order action")
        .parse::<f64>()
        .expect("parse price");

    // Buy with 2000 bps slippage on ask 50100 produces a limit far above ask.
    // 50000 (default 50 bps) would be ~50125; 2000 bps yields ~61122 after
    // rounding. The exact value depends on price-precision rounding, but it
    // must be substantially higher than the 50-bps default.
    assert!(
        price > 55_000.0,
        "params override should produce a much wider limit; got {price}",
    );

    client.disconnect().await.unwrap();
}

fn open_limit_order_with_filled_qty(
    cache: &Rc<RefCell<Cache>>,
    client_order_id: &str,
    venue_order_id: &str,
    filled_qty: Quantity,
) -> OrderAny {
    let account_id = AccountId::from("HYPERLIQUID-001");
    let mut order = make_limit_order(client_order_id);
    cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .unwrap();

    let submitted = OrderSubmitted::new(
        order.trader_id(),
        order.strategy_id(),
        order.instrument_id(),
        order.client_order_id(),
        account_id,
        UUID4::new(),
        UnixNanos::default(),
        UnixNanos::default(),
    );
    order = cache
        .borrow_mut()
        .update_order(&OrderEventAny::Submitted(submitted))
        .expect("submitted transition");

    let accepted = OrderAccepted::new(
        order.trader_id(),
        order.strategy_id(),
        order.instrument_id(),
        order.client_order_id(),
        VenueOrderId::from(venue_order_id),
        account_id,
        UUID4::new(),
        UnixNanos::default(),
        UnixNanos::default(),
        false,
    );
    order = cache
        .borrow_mut()
        .update_order(&OrderEventAny::Accepted(accepted))
        .expect("accepted transition");

    if filled_qty.raw > 0 {
        let filled = OrderFilled::new(
            order.trader_id(),
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            VenueOrderId::from(venue_order_id),
            account_id,
            TradeId::new("trade-partial"),
            order.order_side(),
            OrderType::Limit,
            filled_qty,
            Price::from("56730.0"),
            Currency::USD(),
            LiquiditySide::Taker,
            UUID4::new(),
            UnixNanos::default(),
            UnixNanos::default(),
            false,
            None,
            Some(Money::new(0.0, Currency::USD())),
        );
        order = cache
            .borrow_mut()
            .update_order(&OrderEventAny::Filled(filled))
            .unwrap();
    }
    order
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn test_modify_order_qty_at_filled_emits_modify_rejected() {
    // Modify command with `target_total <= filled` must be rejected without
    // dispatching any trading action. A regression that drops the guard would amend a
    // venue order to `quantity = 0`, which the venue interprets as a cancel
    // and would race the legitimate modify path.
    let state = TestServerState::default();
    let exchange_count = state.exchange_request_count.clone();
    let addr = start_mock_server(state).await;
    let (mut client, mut rx, cache) = create_test_execution_client(addr);
    add_test_account_to_cache(&cache, AccountId::from("HYPERLIQUID-001"));
    client.start().unwrap();
    client.connect().await.unwrap();

    let voi = VenueOrderId::from("44444");
    let order = open_limit_order_with_filled_qty(
        &cache,
        "O-MOD-AT-FILLED",
        voi.as_str(),
        Quantity::from("0.0001"),
    );

    let cmd = ModifyOrder::new(
        order.trader_id(),
        Some(*HYPERLIQUID_CLIENT_ID),
        order.strategy_id(),
        order.instrument_id(),
        order.client_order_id(),
        Some(voi),
        Some(Quantity::from("0.0001")),
        Some(Price::from("56800.0")),
        None,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None, // correlation_id
    );

    client.modify_order(cmd).unwrap();

    let events = drain_modify_rejected_events(&mut rx, Duration::from_millis(250)).await;
    assert_eq!(events.len(), 1, "expected one OrderModifyRejected");
    assert_eq!(events[0].0, order.client_order_id());
    assert!(
        events[0].1.contains("not greater than filled"),
        "reason: {}",
        events[0].1,
    );

    assert_eq!(*exchange_count.lock().await, 0);
    assert!(
        client
            .ws_dispatch_state()
            .pending_modify(&order.client_order_id())
            .is_none(),
    );

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn test_modify_order_subtracts_filled_from_target_total() {
    // Modify on a partially filled order must send `target_total - filled` to
    // the venue. A regression that skips the subtraction would re-open the
    // entire target quantity and double-count the already-filled portion.
    let state = TestServerState::default();
    let last_action = state.last_exchange_action.clone();
    let addr = start_mock_server(state).await;
    let (mut client, _rx, cache) = create_test_execution_client(addr);
    add_test_account_to_cache(&cache, AccountId::from("HYPERLIQUID-001"));
    client.connect().await.unwrap();

    let voi = VenueOrderId::from("55001");
    // Order quantity is 0.0001; partially fill 0.00006 so 0.00004 remains
    // open. With target_total = 0.00009, modify should send 0.00003.
    let order = open_limit_order_with_filled_qty(
        &cache,
        "O-MOD-SUBTRACT",
        voi.as_str(),
        Quantity::from("0.00006"),
    );

    let cmd = ModifyOrder::new(
        order.trader_id(),
        Some(*HYPERLIQUID_CLIENT_ID),
        order.strategy_id(),
        order.instrument_id(),
        order.client_order_id(),
        Some(voi),
        Some(Quantity::from("0.00009")),
        Some(Price::from("56800.0")),
        None,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None, // correlation_id
    );
    client.modify_order(cmd).unwrap();

    wait_until_async(
        || async { client.pending_tasks_all_finished() },
        Duration::from_secs(5),
    )
    .await;

    let action = last_action
        .lock()
        .await
        .clone()
        .expect("modify action should have been sent");
    assert_eq!(action.get("type").and_then(|t| t.as_str()), Some("modify"));
    let size_str = action["order"]["s"]
        .as_str()
        .expect("size field on modify action");
    let size: f64 = size_str.parse().unwrap();
    assert!(
        (size - 0.00003).abs() < 1e-9,
        "modify size must be target_total - filled = 0.00003; got {size_str}",
    );

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn test_submit_order_list_per_order_inner_error_rejects_only_failing() {
    // Two-order list where the venue returns one status per order: the first
    // is a success and the second carries an inline `error`. The execution
    // client must emit OrderRejected for the failing entry only and leave
    // the successful order's identity in place.
    let state = TestServerState::default();
    *state.order_response_override.lock().await = Some(json!({
        "status": "ok",
        "response": {
            "type": "order",
            "data": {
                "statuses": [
                    {"resting": {"oid": 70001u64}},
                    {"error": "Order rejected: insufficient margin"},
                ]
            }
        }
    }));
    let addr = start_mock_server(state).await;
    let (mut client, mut rx, cache) = create_test_execution_client(addr);
    add_test_account_to_cache(&cache, AccountId::from("HYPERLIQUID-001"));
    client.start().unwrap();
    client.connect().await.unwrap();

    let trader_id = TraderId::from("TESTER-001");
    let strategy_id = StrategyId::from("S-001");
    let instrument_id = InstrumentId::from(HYPERLIQUID_TEST_INSTRUMENT);
    let cid_a = ClientOrderId::new("O-LIST-A");
    let cid_b = ClientOrderId::new("O-LIST-B");

    let order_a = make_limit_order(cid_a.as_str());
    let order_b = make_limit_order(cid_b.as_str());
    let init_a = order_a.init_event().clone();
    let init_b = order_b.init_event().clone();

    cache
        .borrow_mut()
        .add_order(order_a.clone(), None, None, false)
        .unwrap();
    cache
        .borrow_mut()
        .add_order(order_b.clone(), None, None, false)
        .unwrap();

    let order_list = OrderList::new(
        OrderListId::from("test-list-1"),
        instrument_id,
        strategy_id,
        vec![cid_a, cid_b],
        UnixNanos::default(),
    );

    let cmd = SubmitOrderList::new(
        trader_id,
        Some(*HYPERLIQUID_CLIENT_ID),
        strategy_id,
        order_list,
        vec![init_a, init_b],
        None,
        None,
        None,
        UUID4::new(),
        UnixNanos::default(),
        None, // correlation_id
    );

    client.submit_order_list(cmd).unwrap();

    wait_until_async(
        || async { client.pending_tasks_all_finished() },
        Duration::from_secs(5),
    )
    .await;

    let rejected = drain_rejected_events(&mut rx, Duration::from_millis(250)).await;
    assert_eq!(
        rejected.len(),
        1,
        "only the failing order should be rejected"
    );
    assert_eq!(rejected[0].0, cid_b);
    assert!(
        rejected[0].1.contains("insufficient margin"),
        "reason: {}",
        rejected[0].1,
    );

    // Successful leg keeps its identity; failed leg is cleaned up.
    assert!(
        client.ws_dispatch_state().lookup_identity(&cid_a).is_some(),
        "successful order identity must remain",
    );
    assert!(
        client.ws_dispatch_state().lookup_identity(&cid_b).is_none(),
        "failed order identity must be cleaned up",
    );

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn test_submit_order_list_grouped_error_broadcast_to_all() {
    // When the venue returns fewer statuses than orders (NormalTpsl/PositionTpsl
    // grouping behavior) and one carries an error, the client must broadcast
    // OrderRejected to every order in the group and clean up all dispatch
    // identities and cloid mappings.
    let state = TestServerState::default();
    state.inner_order_error_next.store(true, Ordering::Relaxed);
    let addr = start_mock_server(state).await;
    let (mut client, mut rx, cache) = create_test_execution_client(addr);
    add_test_account_to_cache(&cache, AccountId::from("HYPERLIQUID-001"));
    client.start().unwrap();
    client.connect().await.unwrap();

    let trader_id = TraderId::from("TESTER-001");
    let strategy_id = StrategyId::from("S-001");
    let instrument_id = InstrumentId::from("ETH-USD-PERP.HYPERLIQUID");
    let cid_p = ClientOrderId::new("O-GRP-P");
    let cid_tp = ClientOrderId::new("O-GRP-TP");
    let cid_sl = ClientOrderId::new("O-GRP-SL");

    let parent = OrderAny::Limit(LimitOrder::new(
        trader_id,
        strategy_id,
        instrument_id,
        cid_p,
        OrderSide::Buy,
        Quantity::from(1),
        Price::from("3000.00"),
        TimeInForce::Gtc,
        None,
        false,
        false,
        false,
        None,
        None,
        None,
        Some(ContingencyType::Oto),
        None,
        Some(vec![cid_tp, cid_sl]),
        None,
        None,
        None,
        None,
        None,
        UUID4::new(),
        UnixNanos::default(),
    ));
    let take_profit = OrderAny::Limit(LimitOrder::new(
        trader_id,
        strategy_id,
        instrument_id,
        cid_tp,
        OrderSide::Sell,
        Quantity::from(1),
        Price::from("3200.00"),
        TimeInForce::Gtc,
        None,
        false,
        true,
        false,
        None,
        None,
        None,
        Some(ContingencyType::Oco),
        None,
        Some(vec![cid_sl]),
        Some(cid_p),
        None,
        None,
        None,
        None,
        UUID4::new(),
        UnixNanos::default(),
    ));
    let stop_loss = OrderAny::StopMarket(StopMarketOrder::new(
        trader_id,
        strategy_id,
        instrument_id,
        cid_sl,
        OrderSide::Sell,
        Quantity::from(1),
        Price::from("2800.00"),
        TriggerType::LastPrice,
        TimeInForce::Gtc,
        None,
        true,
        false,
        None,
        None,
        None,
        Some(ContingencyType::Oco),
        None,
        Some(vec![cid_tp]),
        Some(cid_p),
        None,
        None,
        None,
        None,
        UUID4::new(),
        UnixNanos::default(),
    ));

    let init_p = parent.init_event().clone();
    let init_tp = take_profit.init_event().clone();
    let init_sl = stop_loss.init_event().clone();

    cache
        .borrow_mut()
        .add_order(parent.clone(), None, None, false)
        .unwrap();
    cache
        .borrow_mut()
        .add_order(take_profit.clone(), None, None, false)
        .unwrap();
    cache
        .borrow_mut()
        .add_order(stop_loss.clone(), None, None, false)
        .unwrap();

    let order_list = OrderList::new(
        OrderListId::from("bracket-1"),
        instrument_id,
        strategy_id,
        vec![cid_p, cid_tp, cid_sl],
        UnixNanos::default(),
    );

    let cmd = SubmitOrderList::new(
        trader_id,
        Some(*HYPERLIQUID_CLIENT_ID),
        strategy_id,
        order_list,
        vec![init_p, init_tp, init_sl],
        None,
        None,
        None,
        UUID4::new(),
        UnixNanos::default(),
        None, // correlation_id
    );

    client.submit_order_list(cmd).unwrap();

    wait_until_async(
        || async { client.pending_tasks_all_finished() },
        Duration::from_secs(5),
    )
    .await;

    let rejected = drain_rejected_events(&mut rx, Duration::from_millis(250)).await;
    let cids: std::collections::HashSet<_> = rejected.iter().map(|(c, _)| *c).collect();
    assert!(
        cids.contains(&cid_p) && cids.contains(&cid_tp) && cids.contains(&cid_sl),
        "every order in the group must be rejected; got {cids:?}",
    );

    for cid in [cid_p, cid_tp, cid_sl] {
        assert!(
            client.ws_dispatch_state().lookup_identity(&cid).is_none(),
            "{cid} identity should be cleaned up after grouped rejection",
        );
    }

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn test_query_account_perp_endpoint_failure_emits_no_state() {
    // Mirror of the existing spot-failure test but the perp clearinghouse
    // returns a malformed payload. The spawned task must bail before emitting
    // an AccountState. A regression that swallows the perp parse error would
    // emit a partially-populated state derived only from the spot side.
    let state = TestServerState::default();
    // ClearinghouseState defaults most fields, so provide a value with the
    // wrong type on a field whose default cannot absorb it.
    *state.perp_clearinghouse_response.lock().await = Some(json!({
        "assetPositions": "this-should-be-an-array"
    }));

    let addr = start_mock_server(state).await;
    let (mut client, mut rx, cache) = create_test_execution_client(addr);
    add_test_account_to_cache(&cache, AccountId::from("HYPERLIQUID-001"));

    client.start().unwrap();

    let cmd = QueryAccount::new(
        TraderId::from("TESTER-001"),
        Some(*HYPERLIQUID_CLIENT_ID),
        AccountId::from("HYPERLIQUID-001"),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None, // correlation_id
    );
    client.query_account(cmd).unwrap();

    let event = tokio::time::timeout(Duration::from_millis(500), rx.recv()).await;
    assert!(
        event.is_err(),
        "no AccountState should be emitted on perp parse failure; got {event:?}",
    );
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn test_generate_order_status_reports_filters_open_only_and_time_range() {
    // Mock a frontendOpenOrders payload with 3 orders so the path's open_only
    // filter has work to do; assert open_only=true keeps every entry the
    // venue returns (frontendOpenOrders only ever returns open orders).
    let state = TestServerState::default();
    *state.frontend_open_orders_response.lock().await = Some(json!([
        {
            "coin": "BTC", "side": "B", "limitPx": "95000.0", "sz": "0.001",
            "oid": 100001u64, "timestamp": 1700000000000u64, "origSz": "0.001",
        },
        {
            "coin": "BTC", "side": "B", "limitPx": "95100.0", "sz": "0.002",
            "oid": 100002u64, "timestamp": 1700000010000u64, "origSz": "0.002",
        },
        {
            "coin": "BTC", "side": "B", "limitPx": "95200.0", "sz": "0.003",
            "oid": 100003u64, "timestamp": 1700000020000u64, "origSz": "0.003",
        },
    ]));

    let addr = start_mock_server(state).await;
    let (mut client, _rx, cache) = create_test_execution_client(addr);
    add_test_account_to_cache(&cache, AccountId::from("HYPERLIQUID-001"));
    client.connect().await.unwrap();

    let cmd_all = GenerateOrderStatusReports::new(
        UUID4::new(),
        UnixNanos::default(),
        true,
        Some(InstrumentId::from(HYPERLIQUID_TEST_INSTRUMENT)),
        None,
        None,
        None,
        None,
    );
    let reports = client
        .generate_order_status_reports(&cmd_all)
        .await
        .unwrap();
    assert_eq!(reports.len(), 3);

    // Filter by `start` only: keep orders with ts_last >= start. Since
    // timestamps are converted from ms to ns, choose a cutoff between the
    // first and second order: 1700000005000 ms == 1700000005000 * 1e6 ns.
    let cutoff = UnixNanos::from(1_700_000_005_000_000_000u64);
    let cmd_start = GenerateOrderStatusReports::new(
        UUID4::new(),
        UnixNanos::default(),
        true,
        Some(InstrumentId::from(HYPERLIQUID_TEST_INSTRUMENT)),
        Some(cutoff),
        None,
        None,
        None,
    );
    let reports = client
        .generate_order_status_reports(&cmd_start)
        .await
        .unwrap();
    assert_eq!(reports.len(), 2);

    // Filter by `end` only: keep orders with ts_last <= end. Set end before
    // the third order's timestamp.
    let end = UnixNanos::from(1_700_000_015_000_000_000u64);
    let cmd_end = GenerateOrderStatusReports::new(
        UUID4::new(),
        UnixNanos::default(),
        true,
        Some(InstrumentId::from(HYPERLIQUID_TEST_INSTRUMENT)),
        None,
        Some(end),
        None,
        None,
    );
    let reports = client
        .generate_order_status_reports(&cmd_end)
        .await
        .unwrap();
    assert_eq!(reports.len(), 2);

    // Both bounds: keep only the middle order.
    let cmd_both = GenerateOrderStatusReports::new(
        UUID4::new(),
        UnixNanos::default(),
        true,
        Some(InstrumentId::from(HYPERLIQUID_TEST_INSTRUMENT)),
        Some(cutoff),
        Some(end),
        None,
        None,
    );
    let reports = client
        .generate_order_status_reports(&cmd_both)
        .await
        .unwrap();
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].venue_order_id, VenueOrderId::from("100002"));

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn test_generate_fill_reports_filters_time_range() {
    let state = TestServerState::default();
    *state.user_fills_response.lock().await = Some(json!([
        {
            "coin": "BTC", "px": "50000.0", "sz": "0.001", "side": "B",
            "time": 1_700_000_000_000u64, "startPosition": "0",
            "dir": "Open Long", "closedPnl": "0", "hash": "0xaaaa",
            "oid": 1u64, "crossed": true, "fee": "0.01", "tid": 1u64,
            "feeToken": "USDC",
        },
        {
            "coin": "BTC", "px": "50100.0", "sz": "0.002", "side": "B",
            "time": 1_700_000_010_000u64, "startPosition": "0",
            "dir": "Open Long", "closedPnl": "0", "hash": "0xbbbb",
            "oid": 2u64, "crossed": true, "fee": "0.02", "tid": 2u64,
            "feeToken": "USDC",
        },
        {
            "coin": "BTC", "px": "50200.0", "sz": "0.003", "side": "B",
            "time": 1_700_000_020_000u64, "startPosition": "0",
            "dir": "Open Long", "closedPnl": "0", "hash": "0xcccc",
            "oid": 3u64, "crossed": true, "fee": "0.03", "tid": 3u64,
            "feeToken": "USDC",
        },
    ]));

    let addr = start_mock_server(state).await;
    let (mut client, _rx, cache) = create_test_execution_client(addr);
    add_test_account_to_cache(&cache, AccountId::from("HYPERLIQUID-001"));
    client.connect().await.unwrap();

    let cutoff = UnixNanos::from(1_700_000_005_000_000_000u64);
    let end = UnixNanos::from(1_700_000_015_000_000_000u64);

    let cmd_none = GenerateFillReports::new(
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
        None,
        None,
        None,
        None,
    );
    let reports = client.generate_fill_reports(cmd_none).await.unwrap();
    assert_eq!(reports.len(), 3, "no filter must return every fill");

    let cmd_start = GenerateFillReports::new(
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
        Some(cutoff),
        None,
        None,
        None,
    );
    let reports = client.generate_fill_reports(cmd_start).await.unwrap();
    assert_eq!(reports.len(), 2);

    let cmd_end = GenerateFillReports::new(
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
        None,
        Some(end),
        None,
        None,
    );
    let reports = client.generate_fill_reports(cmd_end).await.unwrap();
    assert_eq!(reports.len(), 2);

    let cmd_both = GenerateFillReports::new(
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
        Some(cutoff),
        Some(end),
        None,
        None,
    );
    let reports = client.generate_fill_reports(cmd_both).await.unwrap();
    assert_eq!(reports.len(), 1);

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn test_generate_mass_status_lookback_filters_only_fills() {
    // generate_mass_status applies the lookback only to fills. Open orders and
    // positions must always be returned in full so reconciliation has the
    // complete current state. A regression that applies the cutoff to other
    // report types would silently drop orders or positions and mask drift.
    let state = TestServerState::default();
    *state.frontend_open_orders_response.lock().await = Some(json!([
        {
            "coin": "BTC", "side": "B", "limitPx": "95000.0", "sz": "0.001",
            "oid": 200001u64, "timestamp": 1u64, "origSz": "0.001",
        },
    ]));
    *state.user_fills_response.lock().await = Some(json!([
        {
            "coin": "BTC", "px": "50000.0", "sz": "0.001", "side": "B",
            "time": 1u64, "startPosition": "0",
            "dir": "Open Long", "closedPnl": "0", "hash": "0xaaaa",
            "oid": 200001u64, "crossed": true, "fee": "0.01", "tid": 1u64,
            "feeToken": "USDC",
        },
    ]));
    let addr = start_mock_server(state).await;
    let (mut client, _rx, cache) = create_test_execution_client(addr);
    add_test_account_to_cache(&cache, AccountId::from("HYPERLIQUID-001"));
    client.connect().await.unwrap();

    // Lookback of 1 minute: the seeded fill at time=1ms is far older than the
    // cutoff, so the fill must be filtered out while the open order survives.
    let mass = client
        .generate_mass_status(Some(1))
        .await
        .unwrap()
        .expect("mass status payload");

    assert_eq!(
        mass.order_reports().len(),
        1,
        "open orders must not be filtered by lookback",
    );
    assert!(
        mass.fill_reports().is_empty(),
        "old fills must be excluded from the lookback window",
    );

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn test_get_account_address_uses_explicit_account_address() {
    // When `config.account_address` is set, the client must use it for info
    // requests instead of the user_address derived from the private key.
    // The mock captures the `user` field of the most recent `clearinghouseState`
    // request so we can assert the override actually flowed through.
    let state = TestServerState::default();
    let last_user = state.last_clearinghouse_user.clone();
    let addr = start_mock_server(state).await;

    let trader_id = TraderId::from("TESTER-001");
    let account_id = AccountId::from("HYPERLIQUID-001");
    let client_id = *HYPERLIQUID_CLIENT_ID;
    let cache = Rc::new(RefCell::new(Cache::default()));

    let core = ExecutionClientCore::new(
        trader_id,
        client_id,
        *HYPERLIQUID_VENUE,
        OmsType::Netting,
        account_id,
        AccountType::Margin,
        None,
        cache.clone(),
    );

    let explicit_address = "0xcafebabedeadbeef000000000000000000000001";
    let config = HyperliquidExecClientConfig {
        private_key: Some(TEST_PRIVATE_KEY.to_string()),
        base_url_http: Some(format!("http://{addr}/info")),
        base_url_exchange: Some(format!("http://{addr}/exchange")),
        base_url_ws: Some(format!("ws://{addr}/ws")),
        account_address: Some(explicit_address.to_string()),
        environment: HyperliquidEnvironment::Mainnet,
        ..HyperliquidExecClientConfig::default()
    };
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    set_exec_event_sender(tx);

    let mut client = HyperliquidExecutionClient::new(core, config).unwrap();
    add_test_account_to_cache(&cache, account_id);

    client.connect().await.unwrap();
    assert!(client.is_connected());

    let captured = last_user
        .lock()
        .await
        .clone()
        .expect("clearinghouseState request must have been issued during connect");
    assert_eq!(
        captured.to_lowercase(),
        explicit_address,
        "config.account_address must be the user supplied to clearinghouseState",
    );

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn test_stop_aborts_ws_stream_and_pending_tasks() {
    // stop() is the lifecycle counterpart to start(): it must abort the WS
    // stream handle, drain pending action tasks, mark the core disconnected and
    // stopped. Hold a submit task open with `pause_next_exchange` so the
    // pending-tasks assertion has something concrete to observe; an empty
    // tasks vec would satisfy `pending_tasks_all_finished()` vacuously.
    let state = TestServerState::default();
    state.pause_next_exchange.store(true, Ordering::Relaxed);
    let pause_release = state.pause_release.clone();
    let exchange_count = state.exchange_request_count.clone();
    let addr = start_mock_server(state).await;
    let (mut client, _rx, cache) = create_test_execution_client(addr);
    add_test_account_to_cache(&cache, AccountId::from("HYPERLIQUID-001"));

    client.start().unwrap();
    client.connect().await.unwrap();
    assert!(client.is_connected());

    let order = make_limit_order("O-STOP-PEND");
    cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .unwrap();
    client.submit_order(make_submit_cmd(&order)).unwrap();

    // Wait until the mock has actually received the request, proving the
    // spawned task is parked at the post await before we call stop().
    wait_until_async(
        move || {
            let count = exchange_count.clone();
            async move { *count.lock().await >= 1 }
        },
        Duration::from_secs(5),
    )
    .await;

    assert!(
        !client.pending_tasks_all_finished(),
        "submit_order task must be pending while the mock holds the response",
    );

    client.stop().unwrap();
    assert!(!client.is_connected());
    assert!(
        client.pending_tasks_all_finished(),
        "stop() must abort the pending action task",
    );

    // Idempotent: a second stop() is a no-op.
    client.stop().unwrap();

    // Release the held mock so its handler can return cleanly.
    pause_release.notify_one();
}

// `await_account_registered` hard-codes a 30s timeout inside `connect()`,
// so exercising the timeout path costs ~30s per test run. The cheap,
// deterministic version requires plumbing an injectable timeout through
// `HyperliquidExecClientConfig` or `connect()` -- a public-API change
// that does not belong inside a test-only patch. The test stays here as
// documentation of the production gap and runs only when invoked by name
// (`cargo test ... -- --ignored`).
#[ignore = "blocks ~30s on hard-coded await_account_registered timeout; revisit once timeout is injectable"]
#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn test_connect_times_out_when_account_never_registers() {
    let state = TestServerState::default();
    let addr = start_mock_server(state).await;
    let (mut client, _rx, _cache) = create_test_execution_client(addr);

    let result = tokio::time::timeout(Duration::from_secs(40), client.connect()).await;
    let inner = result.expect("connect should not exceed harness timeout");
    assert!(
        inner.is_err(),
        "connect must fail when account never registers"
    );

    assert!(!client.is_connected());
    assert!(client.pending_tasks_all_finished());
}
