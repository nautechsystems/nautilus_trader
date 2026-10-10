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

//! Integration tests for `DeriveExecutionClient` against local REST and WS mocks.
//!
//! Covers the lifecycle (connect, private channel subscription, disconnect),
//! the order operations (submit / cancel / modify / batch-cancel / query),
//! report generation (open / history / fill / position), and the private
//! WS dispatch loop. Uses minimal axum mocks that record the incoming
//! request bodies and let tests inject responses or push WS frames.

use std::{
    cell::RefCell,
    collections::HashMap,
    net::SocketAddr,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use axum::{
    Router,
    extract::{
        State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::StatusCode,
    response::{IntoResponse, Json, Response},
    routing::{get, post},
};
use futures_util::StreamExt;
use nautilus_common::{
    cache::{Cache, ORDER_NOT_FOUND},
    clients::ExecutionClient,
    clock::VirtualClock,
    live::runner::{
        replace_data_event_sender, replace_exec_event_sender, replace_system_event_sender,
    },
    messages::{
        DataEvent, ExecutionEvent, SystemEvent,
        execution::{
            BatchCancelOrders, CancelAllOrders, CancelOrder, ExecutionReport, GenerateFillReports,
            GenerateOrderStatusReport, GenerateOrderStatusReports, GeneratePositionStatusReports,
            ModifyOrder, QueryAccount, QueryOrder, SubmitOrder, SubmitOrderList,
        },
        system::SocketState,
    },
    runner::{TimeEventMessage, TimeEventSender, set_time_event_sender},
    testing::wait_until_async,
};
use nautilus_core::{DurationNanos, UUID4, UnixNanos, time::get_atomic_clock_realtime};
use nautilus_derive::{
    common::{
        consts::{DERIVE_VENUE, MIN_SIGNATURE_TTL, TRIGGER_ORDER_SIGNATURE_TTL},
        enums::DeriveEnvironment,
        parse::parse_derive_instrument_any,
    },
    config::DeriveExecutionClientConfig,
    execution::DeriveExecutionClient,
    http::{
        DeriveHttpError,
        models::{DeriveInstrument, DerivePosition, DeriveSubaccount},
        parse::parse_derive_subaccount_to_balances,
        query::DeriveGetSubaccountParams,
    },
    websocket::dispatch::ORDER_DEDUP_CAPACITY,
};
use nautilus_execution::engine::ExecutionEngine;
use nautilus_live::{
    ExecutionClientCore, SocketReconnectRegistry, SocketReconnectRequestOutcome,
    manager::{ExecutionManager, ExecutionManagerConfig},
};
use nautilus_model::{
    accounts::{AccountAny, MarginAccount},
    data::QuoteTick,
    enums::{
        AccountType, OmsType, OrderSide, OrderStatus, OrderType, PositionSide, TimeInForce,
        TriggerType,
    },
    events::{
        AccountState, OrderAccepted, OrderCanceled, OrderEventAny, OrderEventType,
        OrderInitialized, OrderPendingUpdate, OrderSubmitted, OrderUpdated,
    },
    identifiers::{
        AccountId, ClientId, ClientOrderId, InstrumentId, OrderListId, StrategyId, TradeId,
        TraderId, VenueOrderId,
    },
    instruments::Instrument,
    orders::{Order, OrderAny, OrderList, OrderTestBuilder},
    reports::{ExecutionMassStatus, OrderStatusReport, PositionStatusReport},
    types::{AccountBalance, Currency, MarginBalance, Money, Price, Quantity},
};
use nautilus_network::{http::HttpClient, websocket::TransportBackend};
use rstest::rstest;
use rust_decimal_macros::dec;
use serde_json::{Value, json};
use ustr::Ustr;

const TEST_WALLET: &str = "0x000000000000000000000000000000000000aaaa";
const TEST_SESSION_KEY: &str = "0x2ae8be44db8a590d20bffbe3b6872df9b569147d3bf6801a35a28281a4816bbd";
const TEST_SUBACCOUNT: u64 = 30769;
const TEST_DOMAIN_SEPARATOR: &str =
    "0x2222222222222222222222222222222222222222222222222222222222222222";
const TEST_ACTION_TYPEHASH: &str =
    "0x1111111111111111111111111111111111111111111111111111111111111111";
const TEST_TRADE_MODULE_ADDRESS: &str = "0x000000000000000000000000000000000000bbbb";

#[derive(Clone, Default)]
struct RestState {
    get_subaccount_calls: Arc<tokio::sync::Mutex<Vec<Value>>>,
    get_order_calls: Arc<tokio::sync::Mutex<Vec<Value>>>,
    open_orders_calls: Arc<tokio::sync::Mutex<Vec<Value>>>,
    trigger_orders_calls: Arc<tokio::sync::Mutex<Vec<Value>>>,
    order_history_calls: Arc<tokio::sync::Mutex<Vec<Value>>>,
    trade_history_calls: Arc<tokio::sync::Mutex<Vec<Value>>>,
    positions_calls: Arc<tokio::sync::Mutex<Vec<Value>>>,
    ticker_calls: Arc<tokio::sync::Mutex<Vec<Value>>>,
    get_instrument_calls: Arc<tokio::sync::Mutex<Vec<Value>>>,
    subaccount_response: Arc<tokio::sync::Mutex<Value>>,
    open_orders_response: Arc<tokio::sync::Mutex<Value>>,
    trigger_orders_response: Arc<tokio::sync::Mutex<Value>>,
    order_history_response: Arc<tokio::sync::Mutex<Value>>,
    order_history_pages: Arc<tokio::sync::Mutex<Vec<Value>>>,
    trade_history_response: Arc<tokio::sync::Mutex<Value>>,
    trade_history_pages: Arc<tokio::sync::Mutex<Vec<Value>>>,
    positions_response: Arc<tokio::sync::Mutex<Value>>,
    ticker_response: Arc<tokio::sync::Mutex<Value>>,
    get_order_response: Arc<tokio::sync::Mutex<Value>>,
    get_order_responses: Arc<tokio::sync::Mutex<HashMap<String, Value>>>,
    get_instrument_response: Arc<tokio::sync::Mutex<Value>>,
}

#[derive(Clone)]
struct WsState {
    connection_count: Arc<AtomicUsize>,
    request_methods: Arc<tokio::sync::Mutex<Vec<Vec<String>>>>,
    login_frames: Arc<tokio::sync::Mutex<Vec<Value>>>,
    subscribe_frames: Arc<tokio::sync::Mutex<Vec<Value>>>,
    subscribe_status: Arc<tokio::sync::Mutex<Option<HashMap<String, String>>>>,
    login_failures_after_first: Arc<AtomicUsize>,
    disconnect_after_subscribe: Arc<AtomicBool>,
    // Order entry now flows over the WebSocket Trading API. Each vector holds
    // the `params` object of a captured `private/*` frame so assertions read
    // the signed body fields directly (`body["instrument_name"]`, etc.).
    submitted_orders: Arc<tokio::sync::Mutex<Vec<Value>>>,
    submitted_order_received_at_secs: Arc<tokio::sync::Mutex<Vec<u64>>>,
    submitted_trigger_orders: Arc<tokio::sync::Mutex<Vec<Value>>>,
    cancelled_orders: Arc<tokio::sync::Mutex<Vec<Value>>>,
    cancelled_trigger_orders: Arc<tokio::sync::Mutex<Vec<Value>>>,
    cancelled_labels: Arc<tokio::sync::Mutex<Vec<Value>>>,
    cancel_by_instrument_calls: Arc<tokio::sync::Mutex<Vec<Value>>>,
    cancel_all_calls: Arc<tokio::sync::Mutex<Vec<Value>>>,
    replace_orders: Arc<tokio::sync::Mutex<Vec<Value>>>,
    // Injected JSON-RPC reply body (without `id`) per private method. When set,
    // the mock merges the request `id` and returns it instead of the default
    // success result, e.g. `json!({"error": {"code": -32602, "message": "x"}})`.
    order_reply: Arc<tokio::sync::Mutex<Option<Value>>>,
    trigger_order_reply: Arc<tokio::sync::Mutex<Option<Value>>>,
    cancel_reply: Arc<tokio::sync::Mutex<Option<Value>>>,
    cancel_trigger_reply: Arc<tokio::sync::Mutex<Option<Value>>>,
    cancel_by_instrument_reply: Arc<tokio::sync::Mutex<Option<Value>>>,
    cancel_by_label_reply: Arc<tokio::sync::Mutex<Option<Value>>>,
    replace_reply: Arc<tokio::sync::Mutex<Option<Value>>>,
    replace_notification_before_reply: Arc<tokio::sync::Mutex<Option<Value>>>,
    order_notification_before_reply: Arc<tokio::sync::Mutex<Option<Value>>>,
    write_reply_delay: Arc<tokio::sync::Mutex<Duration>>,
    notification_tx: tokio::sync::mpsc::UnboundedSender<Value>,
    notification_rx: Arc<tokio::sync::Mutex<Option<tokio::sync::mpsc::UnboundedReceiver<Value>>>>,
    native_orders: Arc<tokio::sync::Mutex<HashMap<String, Value>>>,
}

impl Default for WsState {
    fn default() -> Self {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<Value>();

        Self {
            connection_count: Arc::new(AtomicUsize::new(0)),
            request_methods: Arc::new(tokio::sync::Mutex::new(Vec::new())),
            login_frames: Arc::new(tokio::sync::Mutex::new(Vec::new())),
            subscribe_frames: Arc::new(tokio::sync::Mutex::new(Vec::new())),
            subscribe_status: Arc::new(tokio::sync::Mutex::new(None)),
            login_failures_after_first: Arc::new(AtomicUsize::new(0)),
            disconnect_after_subscribe: Arc::new(AtomicBool::new(false)),
            submitted_orders: Arc::new(tokio::sync::Mutex::new(Vec::new())),
            submitted_order_received_at_secs: Arc::new(tokio::sync::Mutex::new(Vec::new())),
            submitted_trigger_orders: Arc::new(tokio::sync::Mutex::new(Vec::new())),
            cancelled_orders: Arc::new(tokio::sync::Mutex::new(Vec::new())),
            cancelled_trigger_orders: Arc::new(tokio::sync::Mutex::new(Vec::new())),
            cancelled_labels: Arc::new(tokio::sync::Mutex::new(Vec::new())),
            cancel_by_instrument_calls: Arc::new(tokio::sync::Mutex::new(Vec::new())),
            cancel_all_calls: Arc::new(tokio::sync::Mutex::new(Vec::new())),
            replace_orders: Arc::new(tokio::sync::Mutex::new(Vec::new())),
            order_reply: Arc::new(tokio::sync::Mutex::new(None)),
            trigger_order_reply: Arc::new(tokio::sync::Mutex::new(None)),
            cancel_reply: Arc::new(tokio::sync::Mutex::new(None)),
            cancel_trigger_reply: Arc::new(tokio::sync::Mutex::new(None)),
            cancel_by_instrument_reply: Arc::new(tokio::sync::Mutex::new(None)),
            cancel_by_label_reply: Arc::new(tokio::sync::Mutex::new(None)),
            replace_reply: Arc::new(tokio::sync::Mutex::new(None)),
            replace_notification_before_reply: Arc::new(tokio::sync::Mutex::new(None)),
            order_notification_before_reply: Arc::new(tokio::sync::Mutex::new(None)),
            write_reply_delay: Arc::new(tokio::sync::Mutex::new(Duration::ZERO)),
            native_orders: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            notification_tx: tx,
            notification_rx: Arc::new(tokio::sync::Mutex::new(Some(rx))),
        }
    }
}

impl WsState {
    fn push_notification(&self, frame: Value) {
        self.notification_tx
            .send(frame)
            .expect("notification queue closed");
    }
}

async fn handle_rest_health() -> impl IntoResponse {
    StatusCode::OK
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

async fn handle_get_subaccount(
    State(state): State<RestState>,
    body: axum::body::Bytes,
) -> Response {
    let parsed: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    state.get_subaccount_calls.lock().await.push(parsed);
    let response = state.subaccount_response.lock().await.clone();

    let body = if response.is_null() {
        json!({"id": 1, "result": sample_subaccount_json()})
    } else {
        json!({"id": 1, "result": response})
    };

    (StatusCode::OK, Json(body)).into_response()
}

async fn handle_get_order(State(state): State<RestState>, body: axum::body::Bytes) -> Response {
    let parsed: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    state.get_order_calls.lock().await.push(parsed.clone());
    let mut response = state.get_order_response.lock().await.clone();
    if response.is_null() {
        response = state
            .get_order_responses
            .lock()
            .await
            .get(parsed["order_id"].as_str().unwrap_or_default())
            .cloned()
            .unwrap_or(Value::Null);
    }

    let body = if response.is_null() {
        json!({"id": 1, "result": sample_order_json()})
    } else if response.get("error").is_some() {
        response
    } else {
        json!({"id": 1, "result": response})
    };

    (StatusCode::OK, Json(body)).into_response()
}

async fn handle_get_open_orders(
    State(state): State<RestState>,
    body: axum::body::Bytes,
) -> Response {
    let parsed: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    state.open_orders_calls.lock().await.push(parsed);
    let response = state.open_orders_response.lock().await.clone();

    let body = if response.is_null() {
        json!({"id": 1, "result": {"orders": [sample_order_json()], "subaccount_id": TEST_SUBACCOUNT}})
    } else if response.get("error").is_some() {
        response
    } else {
        json!({"id": 1, "result": response})
    };

    (StatusCode::OK, Json(body)).into_response()
}

async fn handle_get_trigger_orders(
    State(state): State<RestState>,
    body: axum::body::Bytes,
) -> Response {
    let parsed: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    state.trigger_orders_calls.lock().await.push(parsed);
    let response = state.trigger_orders_response.lock().await.clone();

    let body = if response.is_null() {
        json!({"id": 1, "result": {"orders": [], "subaccount_id": TEST_SUBACCOUNT}})
    } else if response.get("error").is_some() {
        response
    } else {
        json!({"id": 1, "result": response})
    };

    (StatusCode::OK, Json(body)).into_response()
}

async fn handle_get_order_history(
    State(state): State<RestState>,
    body: axum::body::Bytes,
) -> Response {
    let parsed: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    state.order_history_calls.lock().await.push(parsed);

    let mut pages = state.order_history_pages.lock().await;
    if !pages.is_empty() {
        let page = pages.remove(0);
        return (StatusCode::OK, Json(json!({"id": 1, "result": page}))).into_response();
    }

    drop(pages);

    let response = state.order_history_response.lock().await.clone();

    let body = if response.is_null() {
        // Default: empty page so by-label fallbacks terminate.
        json!({
            "id": 1,
            "result": {
                "orders": [],
                "pagination": {"count": 0, "num_pages": 0},
                "subaccount_id": TEST_SUBACCOUNT,
            }
        })
    } else if response.get("error").is_some() {
        response
    } else {
        json!({"id": 1, "result": response})
    };

    (StatusCode::OK, Json(body)).into_response()
}

async fn handle_get_trade_history(
    State(state): State<RestState>,
    body: axum::body::Bytes,
) -> Response {
    let parsed: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    state.trade_history_calls.lock().await.push(parsed);

    // `trade_history_pages` lets pagination tests sequence one response per
    // call; when empty, fall back to the single canned response.
    let mut pages = state.trade_history_pages.lock().await;
    if !pages.is_empty() {
        let page = pages.remove(0);
        return (StatusCode::OK, Json(json!({"id": 1, "result": page}))).into_response();
    }

    drop(pages);

    let response = state.trade_history_response.lock().await.clone();

    let body = if response.is_null() {
        json!({
            "id": 1,
            "result": {
                "trades": [],
                "pagination": {"count": 0, "num_pages": 0},
                "subaccount_id": TEST_SUBACCOUNT,
            }
        })
    } else if response.get("error").is_some() {
        response
    } else {
        json!({"id": 1, "result": response})
    };

    (StatusCode::OK, Json(body)).into_response()
}

async fn handle_get_positions(State(state): State<RestState>, body: axum::body::Bytes) -> Response {
    let parsed: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    state.positions_calls.lock().await.push(parsed);
    let response = state.positions_response.lock().await.clone();

    let body = if response.is_null() {
        json!({
            "id": 1,
            "result": {"positions": [], "subaccount_id": TEST_SUBACCOUNT}
        })
    } else if response.get("error").is_some() {
        response
    } else {
        json!({"id": 1, "result": response})
    };

    (StatusCode::OK, Json(body)).into_response()
}

async fn handle_get_tickers(State(state): State<RestState>, body: axum::body::Bytes) -> Response {
    let parsed: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    state.ticker_calls.lock().await.push(parsed);

    let response = state.ticker_response.lock().await.clone();

    let response = if response.is_null() {
        sample_ticker_json("ETH-PERP", 1_700_000_000_013_i64)
    } else {
        response
    };

    let body = if response.get("error").is_some() {
        response
    } else if response.get("tickers").is_some() {
        json!({"id": 1, "result": response})
    } else {
        let instrument_name = response
            .get("instrument_name")
            .and_then(Value::as_str)
            .unwrap_or("ETH-PERP");
        json!({"id": 1, "result": {"tickers": {instrument_name: response}}})
    };

    (StatusCode::OK, Json(body)).into_response()
}

async fn handle_get_instrument(
    State(state): State<RestState>,
    body: axum::body::Bytes,
) -> Response {
    let parsed: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    state.get_instrument_calls.lock().await.push(parsed.clone());
    let response = state.get_instrument_response.lock().await.clone();

    let mut result = if response.is_null() {
        sample_instrument_json()
    } else {
        response
    };

    // Echo the requested name so a fetch for any instrument returns a
    // definition whose name matches the order that triggered it.
    if let Some(requested) = parsed.get("instrument_name").and_then(Value::as_str) {
        result["instrument_name"] = Value::String(requested.to_string());
    }

    (StatusCode::OK, Json(json!({"id": 1, "result": result}))).into_response()
}

async fn start_rest_server(state: RestState) -> SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let router = Router::new()
        .route("/health", get(handle_rest_health))
        .route("/private/get_subaccount", post(handle_get_subaccount))
        .route("/private/get_order", post(handle_get_order))
        .route("/private/get_open_orders", post(handle_get_open_orders))
        .route(
            "/private/get_trigger_orders",
            post(handle_get_trigger_orders),
        )
        .route("/private/get_order_history", post(handle_get_order_history))
        .route("/private/get_trade_history", post(handle_get_trade_history))
        .route("/private/get_positions", post(handle_get_positions))
        .route("/public/get_tickers", post(handle_get_tickers))
        .route("/public/get_instrument", post(handle_get_instrument))
        .with_state(state);

    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });

    wait_for_http_health(addr).await;
    addr
}

async fn handle_ws_upgrade(ws: WebSocketUpgrade, State(state): State<WsState>) -> Response {
    ws.on_upgrade(move |socket| handle_ws(socket, state))
}

async fn handle_ws(mut socket: WebSocket, state: WsState) {
    state.connection_count.fetch_add(1, Ordering::SeqCst);

    let connection_index = {
        let mut requests = state.request_methods.lock().await;
        requests.push(Vec::new());
        requests.len() - 1
    };

    // Take the notification receiver on connect; the test's `push_notification`
    // sends Values that get forwarded to the client as subscription frames.
    let mut notification_rx = state.notification_rx.lock().await.take();

    loop {
        tokio::select! {
            biased;
            frame = socket.next() => {
                let Some(Ok(frame)) = frame else { break };
                match frame {
                    Message::Text(text) => {
                        let Ok(payload) = serde_json::from_str::<Value>(&text) else {
                            continue;
                        };
                        let id = payload.get("id").and_then(Value::as_u64).unwrap_or(0);
                        let method = payload.get("method").and_then(Value::as_str).unwrap_or("");
                        state.request_methods.lock().await[connection_index].push(method.to_string());

                        let params = payload.get("params").cloned().unwrap_or(Value::Null);
                        let request_nonce = params.get("nonce").cloned();
                        let mut reply = match method {
                            "public/login" => {
                                let login_count = {
                                    let mut frames = state.login_frames.lock().await;
                                    frames.push(payload.clone());
                                    frames.len()
                                };
                                let reject_reconnect = login_count > 1
                                    && state
                                        .login_failures_after_first
                                        .try_update(
                                            Ordering::SeqCst,
                                            Ordering::SeqCst,
                                            |remaining| remaining.checked_sub(1),
                                        )
                                        .is_ok();

                                if reject_reconnect {
                                    json!({
                                        "id": id,
                                        "error": {"code": 9002, "message": "Backend unavailable"},
                                    })
                                } else {
                                    json!({"id": id, "result": [TEST_SUBACCOUNT]})
                                }
                            }
                            "subscribe" => {
                                state.subscribe_frames.lock().await.push(payload.clone());
                                let channels = payload
                                    .get("params")
                                    .and_then(|p| p.get("channels"))
                                    .and_then(Value::as_array)
                                    .cloned()
                                    .unwrap_or_default();

                                if let Some(status) = state.subscribe_status.lock().await.clone() {
                                    let current_subscriptions = channels
                                        .iter()
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
                                } else {
                                    {
                                let status: serde_json::Map<String, Value> = channels.iter()
                                    .filter_map(Value::as_str)
                                    .map(|channel| (channel.to_string(), json!("ok")))
                                    .collect();
                                json!({"id": id, "result": {"current_subscriptions": channels, "status": status}})
                            }
                                }
                            }
                            "private/order" if params.get("trigger_type").is_some() => {
                                state
                                    .submitted_trigger_orders
                                    .lock()
                                    .await
                                    .push(params.clone());
                                ws_reply(id, &state.trigger_order_reply, || {
                                    json!({
                                        "result": {
                                            "order": trigger_order_json_with(
                                                "trig-mock-1",
                                                params
                                                    .get("label")
                                                    .and_then(Value::as_str)
                                                    .unwrap_or("STRAT-TRIGGER-1"),
                                                params
                                                    .get("direction")
                                                    .and_then(Value::as_str)
                                                    .unwrap_or("buy"),
                                                params
                                                    .get("instrument_name")
                                                    .and_then(Value::as_str)
                                                    .unwrap_or("ETH-PERP"),
                                                1_700_000_001_000_i64,
                                                params
                                                    .get("order_type")
                                                    .and_then(Value::as_str)
                                                    .unwrap_or("market"),
                                                "untriggered",
                                                params
                                                    .get("limit_price")
                                                    .and_then(Value::as_str)
                                                    .unwrap_or("3500"),
                                                params
                                                    .get("trigger_price")
                                                    .and_then(Value::as_str)
                                                    .unwrap_or("3450"),
                                                params
                                                    .get("trigger_price_type")
                                                    .and_then(Value::as_str)
                                                    .unwrap_or("mark"),
                                                params
                                                    .get("trigger_type")
                                                    .and_then(Value::as_str)
                                                    .unwrap_or("stoploss"),
                                            ),
                                        }
                                    })
                                })
                                .await
                            }
                            "private/order" => {
                                let received_at_secs = SystemTime::now()
                                    .duration_since(UNIX_EPOCH)
                                    .expect("system time is after unix epoch")
                                    .as_secs();
                                state.submitted_orders.lock().await.push(params);
                                state
                                    .submitted_order_received_at_secs
                                    .lock()
                                    .await
                                    .push(received_at_secs);
                                ws_reply(id, &state.order_reply, || {
                                    json!({"result": {"order": sample_order_json()}})
                                })
                                .await
                            }
                            "private/replace" => {
                                state.replace_orders.lock().await.push(params.clone());
                                ws_reply(id, &state.replace_reply, || {
                                    let label = params
                                        .get("label")
                                        .and_then(Value::as_str)
                                        .unwrap_or("STRAT-O-1");
                                    let order_id_to_cancel = params
                                        .get("order_id_to_cancel")
                                        .and_then(Value::as_str)
                                        .unwrap_or("ord-stale-1");
                                    json!({
                                        "result": {
                                            "order": order_json_with(
                                                "ord-replaced-1",
                                                label,
                                                "buy",
                                                "ETH-PERP",
                                                1_700_000_001_000_i64,
                                                "open",
                                            ),
                                            "cancelled_order": order_json_with(
                                                order_id_to_cancel,
                                                label,
                                                "buy",
                                                "ETH-PERP",
                                                1_700_000_000_000_i64,
                                                "cancelled",
                                            ),
                                        }
                                    })
                                })
                                .await
                            }
                            "private/cancel" => {
                                state.cancelled_orders.lock().await.push(params);
                                ws_reply(id, &state.cancel_reply, || json!({"result": {}})).await
                            }
                            "private/cancel_trigger_order" => {
                                state
                                    .cancelled_trigger_orders
                                    .lock()
                                    .await
                                    .push(params.clone());
                                ws_reply(id, &state.cancel_trigger_reply, || {
                                    json!({
                                        "result": trigger_order_json_with(
                                            params
                                                .get("order_id")
                                                .and_then(Value::as_str)
                                                .unwrap_or("trig-mock-1"),
                                            "STRAT-TRIGGER-1",
                                            "buy",
                                            "ETH-PERP",
                                            1_700_000_002_000_i64,
                                            "market",
                                            "cancelled",
                                            "3500",
                                            "3450",
                                            "mark",
                                            "stoploss",
                                        )
                                    })
                                })
                                .await
                            }
                            "private/cancel_by_instrument" => {
                                state
                                    .cancel_by_instrument_calls
                                    .lock()
                                    .await
                                    .push(params);
                                ws_reply(id, &state.cancel_by_instrument_reply, || {
                                    json!({"result": {"cancelled_orders": 1}})
                                })
                                .await
                            }
                            "private/cancel_by_label" => {
                                state.cancelled_labels.lock().await.push(params);
                                ws_reply(id, &state.cancel_by_label_reply, || {
                                    json!({"result": {"cancelled_orders": 1}})
                                })
                                .await
                            }
                            "private/cancel_all" => {
                                state.cancel_all_calls.lock().await.push(params);
                                json!({"id": id, "result": {}})
                            }
                            _ => json!({"id": id, "result": {}}),
                        };

                        if method == "private/replace"
                            && let Some(order) = reply.pointer_mut("/result/order")
                            && order.get("nonce") == Some(&json!("1"))
                        {
                            order["nonce"] = request_nonce.clone().unwrap();
                        }

                        if matches!(method, "private/order" | "private/replace") {
                            record_native_orders(&state, &reply).await;
                        }

                        if method == "private/replace"
                            && let Some(mut notification) = state
                                .replace_notification_before_reply
                                .lock()
                                .await
                                .take()
                        {
                            if let Some(orders) = notification.pointer_mut("/params/data").and_then(Value::as_array_mut) {
                                for order in orders {
                                    if order.get("order_status").is_some() && order.get("nonce") == Some(&json!("1")) {
                                        order["nonce"] = request_nonce.clone().unwrap();
                                    }
                                }
                            }
                            record_native_orders(&state, &notification).await;
                            if socket
                                .send(Message::Text(notification.to_string().into()))
                                .await
                                .is_err()
                            {
                                break;
                            }
                            tokio::time::sleep(Duration::from_millis(100)).await;
                        }

                        if method == "private/order"
                            && let Some(notification) = state.order_notification_before_reply.lock().await.take()
                        {
                            if socket.send(Message::Text(notification.to_string().into())).await.is_err() {
                                break;
                            }
                            tokio::time::sleep(Duration::from_millis(100)).await;
                        }

                        if matches!(method, "private/order" | "private/replace") {
                            tokio::time::sleep(*state.write_reply_delay.lock().await).await;
                        }

                        if socket
                            .send(Message::Text(reply.to_string().into()))
                            .await
                            .is_err()
                        {
                            break;
                        }

                        if method == "subscribe"
                            && state.disconnect_after_subscribe.swap(false, Ordering::SeqCst)
                        {
                            let _ = socket.send(Message::Close(None)).await;
                            break;
                        }
                    }
                    Message::Close(_) => break,
                    _ => {}
                }
            }
            notif = recv_notification(&mut notification_rx) => {
                let Some(notif) = notif else { continue };
                record_native_orders(&state, &notif).await;
                if socket
                    .send(Message::Text(notif.to_string().into()))
                    .await
                    .is_err()
                {
                    break;
                }
            }
        }
    }

    state.connection_count.fetch_sub(1, Ordering::SeqCst);
}

async fn record_native_orders(state: &WsState, frame: &Value) {
    let mut orders = Vec::new();
    if let Some(order) = frame.pointer("/result/order") {
        orders.push(order);
    }

    if let Some(order) = frame.pointer("/result/cancelled_order") {
        orders.push(order);
    }

    if let Some(order) = frame
        .get("result")
        .filter(|value| value.get("order_id").is_some())
    {
        orders.push(order);
    }

    if let Some(rows) = frame.pointer("/params/data").and_then(Value::as_array) {
        orders.extend(rows.iter().filter(|row| row.get("order_status").is_some()));
    }

    let mut native = state.native_orders.lock().await;

    for order in orders {
        if let Some(id) = order["order_id"].as_str() {
            let timestamp = order["last_update_timestamp"].as_i64().unwrap_or_default();
            if native.get(id).is_none_or(|current| {
                current["last_update_timestamp"]
                    .as_i64()
                    .unwrap_or_default()
                    <= timestamp
            }) {
                native.insert(id.to_string(), order.clone());
            }
        }
    }
}

async fn recv_notification(
    rx: &mut Option<tokio::sync::mpsc::UnboundedReceiver<Value>>,
) -> Option<Value> {
    match rx {
        Some(rx) => rx.recv().await,
        None => std::future::pending().await,
    }
}

/// Builds a JSON-RPC reply for a captured `private/*` frame: the injected reply
/// body merged with `id` when set, otherwise the default success result.
async fn ws_reply(
    id: u64,
    injected: &Arc<tokio::sync::Mutex<Option<Value>>>,
    default: impl FnOnce() -> Value,
) -> Value {
    let mut reply = injected.lock().await.clone().unwrap_or_else(default);
    if let Value::Object(map) = &mut reply {
        map.insert("id".to_string(), json!(id));
    }

    reply
}

async fn start_ws_server(state: WsState) -> SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let router = Router::new()
        .route("/ws", get(handle_ws_upgrade))
        .route("/health", get(handle_rest_health))
        .with_state(state);

    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });

    wait_for_http_health(addr).await;
    addr
}

fn rest_url(addr: SocketAddr) -> String {
    format!("http://{addr}")
}

fn ws_url(addr: SocketAddr) -> String {
    format!("ws://{addr}/ws")
}

fn sample_instrument_json() -> Value {
    json!({
        "amount_step": "0.001",
        "base_asset_address": "0x000000000000000000000000000000000000abcd",
        "base_asset_sub_id": "42",
        "base_currency": "ETH",
        "base_fee": "0",
        "instrument_name": "ETH-PERP",
        "instrument_type": "perp",
        "is_active": true,
        "maker_fee_rate": "0.0001",
        "mark_price_fee_rate_cap": null,
        "maximum_amount": "1000",
        "minimum_amount": "0.001",
        "option_details": null,
        "perp_details": {
            "aggregate_funding": "0",
            "funding_rate": "0",
            "index": "ETH-USD",
            "max_rate_per_hour": "0.01",
            "min_rate_per_hour": "-0.01",
            "static_interest_rate": "0",
        },
        "quote_currency": "USDC",
        "scheduled_activation": 0,
        "scheduled_deactivation": 32503680000000_i64,
        "taker_fee_rate": "0.0005",
        "tick_size": "0.01",
    })
}

fn sample_ticker_json(instrument_name: &str, timestamp_ms: i64) -> Value {
    json!({
        "instrument_name": instrument_name,
        "best_ask_amount": "1.0",
        "best_ask_price": "3501.00",
        "best_bid_amount": "1.0",
        "best_bid_price": "3500.00",
        "funding_rate": "0",
        "index_price": "3500",
        "mark_price": "3500",
        "max_price": "5000",
        "min_price": "1",
        "timestamp": timestamp_ms,
    })
}

fn option_instrument_json(instrument_name: &str, option_type: &str, strike: &str) -> Value {
    json!({
        "amount_step": "0.01",
        "base_asset_address": "0x0000000000000000000000000000000000000001",
        "base_asset_sub_id": "12345",
        "base_currency": "ETH",
        "base_fee": "1",
        "instrument_name": instrument_name,
        "instrument_type": "option",
        "is_active": true,
        "maker_fee_rate": "0",
        "mark_price_fee_rate_cap": null,
        "maximum_amount": "100",
        "minimum_amount": "0.01",
        "option_details": {
            "expiry": 1_782_000_000_i64,
            "index": "ETH-USD",
            "option_type": option_type,
            "settlement_price": null,
            "strike": strike,
        },
        "perp_details": null,
        "quote_currency": "USDC",
        "scheduled_activation": 1_700_000_000_i64,
        "scheduled_deactivation": 1_782_000_000_i64,
        "taker_fee_rate": "0.001",
        "tick_size": "1",
    })
}

fn spot_instrument_json(instrument_name: &str) -> Value {
    json!({
        "amount_step": "0.01",
        "base_asset_address": "0x41675b7746AE0E464f2594d258CF399c392A179C",
        "base_asset_sub_id": "0",
        "base_currency": "ETH",
        "base_fee": "0",
        "instrument_name": instrument_name,
        "instrument_type": "erc20",
        "is_active": true,
        "maker_fee_rate": "0",
        "mark_price_fee_rate_cap": null,
        "maximum_amount": "10000",
        "minimum_amount": "0.1",
        "option_details": null,
        "perp_details": null,
        "quote_currency": "USDC",
        "scheduled_activation": 0,
        "scheduled_deactivation": 32503680000000_i64,
        "taker_fee_rate": "0",
        "tick_size": "0.1",
    })
}

fn sample_order_json() -> Value {
    json!({
        "amount": "1",
        "average_price": "3500",
        "cancel_reason": "",
        "creation_timestamp": 1_700_000_000_000_i64,
        "direction": "buy",
        "filled_amount": "0",
        "instrument_name": "ETH-PERP",
        "is_transfer": false,
        "label": "STRAT-O-1",
        "last_update_timestamp": 1_700_000_001_000_i64,
        "limit_price": "3500",
        "max_fee": "1",
        "mmp": false,
        "nonce": "1",
        "order_fee": "0",
        "order_id": "ord-mock-1",
        "order_status": "open",
        "order_type": "limit",
        "signature": "0x00",
        "signature_expiry_sec": 1_700_000_900,
        "signer": "0xsigner",
        "subaccount_id": TEST_SUBACCOUNT,
        "time_in_force": "gtc",
    })
}

fn order_json_with(
    order_id: &str,
    label: &str,
    direction: &str,
    instrument_name: &str,
    last_update_ms: i64,
    status: &str,
) -> Value {
    json!({
        "amount": "1",
        "average_price": "3500",
        "cancel_reason": "",
        "creation_timestamp": 1_700_000_000_000_i64,
        "direction": direction,
        "filled_amount": "0",
        "instrument_name": instrument_name,
        "is_transfer": false,
        "label": label,
        "last_update_timestamp": last_update_ms,
        "limit_price": "3500",
        "max_fee": "1",
        "mmp": false,
        "nonce": "1",
        "order_fee": "0",
        "order_id": order_id,
        "order_status": status,
        "order_type": "limit",
        "signature": "0x00",
        "signature_expiry_sec": 1_700_000_900,
        "signer": "0xsigner",
        "subaccount_id": TEST_SUBACCOUNT,
        "time_in_force": "gtc",
    })
}

#[expect(clippy::too_many_arguments)]
fn trigger_order_json_with(
    order_id: &str,
    label: &str,
    direction: &str,
    instrument_name: &str,
    last_update_ms: i64,
    order_type: &str,
    status: &str,
    limit_price: &str,
    trigger_price: &str,
    trigger_price_type: &str,
    trigger_type: &str,
) -> Value {
    json!({
        "amount": "1",
        "average_price": "0",
        "cancel_reason": "",
        "creation_timestamp": 1_700_000_000_000_i64,
        "direction": direction,
        "filled_amount": "0",
        "instrument_name": instrument_name,
        "is_transfer": false,
        "label": label,
        "last_update_timestamp": last_update_ms,
        "limit_price": limit_price,
        "max_fee": "1",
        "mmp": false,
        "nonce": "1",
        "order_fee": "0",
        "order_id": order_id,
        "order_status": status,
        "order_type": order_type,
        "signature": "0x00",
        "signature_expiry_sec": 1_702_678_400,
        "signer": "0xsigner",
        "subaccount_id": TEST_SUBACCOUNT,
        "time_in_force": "gtc",
        "trigger_price": trigger_price,
        "trigger_price_type": trigger_price_type,
        "trigger_type": trigger_type,
    })
}

fn sample_trade_json(trade_id: &str, order_id: &str, instrument_name: &str) -> Value {
    trade_json_with_label(trade_id, order_id, instrument_name, "STRAT-O-1")
}

fn trade_json_with_label(
    trade_id: &str,
    order_id: &str,
    instrument_name: &str,
    label: &str,
) -> Value {
    json!({
        "direction": "buy",
        "index_price": "3500",
        "instrument_name": instrument_name,
        "is_transfer": false,
        "label": label,
        "liquidity_role": "taker",
        "mark_price": "3500",
        "order_id": order_id,
        "quote_id": null,
        "realized_pnl": "0",
        "subaccount_id": TEST_SUBACCOUNT,
        "timestamp": 1_700_000_002_000_i64,
        "trade_amount": "1",
        "trade_fee": "0.5",
        "trade_id": trade_id,
        "trade_price": "3505",
        "tx_hash": "0xabc",
        "batch_status": "Settled",
        "wallet": "0xwallet",
    })
}

fn sample_position_json(instrument_name: &str, amount: &str) -> Value {
    json!({
        "amount": amount,
        "average_price": "3500",
        "creation_timestamp": 1_700_000_000_000_i64,
        "cumulative_funding": "0",
        "delta": "1",
        "gamma": "0",
        "index_price": "3500",
        "initial_margin": "100",
        "instrument_name": instrument_name,
        "instrument_type": "perp",
        "leverage": null,
        "liquidation_price": null,
        "maintenance_margin": "50",
        "mark_price": "3500",
        "mark_value": "3500",
        "net_settlements": "0",
        "open_orders_margin": "0",
        "pending_funding": "0",
        "realized_pnl": "0",
        "theta": "0",
        "unrealized_pnl": "0",
        "vega": "0",
    })
}

fn sample_subaccount_json() -> Value {
    json!({
        "collaterals": [{
            "amount": "1000",
            "asset_name": "USDC",
            "asset_type": "erc20",
            "cumulative_interest": "0",
            "currency": "USDC",
            "initial_margin": "100",
            "maintenance_margin": "50",
            "mark_price": "1",
            "mark_value": "1000",
            "pending_interest": "0",
        }],
        "collaterals_initial_margin": "100",
        "collaterals_maintenance_margin": "50",
        "collaterals_value": "1000",
        "currency": ["ETH", "BTC"],
        "failed_to_fetch": false,
        "manager_id": 3,
        "risk_universe_id": 1,
        "mm_credits": "0",
        "projected_margin_change": "0",
        "vault_deposit_holds": [],
        "initial_margin": "100",
        "is_under_liquidation": false,
        "maintenance_margin": "50",
        "margin_type": "SM",
        "open_orders": [],
        "open_orders_margin": "0",
        "positions": [],
        "positions_initial_margin": "0",
        "positions_maintenance_margin": "0",
        "positions_value": "0",
        "subaccount_id": TEST_SUBACCOUNT,
        "subaccount_value": "1000",
    })
}

fn test_config(rest: SocketAddr, ws: SocketAddr) -> DeriveExecutionClientConfig {
    DeriveExecutionClientConfig {
        account_id: AccountId::from("DERIVE-001"),
        wallet_address: Some(TEST_WALLET.to_string()),
        session_key: Some(TEST_SESSION_KEY.into()),
        subaccount_id: Some(TEST_SUBACCOUNT),
        base_url_rest: Some(rest_url(rest)),
        base_url_ws: Some(ws_url(ws)),
        proxy_url: None,
        environment: DeriveEnvironment::Testnet,
        http_timeout_secs: 5,
        max_retries: 1,
        retry_delay_initial_ms: 50,
        retry_delay_max_ms: 500,
        ws_timeout_secs: Some(30),
        max_fee_per_contract: Some(dec!(1000)),
        transport_backend: TransportBackend::default(),
        domain_separator: Some(TEST_DOMAIN_SEPARATOR.to_string()),
        action_typehash: Some(TEST_ACTION_TYPEHASH.to_string()),
        trade_module_address: Some(TEST_TRADE_MODULE_ADDRESS.to_string()),
        signature_expiry_secs: 600,
        market_order_slippage_bps: 50,
        max_matching_requests_per_second: None,
        max_per_instrument_matching_requests_per_second: None,
    }
}

fn build_core(cache: Rc<RefCell<Cache>>) -> ExecutionClientCore {
    ExecutionClientCore::new(
        TraderId::from("TRADER-001"),
        ClientId::from("DERIVE"),
        *DERIVE_VENUE,
        OmsType::Netting,
        AccountId::from("DERIVE-001"),
        AccountType::Margin,
        None,
        cache,
    )
}

struct TestClient {
    client: DeriveExecutionClient,
    cache: Rc<RefCell<Cache>>,
    rx: tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
}

async fn build_client(rest_state: RestState, ws_state: WsState) -> TestClient {
    build_client_with_config(rest_state, ws_state, None, |config| config).await
}

async fn build_client_with_config(
    rest_state: RestState,
    mut ws_state: WsState,
    registry: Option<&SocketReconnectRegistry>,
    configure: impl FnOnce(DeriveExecutionClientConfig) -> DeriveExecutionClientConfig,
) -> TestClient {
    ws_state.native_orders = rest_state.get_order_responses.clone();
    let rest_addr = start_rest_server(rest_state).await;
    let ws_addr = start_ws_server(ws_state).await;
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<ExecutionEvent>();
    replace_exec_event_sender(tx);

    let cache = Rc::new(RefCell::new(Cache::default()));
    // Pre-register the account so `connect()`'s `await_account_registered`
    // gate resolves immediately; the live runner populates the cache from
    // `refresh_account_state`'s `AccountState` event, but tests drive the
    // emitter directly.
    register_test_account(&cache, AccountId::from("DERIVE-001"));

    let config = configure(test_config(rest_addr, ws_addr));
    let client = || DeriveExecutionClient::new(build_core(cache.clone()), config);
    let mut client = match registry {
        Some(registry) => registry.scope(client),
        None => client(),
    }
    .expect("client creation succeeds");

    // start() installs the freshly-replaced event sender on the emitter, so
    // tests that drain the receiver must call it before any emit_*.
    client.start().expect("start succeeds");
    TestClient { client, cache, rx }
}

fn register_test_account(cache: &Rc<RefCell<Cache>>, account_id: AccountId) {
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

async fn wait_until<F, Fut>(predicate: F, _label: &str)
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    wait_until_async(predicate, Duration::from_secs(5)).await;
}

async fn drain_until<F>(
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
    predicate: F,
    label: &str,
) -> ExecutionEvent
where
    F: Fn(&ExecutionEvent) -> bool,
{
    let deadline = Duration::from_secs(5);

    let outcome = tokio::time::timeout(deadline, async {
        loop {
            let event = rx.recv().await?;
            if predicate(&event) {
                return Some(event);
            }
        }
    })
    .await
    .unwrap_or(None);

    match outcome {
        Some(event) => event,
        None => panic!("timeout waiting for: {label}"),
    }
}

/// Drains until `OrderDenied` for `client_order_id`, failing if `OrderSubmitted`
/// for the same order arrives first.
async fn drain_denied_without_submitted(
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
    client_order_id: &ClientOrderId,
) -> ExecutionEvent {
    let deadline = Duration::from_secs(5);

    let outcome = tokio::time::timeout(deadline, async {
        loop {
            let event = rx.recv().await?;

            if let ExecutionEvent::Order(OrderEventAny::Submitted(submitted)) = &event
                && submitted.client_order_id == *client_order_id
            {
                panic!("OrderSubmitted emitted for {client_order_id} before OrderDenied");
            }

            if let ExecutionEvent::Order(OrderEventAny::Denied(denied)) = &event
                && denied.client_order_id == *client_order_id
            {
                return Some(event);
            }
        }
    })
    .await
    .unwrap_or(None);

    match outcome {
        Some(event) => event,
        None => panic!("timeout waiting for OrderDenied for {client_order_id}"),
    }
}

fn build_limit_order(
    instrument_id: InstrumentId,
    client_order_id: ClientOrderId,
    side: OrderSide,
    price: Price,
    quantity: Quantity,
) -> OrderAny {
    build_limit_order_with_time_in_force(
        instrument_id,
        client_order_id,
        side,
        price,
        quantity,
        TimeInForce::Gtc,
        false,
    )
}

fn build_limit_order_with_time_in_force(
    instrument_id: InstrumentId,
    client_order_id: ClientOrderId,
    side: OrderSide,
    price: Price,
    quantity: Quantity,
    time_in_force: TimeInForce,
    post_only: bool,
) -> OrderAny {
    OrderTestBuilder::new(OrderType::Limit)
        .trader_id(TraderId::from("TRADER-001"))
        .strategy_id(StrategyId::from("S-1"))
        .instrument_id(instrument_id)
        .client_order_id(client_order_id)
        .side(side)
        .quantity(quantity)
        .price(price)
        .time_in_force(time_in_force)
        .post_only(post_only)
        .build()
}

fn build_reduce_only_limit_order(
    instrument_id: InstrumentId,
    client_order_id: ClientOrderId,
    side: OrderSide,
    price: Price,
    quantity: Quantity,
) -> OrderAny {
    OrderTestBuilder::new(OrderType::Limit)
        .trader_id(TraderId::from("TRADER-001"))
        .strategy_id(StrategyId::from("S-1"))
        .instrument_id(instrument_id)
        .client_order_id(client_order_id)
        .side(side)
        .quantity(quantity)
        .price(price)
        .time_in_force(TimeInForce::Ioc)
        .reduce_only(true)
        .build()
}

fn build_market_order(
    instrument_id: InstrumentId,
    client_order_id: ClientOrderId,
    side: OrderSide,
    quantity: Quantity,
) -> OrderAny {
    OrderTestBuilder::new(OrderType::Market)
        .trader_id(TraderId::from("TRADER-001"))
        .strategy_id(StrategyId::from("S-1"))
        .instrument_id(instrument_id)
        .client_order_id(client_order_id)
        .side(side)
        .quantity(quantity)
        .build()
}

fn build_stop_market_order(
    instrument_id: InstrumentId,
    client_order_id: ClientOrderId,
    side: OrderSide,
    trigger_price: Price,
    quantity: Quantity,
) -> OrderAny {
    OrderTestBuilder::new(OrderType::StopMarket)
        .trader_id(TraderId::from("TRADER-001"))
        .strategy_id(StrategyId::from("S-1"))
        .instrument_id(instrument_id)
        .client_order_id(client_order_id)
        .side(side)
        .quantity(quantity)
        .trigger_price(trigger_price)
        .trigger_type(TriggerType::MarkPrice)
        .build()
}

fn accepted_order(
    mut order: OrderAny,
    venue_order_id: VenueOrderId,
    account_id: AccountId,
) -> OrderAny {
    order
        .apply(OrderEventAny::Accepted(OrderAccepted::new(
            order.trader_id(),
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            venue_order_id,
            account_id,
            UUID4::new(),
            UnixNanos::from(1),
            UnixNanos::from(1),
            false,
        )))
        .expect("accept order");
    order
}

fn add_order_to_cache(cache: &Rc<RefCell<Cache>>, order: OrderAny, client_id: Option<ClientId>) {
    cache
        .borrow_mut()
        .add_order(order, None, client_id, false)
        .expect("cache insert");
}

fn build_limit_if_touched_order(
    instrument_id: InstrumentId,
    client_order_id: ClientOrderId,
    side: OrderSide,
    price: Price,
    trigger_price: Price,
    quantity: Quantity,
) -> OrderAny {
    OrderTestBuilder::new(OrderType::LimitIfTouched)
        .trader_id(TraderId::from("TRADER-001"))
        .strategy_id(StrategyId::from("S-1"))
        .instrument_id(instrument_id)
        .client_order_id(client_order_id)
        .side(side)
        .quantity(quantity)
        .price(price)
        .trigger_price(trigger_price)
        .trigger_type(TriggerType::MarkPrice)
        .build()
}

fn submit_cmd(order: &OrderAny) -> SubmitOrder {
    SubmitOrder::from_order(
        order,
        TraderId::from("TRADER-001"),
        Some(ClientId::from("DERIVE")),
        None,
        UUID4::new(),
        UnixNanos::default(),
    )
}

fn make_subscription_frame(channel: &str, data: &Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "method": "subscription",
        "params": {
            "channel": channel,
            "data": data,
        }
    })
}

#[rstest]
#[case("orders")]
#[case("positions")]
#[case("both")]
#[case("triggers")]
#[tokio::test]
async fn test_connect_observes_native_portfolio_instruments_before_private_stream(
    #[case] source: &str,
) {
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    let mut snapshot = sample_subaccount_json();
    if matches!(source, "orders" | "both") {
        snapshot["open_orders"] = json!([sample_order_json()]);
    }

    if matches!(source, "positions" | "both") {
        snapshot["positions"] = json!([sample_position_json("ETH-PERP", "0.75")]);
    }

    if source == "triggers" {
        let mut order = sample_order_json();
        order["order_status"] = json!("untriggered");
        order["trigger_price"] = json!("3600");
        order["trigger_type"] = json!("stoploss");
        order["trigger_price_type"] = json!("mark");
        *rest_state.trigger_orders_response.lock().await =
            json!({"orders": [order], "subaccount_id": TEST_SUBACCOUNT});
    }

    *rest_state.subaccount_response.lock().await = snapshot;
    let mut tc = build_client(rest_state.clone(), ws_state.clone()).await;
    let (data_tx, mut data_rx) = tokio::sync::mpsc::unbounded_channel();
    replace_data_event_sender(data_tx);
    let cache = tc.cache.clone();

    let observed = tokio::time::timeout(Duration::from_secs(2), async {
        tokio::join!(tc.client.connect(), async {
            let event = data_rx.recv().await.unwrap();
            let DataEvent::Instrument(instrument) = event else {
                panic!("Expected required instrument definition");
            };
            let before_stream = ws_state.login_frames.lock().await.is_empty();
            let raw = instrument.info().unwrap().clone();
            let id = instrument.id();
            let precision = (instrument.price_precision(), instrument.size_precision());
            cache.borrow_mut().add_instrument(instrument).unwrap();
            (before_stream, id, precision, raw)
        })
    })
    .await;

    tc.client.disconnect().await.unwrap();
    let (connected, (before_stream, id, precision, raw)) =
        observed.expect("bootstrap must publish its required definition");
    connected.unwrap();
    assert!(before_stream);
    assert_eq!(id, InstrumentId::from("ETH-PERP.DERIVE"));
    assert_eq!(precision, (2, 3));
    assert_eq!(raw.get_str("instrument_name"), Some("ETH-PERP"));
    assert!(cache.borrow().instrument(&id).is_some());
    let definitions = rest_state.get_instrument_calls.lock().await.clone();
    assert_eq!(definitions, vec![json!({"instrument_name": "ETH-PERP"})]);
    assert_eq!(rest_state.get_subaccount_calls.lock().await.len(), 2);
    assert_eq!(rest_state.trigger_orders_calls.lock().await.len(), 2);
    assert!(data_rx.try_recv().is_err());
}

#[rstest]
#[tokio::test]
async fn test_connect_refreshes_account_after_private_subscription_confirmation() {
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    let mut tc = build_client(rest_state.clone(), ws_state.clone()).await;
    let subscription = ws_state.subscribe_status.lock().await;
    let clock = get_atomic_clock_realtime();
    let ts_started = clock.get_time_ns();

    let (connected, ()) = tokio::join!(tc.client.connect(), async {
        wait_until(
            || async { ws_state.subscribe_frames.lock().await.len() == 1 },
            "private subscription awaiting confirmation",
        )
        .await;
        let mut snapshot = sample_subaccount_json();
        snapshot["collaterals"][0]["amount"] = json!("1250");
        snapshot["collaterals"][0]["mark_value"] = json!("1250");
        snapshot["collaterals_value"] = json!("1250");
        *rest_state.subaccount_response.lock().await = snapshot;
        drop(subscription);
    });
    connected.unwrap();
    let ts_connected = clock.get_time_ns();
    tc.client.disconnect().await.unwrap();

    let mut accounts = Vec::new();

    while let Ok(event) = tc.rx.try_recv() {
        if let ExecutionEvent::Account(state) = event {
            accounts.push(state);
        }
    }

    assert_eq!(accounts.len(), 2);
    assert_eq!(accounts[0].account_id, AccountId::from("DERIVE-001"));
    assert_eq!(accounts[0].balances[0].total.as_decimal(), dec!(1000));
    assert_eq!(accounts[0].balances[0].total.currency, Currency::USDC());
    assert_eq!(accounts[1].account_id, AccountId::from("DERIVE-001"));
    assert_eq!(accounts[1].balances[0].total.as_decimal(), dec!(1250));
    assert_eq!(accounts[1].balances[0].total.currency, Currency::USDC());
    assert!(accounts.iter().all(|state| {
        state.ts_event >= ts_started
            && state.ts_event <= state.ts_init
            && state.ts_init <= ts_connected
    }));
    assert_eq!(rest_state.get_subaccount_calls.lock().await.len(), 2);
    assert_eq!(ws_state.subscribe_frames.lock().await.len(), 1);
    assert!(ws_state.submitted_orders.lock().await.is_empty());
}

#[rstest]
#[tokio::test]
async fn test_exec_client_connect_subscribes_private_channels() {
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    let (system_tx, mut system_rx) = tokio::sync::mpsc::unbounded_channel();
    replace_system_event_sender(system_tx);
    let registry = SocketReconnectRegistry::default();

    let mut tc =
        build_client_with_config(rest_state, ws_state.clone(), Some(&registry), |config| {
            config
        })
        .await;

    tc.client.connect().await.expect("connect succeeds");

    wait_until(
        || {
            let state = ws_state.clone();
            async move { !state.subscribe_frames.lock().await.is_empty() }
        },
        "subscribe frame received",
    )
    .await;

    let event = tokio::time::timeout(Duration::from_secs(2), system_rx.recv())
        .await
        .unwrap()
        .unwrap();
    let SystemEvent::SocketState(change) = event;
    let endpoint = Ustr::from("derive-user-streams");
    let client_id = ClientId::from("DERIVE");
    let handle = registry.handle(client_id, endpoint).unwrap();

    assert_eq!(change.client_id, client_id);
    assert_eq!(change.venue, Some(*DERIVE_VENUE));
    assert_eq!(change.endpoint, endpoint);
    assert_eq!(change.state, SocketState::Connected);
    assert_eq!(
        handle.request_reconnect(),
        SocketReconnectRequestOutcome::Accepted
    );
    let event = tokio::time::timeout(Duration::from_secs(2), system_rx.recv())
        .await
        .unwrap()
        .unwrap();
    let SystemEvent::SocketState(change) = event;
    assert_eq!(change.endpoint, endpoint);
    assert_eq!(change.state, SocketState::Disconnected);

    let frames = ws_state.subscribe_frames.lock().await.clone();

    let channels: Vec<String> = frames
        .iter()
        .flat_map(|f| {
            f["params"]["channels"]
                .as_array()
                .cloned()
                .unwrap_or_default()
        })
        .filter_map(|c| c.as_str().map(str::to_string))
        .collect();

    assert!(channels.contains(&format!("{TEST_SUBACCOUNT}.orders")));
    assert!(channels.contains(&format!("{TEST_SUBACCOUNT}.trades")));
    assert!(channels.contains(&format!("{TEST_SUBACCOUNT}.balances")));

    tc.client.disconnect().await.expect("disconnect succeeds");
    assert!(registry.handle(client_id, endpoint).is_none());
}

#[rstest]
#[tokio::test]
async fn test_exec_client_connect_fails_when_private_channel_is_rejected() {
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    *ws_state.subscribe_status.lock().await = Some(HashMap::from([
        (format!("{TEST_SUBACCOUNT}.orders"), "ok".to_string()),
        (
            format!("{TEST_SUBACCOUNT}.trades"),
            "unauthorized".to_string(),
        ),
        (format!("{TEST_SUBACCOUNT}.balances"), "ok".to_string()),
    ]));
    let mut tc = build_client(rest_state, ws_state.clone()).await;

    let err = tc
        .client
        .connect()
        .await
        .expect_err("private subscribe rejection must fail connect");

    let error_chain = format!("{err:#}");
    assert!(error_chain.contains("private WS subscriptions"));
    assert!(error_chain.contains("unauthorized"));
    assert!(!tc.client.is_connected());
    wait_until(
        || {
            let state = ws_state.clone();
            async move { state.connection_count.load(Ordering::SeqCst) == 0 }
        },
        "failed connect tears down WS transport",
    )
    .await;
}

#[rstest]
#[case("SM")]
#[case("PM2")]
#[tokio::test]
async fn test_exec_client_reconnect_refreshes_account_and_submits_mass_status(
    #[case] margin_type: &str,
) {
    let rest_state = RestState::default();
    let mut subaccount = sample_subaccount_json();
    subaccount["margin_type"] = json!(margin_type);
    *rest_state.subaccount_response.lock().await = subaccount;
    let ws_state = WsState::default();
    ws_state
        .disconnect_after_subscribe
        .store(true, Ordering::SeqCst);
    let mut tc = build_report_client(rest_state.clone(), ws_state.clone()).await;

    tc.client.connect().await.expect("connect succeeds");

    let event = drain_until(
        &mut tc.rx,
        |event| {
            matches!(
                event,
                ExecutionEvent::Report(ExecutionReport::MassStatus(_))
            )
        },
        "post-reconnect mass status",
    )
    .await;

    if let ExecutionEvent::Report(ExecutionReport::MassStatus(status)) = event {
        assert_eq!(status.client_id, ClientId::from("DERIVE"));
        assert_eq!(status.account_id, AccountId::from("DERIVE-001"));
    } else {
        unreachable!();
    }

    assert!(rest_state.get_subaccount_calls.lock().await.len() >= 2);
    assert!(!rest_state.open_orders_calls.lock().await.is_empty());
    assert!(!rest_state.trigger_orders_calls.lock().await.is_empty());
    assert!(!rest_state.order_history_calls.lock().await.is_empty());
    assert!(!rest_state.trade_history_calls.lock().await.is_empty());
    assert!(!rest_state.positions_calls.lock().await.is_empty());
    assert_eq!(ws_state.login_frames.lock().await.len(), 2);
    assert_eq!(ws_state.subscribe_frames.lock().await.len(), 2);
    assert_eq!(
        *ws_state.request_methods.lock().await,
        vec![
            vec!["public/login", "subscribe"],
            vec!["public/login", "subscribe"],
        ]
    );

    for frame in ws_state.subscribe_frames.lock().await.iter() {
        let mut channels: Vec<String> =
            serde_json::from_value(frame["params"]["channels"].clone()).unwrap();
        channels.sort();
        assert_eq!(
            channels,
            vec![
                format!("{TEST_SUBACCOUNT}.balances"),
                format!("{TEST_SUBACCOUNT}.orders"),
                format!("{TEST_SUBACCOUNT}.trades"),
            ]
        );
    }

    assert!(tc.client.is_connected());

    tc.client.disconnect().await.expect("disconnect succeeds");
}

#[derive(Debug)]
struct ReconciliationTimeSender(tokio::sync::mpsc::UnboundedSender<TimeEventMessage>);

impl TimeEventSender for ReconciliationTimeSender {
    fn send(&self, message: TimeEventMessage) {
        self.0.send(message).unwrap();
    }
}

#[tokio::test]
async fn test_reconnect_refreshes_cached_replacement_after_terminal_eviction() {
    let rest = RestState::default();
    *rest.open_orders_response.lock().await =
        json!({"orders": [], "subaccount_id": TEST_SUBACCOUNT});
    *rest.positions_response.lock().await =
        json!({"positions": [], "subaccount_id": TEST_SUBACCOUNT});
    let ws = WsState::default();
    let cid = ClientOrderId::from("RECONNECT-REPLACEMENT");
    let parent = VenueOrderId::from("reconnect-parent");
    let current = VenueOrderId::from("reconnect-current");
    let instrument_id = InstrumentId::from("ETH-PERP.DERIVE");
    let account_id = AccountId::from("DERIVE-001");
    let parent_open = order_json_with(
        parent.as_str(),
        cid.as_str(),
        "buy",
        "ETH-PERP",
        1_700_000_001_000,
        "open",
    );
    let mut parent_closed = parent_open.clone();
    parent_closed["order_status"] = json!("cancelled");
    parent_closed["last_update_timestamp"] = json!(1_700_000_002_000_i64);
    let mut child = order_json_with(
        current.as_str(),
        cid.as_str(),
        "buy",
        "ETH-PERP",
        1_700_000_002_000,
        "open",
    );
    child["replaced_order_id"] = json!(parent.as_str());
    child["amount"] = json!("2");
    child["limit_price"] = json!("3505");
    *ws.order_reply.lock().await = Some(json!({"result": {"order": parent_open}}));
    *ws.replace_reply.lock().await =
        Some(json!({"result": {"order": child, "cancelled_order": parent_closed}}));
    let registry = SocketReconnectRegistry::default();
    let mut tc =
        build_client_with_config(rest.clone(), ws.clone(), Some(&registry), |config| config).await;
    let raw: DeriveInstrument = serde_json::from_value(sample_instrument_json()).unwrap();
    let instrument = parse_derive_instrument_any(&raw, UnixNanos::default())
        .unwrap()
        .unwrap();
    tc.client.cache_instrument(raw).unwrap();
    tc.cache.borrow_mut().add_instrument(instrument).unwrap();
    let (time_sender, mut time_events) = tokio::sync::mpsc::unbounded_channel();
    set_time_event_sender(Arc::new(ReconciliationTimeSender(time_sender)));
    let mut canceled = vec![];

    for index in 0..ORDER_DEDUP_CAPACITY {
        let id = ClientOrderId::from(format!("RECONNECT-EVICT-{index}").as_str());
        let venue_id = VenueOrderId::from(format!("reconnect-evict-{index}").as_str());
        let order = accepted_order(
            build_limit_order(
                instrument_id,
                id,
                OrderSide::Buy,
                Price::from("3500.00"),
                Quantity::from("1.000"),
            ),
            venue_id,
            account_id,
        );
        add_order_to_cache(&tc.cache, order, Some(ClientId::from("DERIVE")));
        canceled.push(order_json_with(
            venue_id.as_str(),
            id.as_str(),
            "buy",
            "ETH-PERP",
            1_700_000_003_000,
            "cancelled",
        ));
    }

    tc.client.connect().await.unwrap();
    drain_initial_account_state(&mut tc).await;
    let order = build_limit_order(
        instrument_id,
        cid,
        OrderSide::Buy,
        Price::from("3500.00"),
        Quantity::from("1.000"),
    );
    add_order_to_cache(&tc.cache, order.clone(), Some(ClientId::from("DERIVE")));
    tc.client.submit_order(submit_cmd(&order)).unwrap();
    let accepted = drain_until(
        &mut tc.rx,
        |event| matches!(event, ExecutionEvent::Order(OrderEventAny::Accepted(_))),
        "accepted late order",
    )
    .await;

    let ExecutionEvent::Order(accepted) = accepted else {
        unreachable!()
    };

    tc.cache.borrow_mut().update_order(&accepted).unwrap();
    tc.client
        .modify_order(ModifyOrder::new(
            TraderId::from("TRADER-001"),
            Some(ClientId::from("DERIVE")),
            StrategyId::from("S-1"),
            instrument_id,
            cid,
            Some(parent),
            Some(Quantity::from("2.000")),
            Some(Price::from("3505.00")),
            None,
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ))
        .unwrap();
    let updated = drain_until(
        &mut tc.rx,
        |event| matches!(event, ExecutionEvent::Order(OrderEventAny::Updated(_))),
        "replacement updated",
    )
    .await;

    let ExecutionEvent::Order(updated) = updated else {
        unreachable!()
    };

    tc.cache.borrow_mut().update_order(&updated).unwrap();
    child["order_status"] = json!("cancelled");
    child["cancel_reason"] = json!("user_request");
    child["last_update_timestamp"] = json!(1_700_000_003_000_i64);
    canceled.insert(0, child.clone());
    push_orders_update(&ws, &json!(canceled));

    for _ in 0..canceled.len() {
        let event = drain_until(
            &mut tc.rx,
            |event| matches!(event, ExecutionEvent::Order(OrderEventAny::Canceled(_))),
            "terminal eviction",
        )
        .await;

        let ExecutionEvent::Order(event) = event else {
            unreachable!()
        };

        tc.cache.borrow_mut().update_order(&event).unwrap();
    }

    *rest.order_history_response.lock().await = json!({"orders": [child], "pagination": {"count": 1, "num_pages": 1}, "subaccount_id": TEST_SUBACCOUNT});
    let reconnect = registry
        .handle(ClientId::from("DERIVE"), Ustr::from("derive-user-streams"))
        .unwrap()
        .request_reconnect();

    let event = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            tokio::select! {
                message = time_events.recv() => assert!(message.unwrap().dispatch()),
                event = tc.rx.recv() => {
                    if let Some(ExecutionEvent::Report(ExecutionReport::MassStatus(status))) = event {
                        break status;
                    }
                }
            }
        }
    }).await.unwrap();

    let reports = event.order_reports();
    let report = reports.get(&current).unwrap();

    let mut expected = OrderStatusReport::new(
        account_id,
        instrument_id,
        Some(cid),
        current,
        Some(OrderSide::Buy),
        OrderType::Limit,
        TimeInForce::Gtc,
        OrderStatus::Canceled,
        Quantity::from("2.000"),
        Quantity::from("0.000"),
        UnixNanos::from(1_700_000_000_000_000_000_u64),
        UnixNanos::from(1_700_000_003_000_000_000_u64),
        report.ts_init,
        Some(report.report_id),
    );
    expected.price = Some(Price::from("3505.00"));
    expected.cancel_reason = Some("user_request".to_string());

    assert_eq!(event.client_id, ClientId::from("DERIVE"));
    assert_eq!(reconnect, SocketReconnectRequestOutcome::Accepted);
    assert_eq!(event.account_id, account_id);
    assert_eq!(event.order_reports().len(), 1);
    assert_eq!(report, &expected);
    assert_eq!(
        tc.cache.borrow().order(&cid).unwrap().venue_order_ids(),
        vec![&parent, &current]
    );
    assert_eq!(ws.login_frames.lock().await.len(), 2);
    assert_eq!(ws.subscribe_frames.lock().await.len(), 2);
    assert_eq!(ws.submitted_orders.lock().await.len(), 1);
    assert_eq!(ws.replace_orders.lock().await.len(), 1);
    tc.client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_exec_client_marks_disconnected_after_reconnect_auth_exhaustion() {
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    ws_state
        .login_failures_after_first
        .store(10, Ordering::SeqCst);
    ws_state
        .disconnect_after_subscribe
        .store(true, Ordering::SeqCst);
    let mut tc = build_client(rest_state, ws_state.clone()).await;

    tc.client.connect().await.expect("initial connect succeeds");
    tokio::time::timeout(Duration::from_secs(5), async {
        while tc.client.is_connected() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("execution client remained connected after auth exhaustion");

    assert!(!tc.client.is_connected());
    assert_eq!(ws_state.login_frames.lock().await.len(), 4);
    wait_until(
        || {
            let state = ws_state.clone();
            async move { state.connection_count.load(Ordering::SeqCst) == 0 }
        },
        "failed session recovery closes transport",
    )
    .await;
}

#[rstest]
#[case("SM")]
#[case("PM2")]
#[tokio::test]
async fn test_submit_order_limit_posts_signed_payload(#[case] margin_type: &str) {
    let rest_state = RestState::default();
    let mut subaccount = sample_subaccount_json();
    subaccount["margin_type"] = json!(margin_type);
    *rest_state.subaccount_response.lock().await = subaccount;
    let ws_state = WsState::default();
    let mut tc = build_client(rest_state, ws_state.clone()).await;
    tc.client.connect().await.expect("connect succeeds");

    let instrument_id = InstrumentId::from("ETH-PERP.DERIVE");
    let client_order_id = ClientOrderId::from("STRAT-LIMIT-1");
    let order = build_limit_order(
        instrument_id,
        client_order_id,
        OrderSide::Buy,
        Price::from("3500.00"),
        Quantity::from("1.000"),
    );
    tc.cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .expect("cache insert");

    tc.client
        .submit_order(submit_cmd(&order))
        .expect("submit Ok");

    wait_until(
        || {
            let state = ws_state.clone();
            async move { !state.submitted_orders.lock().await.is_empty() }
        },
        "private/order posted",
    )
    .await;

    let posts = ws_state.submitted_orders.lock().await;
    let body = &posts[0];
    assert_eq!(body["instrument_name"].as_str(), Some("ETH-PERP"));
    assert_eq!(body["direction"].as_str(), Some("buy"));
    assert_eq!(body["order_type"].as_str(), Some("limit"));
    assert_eq!(body["time_in_force"].as_str(), Some("gtc"));
    assert_eq!(body["label"].as_str(), Some("STRAT-LIMIT-1"));
    assert_eq!(body["limit_price"].as_str(), Some("3500.00"));
    assert_eq!(body["amount"].as_str(), Some("1.000"));
    assert_eq!(body["subaccount_id"].as_u64(), Some(TEST_SUBACCOUNT));
    assert!(body["signature"].as_str().unwrap().starts_with("0x"));
    assert!(body["nonce"].as_str().unwrap().parse::<u64>().unwrap() > 1_700_000_000_000_000_000);

    tc.client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test]
async fn test_submit_order_accepts_signature_ttl_above_minimum() {
    let rest_state = RestState::default();
    let ws_state = WsState::default();

    let mut tc = build_client_with_config(rest_state, ws_state.clone(), None, |mut config| {
        config.signature_expiry_secs = MIN_SIGNATURE_TTL.as_secs() + 1;
        config
    })
    .await;

    tc.client.connect().await.expect("connect succeeds");

    let instrument_id = InstrumentId::from("ETH-PERP.DERIVE");
    let client_order_id = ClientOrderId::from("STRAT-OK-TTL-1");
    let order = build_limit_order(
        instrument_id,
        client_order_id,
        OrderSide::Buy,
        Price::from("3500.00"),
        Quantity::from("1.000"),
    );
    tc.cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .expect("cache insert");

    let start_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system time is after unix epoch")
        .as_secs() as i64;
    tc.client
        .submit_order(submit_cmd(&order))
        .expect("submit Ok");

    wait_until(
        || {
            let state = ws_state.clone();
            async move { !state.submitted_orders.lock().await.is_empty() }
        },
        "private/order posted",
    )
    .await;

    let posts = ws_state.submitted_orders.lock().await;
    let body = &posts[0];
    let expiry = body["signature_expiry_sec"]
        .as_i64()
        .expect("payload has signature expiry");
    let expected_ttl = (MIN_SIGNATURE_TTL.as_secs() + 1) as i64;
    assert!(
        expiry >= start_secs + expected_ttl - 2 && expiry <= start_secs + expected_ttl + 5,
        "signature expiry must use the configured TTL above the minimum, was {expiry}",
    );
    assert_eq!(body["label"].as_str(), Some("STRAT-OK-TTL-1"));

    tc.client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test]
async fn test_submit_preserves_synchronous_fill_with_unknown_order_status() {
    let ws_state = WsState::default();
    let cid = ClientOrderId::from("STRAT-UNKNOWN-STATUS");
    let mut response_order = order_json_with(
        "ord-unknown-status",
        cid.as_str(),
        "buy",
        "ETH-PERP",
        1_700_000_001_000,
        "future-status",
    );
    response_order["amount"] = json!("1");
    *ws_state.order_reply.lock().await = Some(json!({"result": {
        "order": response_order,
        "trades": [trade_json_with_label("trade-unknown-status", "ord-unknown-status", "ETH-PERP", cid.as_str())],
    }}));
    let mut tc = build_client(RestState::default(), ws_state.clone()).await;
    tc.client
        .cache_instrument(serde_json::from_value(sample_instrument_json()).unwrap())
        .unwrap();
    tc.client.connect().await.unwrap();
    let order = build_limit_order(
        InstrumentId::from("ETH-PERP.DERIVE"),
        cid,
        OrderSide::Buy,
        Price::from("3500.00"),
        Quantity::from("1.000"),
    );
    tc.cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .unwrap();
    tc.client.submit_order(submit_cmd(&order)).unwrap();
    let event = drain_until(
        &mut tc.rx,
        |event| matches!(event, ExecutionEvent::Order(OrderEventAny::Filled(_))),
        "known synchronous fill",
    )
    .await;

    let ExecutionEvent::Order(OrderEventAny::Filled(fill)) = event else {
        unreachable!()
    };

    assert_eq!(fill.client_order_id, cid);
    assert_eq!(
        fill.venue_order_id,
        VenueOrderId::from("ord-unknown-status")
    );
    assert_eq!(fill.trade_id, TradeId::from("trade-unknown-status"));
    assert_eq!(fill.instrument_id, InstrumentId::from("ETH-PERP.DERIVE"));
    assert_eq!(fill.order_type, OrderType::Limit);
    assert_eq!(fill.order_side, OrderSide::Buy);
    assert_eq!(fill.last_qty, Quantity::from("1.000"));
    assert_eq!(fill.last_px, Price::from("3505.00"));
    assert_eq!(fill.commission, Some(Money::from("0.5 USDC")));
    assert_eq!(
        fill.ts_event,
        UnixNanos::from(1_700_000_002_000_000_000_u64)
    );
    assert_eq!(ws_state.submitted_orders.lock().await.len(), 1);
    tc.client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_deeply_paced_submit_builds_signature_after_matching_quota_wait() {
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    // The fixed window is aligned to client construction, so measure from
    // before the build to bound the reset wait.
    let started = std::time::Instant::now();

    let mut tc = build_client_with_config(rest_state, ws_state.clone(), None, |mut config| {
        config.signature_expiry_secs = MIN_SIGNATURE_TTL.as_secs() + 1;
        config.max_matching_requests_per_second = Some(1);
        config
    })
    .await;

    tc.client.connect().await.expect("connect succeeds");

    let instrument_id = InstrumentId::from("ETH-PERP.DERIVE");

    for sequence in 0..7 {
        let order = build_limit_order(
            instrument_id,
            ClientOrderId::from(format!("STRAT-PACED-{sequence}")),
            OrderSide::Buy,
            Price::from("3500.00"),
            Quantity::from("1.000"),
        );
        tc.cache
            .borrow_mut()
            .add_order(order.clone(), None, None, false)
            .expect("cache insert");
        tc.client
            .submit_order(submit_cmd(&order))
            .expect("submit Ok");
    }

    // The fixed-window reset departs the last two writes at the ~5s boundary;
    // bound the wait from order submission so it cannot race the reset.
    wait_until_async(
        || {
            let state = ws_state.clone();
            async move { state.submitted_orders.lock().await.len() == 7 }
        },
        Duration::from_secs(15),
    )
    .await;

    let elapsed = started.elapsed();
    let posts = ws_state.submitted_orders.lock().await;
    let received_at_secs = ws_state.submitted_order_received_at_secs.lock().await;

    assert_eq!(posts.len(), 7);
    assert_eq!(received_at_secs.len(), 7);
    assert!(
        elapsed >= Duration::from_secs(4),
        "writes past the five-request burst must wait for the discrete window \
         reset (~5s), elapsed {elapsed:?}",
    );

    for (body, received_at_secs) in posts.iter().zip(received_at_secs.iter()) {
        let expiry_secs = body["signature_expiry_sec"]
            .as_i64()
            .expect("payload has signature expiry");
        let remaining_secs = i128::from(expiry_secs) - i128::from(*received_at_secs);
        assert!(
            remaining_secs >= i128::from(MIN_SIGNATURE_TTL.as_secs()),
            "signature for {} must retain at least the venue minimum after pacing, remaining {remaining_secs} s",
            body["label"],
        );
    }

    drop(received_at_secs);
    drop(posts);

    tc.client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test]
async fn test_global_matching_allowance_gates_distinct_instrument_until_window_reset() {
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    // The fixed window is aligned to client construction, so measure from
    // before the build to bound the reset wait.
    let started = std::time::Instant::now();
    let started_secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time is after unix epoch")
        .as_secs();
    let mut tc =
        build_client_with_config(rest_state, ws_state.clone(), None, |config| config).await;
    tc.client.connect().await.expect("connect succeeds");

    // Five ETH-PERP writes exhaust the Trader account-wide matching window
    // (and ETH-PERP's own per-instrument window) without touching BTC-PERP's.
    for sequence in 0..5 {
        let order = build_limit_order(
            InstrumentId::from("ETH-PERP.DERIVE"),
            ClientOrderId::from(format!("STRAT-GLOBAL-ETH-{sequence}")),
            OrderSide::Buy,
            Price::from("3500.00"),
            Quantity::from("1.000"),
        );
        tc.cache
            .borrow_mut()
            .add_order(order.clone(), None, None, false)
            .expect("cache insert");
        tc.client
            .submit_order(submit_cmd(&order))
            .expect("submit Ok");
    }

    wait_until_async(
        || {
            let state = ws_state.clone();
            async move { state.submitted_orders.lock().await.len() == 5 }
        },
        Duration::from_secs(10),
    )
    .await;

    // A BTC-PERP write has a fresh per-instrument allowance, but the global
    // bucket is drained: it must wait for the discrete window reset.
    let btc_order = build_limit_order(
        InstrumentId::from("BTC-PERP.DERIVE"),
        ClientOrderId::from("STRAT-GLOBAL-BTC-0"),
        OrderSide::Buy,
        Price::from("50000.00"),
        Quantity::from("1.000"),
    );
    tc.cache
        .borrow_mut()
        .add_order(btc_order.clone(), None, None, false)
        .expect("cache insert");
    tc.client
        .submit_order(submit_cmd(&btc_order))
        .expect("submit Ok");

    wait_until_async(
        || {
            let state = ws_state.clone();
            async move { state.submitted_orders.lock().await.len() == 6 }
        },
        Duration::from_secs(15),
    )
    .await;

    let posts = ws_state.submitted_orders.lock().await;
    let received_at_secs = ws_state.submitted_order_received_at_secs.lock().await;
    assert_eq!(posts.len(), 6);
    assert_eq!(posts[5]["instrument_name"].as_str(), Some("BTC-PERP"));

    let btc_received_secs = received_at_secs[5];
    let eth_received_secs = received_at_secs[..5].to_vec();
    assert!(
        btc_received_secs >= started_secs + 4,
        "global bucket must gate the BTC-PERP write until the ~5s window reset, \
         started {started_secs}, BTC-PERP received {btc_received_secs}",
    );
    assert!(
        eth_received_secs
            .iter()
            .all(|&secs| secs <= btc_received_secs),
        "the five ETH-PERP writes must depart within the first window",
    );
    drop(received_at_secs);
    drop(posts);
    drop(eth_received_secs);

    let elapsed = started.elapsed();
    assert!(
        elapsed < Duration::from_secs(12),
        "smoke bound: the reset wait must be one window, elapsed {elapsed:?}",
    );

    tc.client.disconnect().await.expect("disconnect");
}

#[rstest]
#[case(MIN_SIGNATURE_TTL.as_secs(), "must be greater than the Derive minimum")]
#[case(MIN_SIGNATURE_TTL.as_secs() - 1, "must be greater than the Derive minimum")]
#[tokio::test]
async fn test_submit_order_rejects_signature_ttl_minimum_or_lower_before_posting(
    #[case] signature_expiry_secs: u64,
    #[case] reason_fragment: &str,
) {
    let rest_state = RestState::default();
    let ws_state = WsState::default();

    let mut tc = build_client_with_config(rest_state, ws_state.clone(), None, |mut config| {
        config.signature_expiry_secs = signature_expiry_secs;
        config
    })
    .await;

    tc.client.connect().await.expect("connect succeeds");

    let instrument_id = InstrumentId::from("ETH-PERP.DERIVE");
    let client_order_id = ClientOrderId::from("STRAT-BAD-TTL-1");
    let order = build_limit_order(
        instrument_id,
        client_order_id,
        OrderSide::Buy,
        Price::from("3500.00"),
        Quantity::from("1.000"),
    );
    tc.cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .expect("cache insert");

    tc.client
        .submit_order(submit_cmd(&order))
        .expect("submit Ok");

    let _ = drain_until(
        &mut tc.rx,
        |event| matches!(event, ExecutionEvent::Order(OrderEventAny::Submitted(_))),
        "OrderSubmitted event",
    )
    .await;
    let event = drain_until(
        &mut tc.rx,
        |event| matches!(event, ExecutionEvent::Order(OrderEventAny::Rejected(_))),
        "OrderRejected event",
    )
    .await;

    if let ExecutionEvent::Order(OrderEventAny::Rejected(rejected)) = event {
        assert_eq!(rejected.client_order_id, order.client_order_id());
        assert!(!rejected.due_post_only);
        assert!(
            rejected.reason.contains("order expiry validation failed")
                && rejected.reason.contains(reason_fragment),
            "unexpected reject reason: {}",
            rejected.reason,
        );
    } else {
        unreachable!();
    }

    assert!(
        ws_state.submitted_orders.lock().await.is_empty(),
        "invalid signature TTL must not post private/order",
    );

    tc.client.disconnect().await.expect("disconnect");
}

#[rstest]
#[case(OrderType::StopMarket, "market", "stoploss", "3582.00")]
#[case(OrderType::StopLimit, "limit", "stoploss", "3555.00")]
#[case(OrderType::MarketIfTouched, "market", "takeprofit", "3582.00")]
#[case(OrderType::LimitIfTouched, "limit", "takeprofit", "3555.00")]
#[tokio::test]
async fn test_submit_trigger_order_posts_v3_order_and_emits_accepted(
    #[case] order_type: OrderType,
    #[case] wire_type: &str,
    #[case] trigger_type: &str,
    #[case] limit_price: &str,
) {
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    let mut tc = build_client(rest_state, ws_state.clone()).await;
    tc.client.connect().await.expect("connect succeeds");

    let instrument_id = InstrumentId::from("ETH-PERP.DERIVE");
    let client_order_id = ClientOrderId::from("STRAT-STOP-1");
    let mut builder = OrderTestBuilder::new(order_type);
    builder
        .trader_id(TraderId::from("TRADER-001"))
        .strategy_id(StrategyId::from("S-1"))
        .instrument_id(instrument_id)
        .client_order_id(client_order_id)
        .side(OrderSide::Sell)
        .quantity(Quantity::from("1.000"))
        .trigger_price(Price::from("3600.00"))
        .trigger_type(TriggerType::MarkPrice);

    if matches!(order_type, OrderType::StopLimit | OrderType::LimitIfTouched) {
        builder.price(Price::from("3555.00"));
    }

    let order = builder.build();
    tc.cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .expect("cache insert");

    let start_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system time is after unix epoch")
        .as_secs() as i64;
    tc.client
        .submit_order(submit_cmd(&order))
        .expect("submit Ok");

    let _ = drain_until(
        &mut tc.rx,
        |e| matches!(e, ExecutionEvent::Order(OrderEventAny::Submitted(_))),
        "OrderSubmitted",
    )
    .await;
    wait_until(
        || {
            let state = ws_state.clone();
            async move { !state.submitted_trigger_orders.lock().await.is_empty() }
        },
        "trigger private/order posted",
    )
    .await;

    let posts = ws_state.submitted_trigger_orders.lock().await;
    let body = &posts[0];
    assert_eq!(body["instrument_name"].as_str(), Some("ETH-PERP"));
    assert_eq!(body["direction"].as_str(), Some("sell"));
    assert_eq!(body["order_type"].as_str(), Some(wire_type));
    assert_eq!(body["time_in_force"].as_str(), Some("gtc"));
    assert_eq!(body["label"].as_str(), Some("STRAT-STOP-1"));
    assert_eq!(body["limit_price"].as_str(), Some(limit_price));
    assert_eq!(body["amount"].as_str(), Some("1.000"));
    assert_eq!(body["trigger_price"].as_str(), Some("3600.00"));
    assert_eq!(body["trigger_price_type"].as_str(), Some("mark"));
    assert_eq!(body["trigger_type"].as_str(), Some(trigger_type));
    assert_eq!(body["subaccount_id"].as_u64(), Some(TEST_SUBACCOUNT));
    assert_eq!(body["referral_code"], "nautilus");
    assert_eq!(body.get("mmp"), None);
    assert_eq!(body.get("reduce_only"), None);
    assert!(body["nonce"].is_string());
    assert!(body["signature"].as_str().unwrap().starts_with("0x"));
    assert_eq!(body.get("conn_id"), None);
    assert_eq!(body.get("order_id"), None);
    let expiry = body["signature_expiry_sec"]
        .as_i64()
        .expect("trigger payload has signature expiry");
    let expected_ttl = TRIGGER_ORDER_SIGNATURE_TTL.as_secs() as i64;
    assert!(
        expiry >= start_secs + expected_ttl - 2 && expiry <= start_secs + expected_ttl + 5,
        "trigger expiry must be about 31 days from submit time, was {expiry}",
    );
    assert!(
        ws_state.submitted_orders.lock().await.is_empty(),
        "trigger fields distinguish conditional from regular submissions",
    );
    drop(posts);

    let event = drain_until(
        &mut tc.rx,
        |e| matches!(e, ExecutionEvent::Order(OrderEventAny::Accepted(_))),
        "OrderAccepted",
    )
    .await;

    if let ExecutionEvent::Order(OrderEventAny::Accepted(accepted)) = event {
        assert_eq!(accepted.client_order_id, client_order_id);
        assert_eq!(accepted.venue_order_id.as_str(), "trig-mock-1");
        assert_eq!(accepted.strategy_id, StrategyId::from("S-1"));
        assert_eq!(accepted.instrument_id, instrument_id);
    } else {
        unreachable!();
    }

    tc.client.disconnect().await.expect("disconnect");
}

#[rstest]
#[case(TimeInForce::Gtc, false, "gtc")]
#[case(TimeInForce::Ioc, false, "ioc")]
#[case(TimeInForce::Fok, false, "fok")]
#[case(TimeInForce::Gtc, true, "post_only")]
#[tokio::test]
async fn test_submit_order_posts_supported_time_in_force(
    #[case] time_in_force: TimeInForce,
    #[case] post_only: bool,
    #[case] expected: &str,
) {
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    let mut tc = build_client(rest_state, ws_state.clone()).await;
    tc.client.connect().await.expect("connect succeeds");

    let instrument_id = InstrumentId::from("ETH-PERP.DERIVE");

    let post_only_suffix = if post_only { "POST" } else { "NORM" };
    let client_order_id =
        ClientOrderId::from(format!("STRAT-TIF-{time_in_force:?}-{post_only_suffix}"));
    let order = build_limit_order_with_time_in_force(
        instrument_id,
        client_order_id,
        OrderSide::Buy,
        Price::from("3500.00"),
        Quantity::from("1.000"),
        time_in_force,
        post_only,
    );
    tc.cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .expect("cache insert");

    tc.client
        .submit_order(submit_cmd(&order))
        .expect("submit Ok");

    wait_until(
        || {
            let state = ws_state.clone();
            async move { !state.submitted_orders.lock().await.is_empty() }
        },
        "private/order posted",
    )
    .await;

    let posts = ws_state.submitted_orders.lock().await;
    assert_eq!(posts[0]["time_in_force"].as_str(), Some(expected));

    tc.client.disconnect().await.expect("disconnect");
}

#[rstest]
#[case(TimeInForce::Day, false, "UNSUPPORTED_TIME_IN_FORCE: DAY")]
#[case(TimeInForce::Day, true, "UNSUPPORTED_TIME_IN_FORCE: DAY")]
#[case(TimeInForce::Ioc, true, "UNSUPPORTED_TIME_IN_FORCE: IOC")]
#[case(TimeInForce::Fok, true, "UNSUPPORTED_TIME_IN_FORCE: FOK")]
#[tokio::test]
async fn test_submit_order_denies_unsupported_time_in_force_before_posting(
    #[case] time_in_force: TimeInForce,
    #[case] post_only: bool,
    #[case] reason_fragment: &str,
) {
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    let mut tc = build_client(rest_state, ws_state.clone()).await;
    tc.client.connect().await.expect("connect succeeds");

    let instrument_id = InstrumentId::from("ETH-PERP.DERIVE");

    let post_only_suffix = if post_only { "POST" } else { "NORM" };
    let client_order_id = ClientOrderId::from(format!(
        "STRAT-BAD-TIF-{time_in_force:?}-{post_only_suffix}"
    ));
    let order = build_limit_order_with_time_in_force(
        instrument_id,
        client_order_id,
        OrderSide::Buy,
        Price::from("3500.00"),
        Quantity::from("1.000"),
        time_in_force,
        post_only,
    );
    tc.cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .expect("cache insert");

    tc.client
        .submit_order(submit_cmd(&order))
        .expect("submit Ok");

    let event = drain_denied_without_submitted(&mut tc.rx, &order.client_order_id()).await;

    if let ExecutionEvent::Order(OrderEventAny::Denied(denied)) = event {
        assert_eq!(denied.client_order_id, order.client_order_id());
        assert_eq!(denied.reason.as_str(), reason_fragment);
    } else {
        unreachable!();
    }

    assert!(
        ws_state.submitted_orders.lock().await.is_empty(),
        "invalid TIF must not post to the venue",
    );

    tc.client.disconnect().await.expect("disconnect");
}

#[tokio::test]
async fn test_submit_order_denies_unsupported_order_type_before_posting() {
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    let mut tc = build_client(rest_state, ws_state.clone()).await;
    tc.client.connect().await.expect("connect succeeds");

    let instrument_id = InstrumentId::from("ETH-PERP.DERIVE");
    let client_order_id = ClientOrderId::from("STRAT-BAD-TYPE");
    let order = OrderTestBuilder::new(OrderType::MarketToLimit)
        .trader_id(TraderId::from("TRADER-001"))
        .strategy_id(StrategyId::from("S-1"))
        .instrument_id(instrument_id)
        .client_order_id(client_order_id)
        .side(OrderSide::Buy)
        .quantity(Quantity::from("1.000"))
        .price(Price::from("3500.00"))
        .build();
    tc.cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .expect("cache insert");

    tc.client
        .submit_order(submit_cmd(&order))
        .expect("submit Ok");

    let event = drain_denied_without_submitted(&mut tc.rx, &order.client_order_id()).await;

    if let ExecutionEvent::Order(OrderEventAny::Denied(denied)) = event {
        assert_eq!(denied.client_order_id, order.client_order_id());
        assert_eq!(
            denied.reason.as_str(),
            "UNSUPPORTED_ORDER_TYPE: MARKET_TO_LIMIT"
        );
    } else {
        unreachable!();
    }

    assert!(
        ws_state.submitted_orders.lock().await.is_empty(),
        "unsupported order type must not post to the venue",
    );

    tc.client.disconnect().await.expect("disconnect");
}

#[tokio::test]
async fn test_submit_order_denies_unsupported_trigger_price_type_before_posting() {
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    let mut tc = build_client(rest_state, ws_state.clone()).await;
    tc.client.connect().await.expect("connect succeeds");

    let instrument_id = InstrumentId::from("ETH-PERP.DERIVE");
    let client_order_id = ClientOrderId::from("STRAT-BAD-TRIGGER-TYPE");
    let order = OrderTestBuilder::new(OrderType::StopMarket)
        .trader_id(TraderId::from("TRADER-001"))
        .strategy_id(StrategyId::from("S-1"))
        .instrument_id(instrument_id)
        .client_order_id(client_order_id)
        .side(OrderSide::Sell)
        .quantity(Quantity::from("1.000"))
        .trigger_price(Price::from("3400.00"))
        .trigger_type(TriggerType::IndexPrice)
        .build();
    tc.cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .expect("cache insert");

    tc.client
        .submit_order(submit_cmd(&order))
        .expect("submit Ok");

    let event = drain_denied_without_submitted(&mut tc.rx, &order.client_order_id()).await;

    if let ExecutionEvent::Order(OrderEventAny::Denied(denied)) = event {
        assert_eq!(denied.client_order_id, order.client_order_id());
        assert!(
            denied.reason.contains("unsupported trigger price type"),
            "unexpected deny reason: {}",
            denied.reason,
        );
    } else {
        unreachable!();
    }

    assert!(
        ws_state.submitted_orders.lock().await.is_empty(),
        "unsupported trigger price type must not post to the venue",
    );

    tc.client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test]
async fn test_submit_order_market_with_quote_uses_rounded_slippage_bound() {
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    let mut refreshed_ticker = sample_ticker_json("ETH-PERP", 1_700_000_000_013_i64);
    refreshed_ticker["best_ask_price"] = json!("3501.00");
    *rest_state.ticker_response.lock().await = refreshed_ticker;
    let mut tc = build_client(rest_state.clone(), ws_state.clone()).await;
    tc.client.connect().await.expect("connect succeeds");

    let instrument_id = InstrumentId::from("ETH-PERP.DERIVE");
    let client_order_id = ClientOrderId::from("STRAT-MARKET-1");

    let quote = QuoteTick::new(
        instrument_id,
        Price::from("3100.00"),
        Price::from("3101.00"),
        Quantity::from("1.000"),
        Quantity::from("1.000"),
        UnixNanos::default(),
        UnixNanos::default(),
    );
    tc.cache
        .borrow_mut()
        .add_quote(quote)
        .expect("quote insert");
    let order = build_market_order(
        instrument_id,
        client_order_id,
        OrderSide::Buy,
        Quantity::from("0.500"),
    );
    tc.cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .expect("cache insert");

    tc.client
        .submit_order(submit_cmd(&order))
        .expect("submit Ok");
    wait_until(
        || {
            let state = ws_state.clone();
            async move { !state.submitted_orders.lock().await.is_empty() }
        },
        "private/order posted for market",
    )
    .await;

    let posts = ws_state.submitted_orders.lock().await;
    let body = &posts[0];
    assert_eq!(body["order_type"].as_str(), Some("market"));
    // Refreshed REST ask, not stale cache: 3501 * 1.005 -> 3518.51
    assert_eq!(body["limit_price"].as_str(), Some("3518.51"));
    assert_eq!(rest_state.ticker_calls.lock().await.len(), 1);

    tc.client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test]
async fn test_submit_order_market_without_quote_is_denied() {
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    let mut tc = build_client(rest_state.clone(), ws_state.clone()).await;
    tc.client.connect().await.expect("connect succeeds");

    let instrument_id = InstrumentId::from("ETH-PERP.DERIVE");
    let client_order_id = ClientOrderId::from("STRAT-MARKET-2");
    let order = build_market_order(
        instrument_id,
        client_order_id,
        OrderSide::Buy,
        Quantity::from("0.500"),
    );
    tc.cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .expect("cache insert");

    tc.client
        .submit_order(submit_cmd(&order))
        .expect("submit Ok");

    let event = drain_until(
        &mut tc.rx,
        |event| matches!(event, ExecutionEvent::Order(OrderEventAny::Denied(_))),
        "OrderDenied event",
    )
    .await;

    if let ExecutionEvent::Order(OrderEventAny::Denied(denied)) = event {
        assert_eq!(
            denied.reason.as_str(),
            "MARKET_PRICE_UNAVAILABLE: order_type=MARKET, instrument_id=ETH-PERP.DERIVE"
        );
    } else {
        unreachable!();
    }

    assert!(ws_state.submitted_orders.lock().await.is_empty());
    assert!(rest_state.ticker_calls.lock().await.is_empty());

    tc.client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test]
async fn test_submit_order_market_rejects_when_quote_refresh_fails_without_posting() {
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    *rest_state.ticker_response.lock().await = json!({
        "id": 1,
        "error": {"code": -32000, "message": "ticker unavailable"},
    });
    let mut tc = build_client(rest_state.clone(), ws_state.clone()).await;
    tc.client.connect().await.expect("connect succeeds");

    let instrument_id = InstrumentId::from("ETH-PERP.DERIVE");
    let client_order_id = ClientOrderId::from("STRAT-MARKET-REFRESH-FAIL");

    let quote = QuoteTick::new(
        instrument_id,
        Price::from("3500.00"),
        Price::from("3501.00"),
        Quantity::from("1.000"),
        Quantity::from("1.000"),
        UnixNanos::default(),
        UnixNanos::default(),
    );
    tc.cache
        .borrow_mut()
        .add_quote(quote)
        .expect("quote insert");
    let order = build_market_order(
        instrument_id,
        client_order_id,
        OrderSide::Buy,
        Quantity::from("0.500"),
    );
    tc.cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .expect("cache insert");

    tc.client
        .submit_order(submit_cmd(&order))
        .expect("submit Ok");

    let _ = drain_until(
        &mut tc.rx,
        |event| matches!(event, ExecutionEvent::Order(OrderEventAny::Submitted(_))),
        "OrderSubmitted event",
    )
    .await;
    let event = drain_until(
        &mut tc.rx,
        |event| matches!(event, ExecutionEvent::Order(OrderEventAny::Rejected(_))),
        "OrderRejected event",
    )
    .await;

    if let ExecutionEvent::Order(OrderEventAny::Rejected(rejected)) = event {
        assert_eq!(rejected.client_order_id, order.client_order_id());
        assert!(
            rejected
                .reason
                .contains("market-order quote refresh failed"),
            "unexpected reject reason: {}",
            rejected.reason,
        );
    } else {
        unreachable!();
    }

    assert!(!rest_state.ticker_calls.lock().await.is_empty());
    assert!(ws_state.submitted_orders.lock().await.is_empty());

    tc.client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test]
async fn test_submit_order_jsonrpc_rejection_emits_order_rejected() {
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    // Mock returns a structured JSON-RPC error envelope.
    *ws_state.order_reply.lock().await = Some(json!({
        "error": {"code": -32602, "message": "Invalid params"}
    }));
    let mut tc = build_client(rest_state, ws_state).await;
    tc.client.connect().await.expect("connect succeeds");

    let instrument_id = InstrumentId::from("ETH-PERP.DERIVE");
    let client_order_id = ClientOrderId::from("STRAT-REJECT-1");
    let order = build_limit_order(
        instrument_id,
        client_order_id,
        OrderSide::Buy,
        Price::from("3500.00"),
        Quantity::from("1.000"),
    );
    tc.cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .expect("cache insert");
    tc.client
        .submit_order(submit_cmd(&order))
        .expect("submit Ok");

    let event = drain_until(
        &mut tc.rx,
        |event| matches!(event, ExecutionEvent::Order(OrderEventAny::Rejected(_))),
        "OrderRejected event",
    )
    .await;

    if let ExecutionEvent::Order(OrderEventAny::Rejected(rejected)) = event {
        let reason = rejected.reason.as_str();
        assert!(!rejected.due_post_only);
        assert!(
            reason.contains("-32602") && reason.contains("Invalid params"),
            "unexpected reject reason: {reason}",
        );
    } else {
        unreachable!();
    }

    tc.client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test]
async fn test_submit_order_post_only_cross_jsonrpc_sets_due_post_only() {
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    *ws_state.order_reply.lock().await = Some(json!({
        "error": {
            "code": 11008,
            "message": "Post only order cannot cross the market"
        }
    }));
    let mut tc = build_client(rest_state, ws_state).await;
    tc.client.connect().await.expect("connect succeeds");

    let instrument_id = InstrumentId::from("ETH-PERP.DERIVE");
    let client_order_id = ClientOrderId::from("STRAT-POST-ONLY-CROSS");
    let order = build_limit_order_with_time_in_force(
        instrument_id,
        client_order_id,
        OrderSide::Buy,
        Price::from("3500.00"),
        Quantity::from("1.000"),
        TimeInForce::Gtc,
        true,
    );
    tc.cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .expect("cache insert");
    tc.client
        .submit_order(submit_cmd(&order))
        .expect("submit Ok");

    let event = drain_until(
        &mut tc.rx,
        |event| matches!(event, ExecutionEvent::Order(OrderEventAny::Rejected(_))),
        "OrderRejected post-only cross",
    )
    .await;

    if let ExecutionEvent::Order(OrderEventAny::Rejected(rejected)) = event {
        let reason = rejected.reason.as_str();
        assert!(rejected.due_post_only);
        assert!(
            reason.contains("11008") && reason.contains("Post only order cannot cross the market"),
            "unexpected reject reason: {reason}",
        );
    } else {
        unreachable!();
    }

    tc.client.disconnect().await.expect("disconnect");
}

#[rstest]
#[case::internal_error(-32603)]
#[case::order_confirmation_timeout(9000)]
#[case::engine_confirmation_timeout(9001)]
#[tokio::test]
async fn test_submit_order_jsonrpc_ambiguous_does_not_emit_order_rejected(#[case] code: i64) {
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    *ws_state.order_reply.lock().await = Some(json!({
        "error": {"code": code, "message": "Internal venue error"}
    }));
    let mut tc = build_client(rest_state, ws_state.clone()).await;
    tc.client.connect().await.expect("connect succeeds");

    let instrument_id = InstrumentId::from("ETH-PERP.DERIVE");
    let client_order_id = ClientOrderId::from("STRAT-SUBMIT-RETRY");
    let order = build_limit_order(
        instrument_id,
        client_order_id,
        OrderSide::Buy,
        Price::from("3500.00"),
        Quantity::from("1.000"),
    );
    tc.cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .expect("cache insert");
    tc.client
        .submit_order(submit_cmd(&order))
        .expect("submit Ok");

    // OrderSubmitted lands synchronously; drain it so the timeout below
    // only watches for a stray Rejected emission.
    let _ = drain_until(
        &mut tc.rx,
        |e| matches!(e, ExecutionEvent::Order(OrderEventAny::Submitted(_))),
        "OrderSubmitted",
    )
    .await;
    wait_until(
        || {
            let state = ws_state.clone();
            async move { !state.submitted_orders.lock().await.is_empty() }
        },
        "private/order posted",
    )
    .await;

    let outcome = tokio::time::timeout(Duration::from_millis(200), async {
        loop {
            match tc.rx.recv().await {
                Some(ExecutionEvent::Order(OrderEventAny::Rejected(_))) => {
                    return Some("unexpected OrderRejected on retryable code");
                }
                Some(_) => {}
                None => return None,
            }
        }
    })
    .await;

    assert!(
        outcome.is_err(),
        "ambiguous JSON-RPC code must not emit OrderRejected, was {outcome:?}",
    );

    assert_eq!(ws_state.submitted_orders.lock().await.len(), 1);

    tc.client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test]
async fn test_submit_order_rate_limit_jsonrpc_emits_order_rejected() {
    // Observed venue behavior: Derive returns `-32000 Rate limit exceeded`
    // for throttled requests. The code sits in the JSON-RPC server-error
    // range and is HTTP-retryable, but the matching engine never saw the
    // request: the gateway threw it out. This is a *definitive* rejection
    // for the write outcome, so the adapter must emit OrderRejected to
    // clear the engine's PendingSubmit. The narrower ambiguous classifier
    // `is_write_outcome_ambiguous_jsonrpc` exists so codes like this are
    // not silently swallowed.
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    *ws_state.order_reply.lock().await = Some(json!({
        "error": {"code": -32000, "message": "Rate limit exceeded: 0xwallet-nonMatching"}
    }));
    let mut tc = build_client(rest_state, ws_state).await;
    tc.client.connect().await.expect("connect succeeds");

    let instrument_id = InstrumentId::from("ETH-PERP.DERIVE");
    let client_order_id = ClientOrderId::from("STRAT-RATE-LIMIT");
    let order = build_limit_order(
        instrument_id,
        client_order_id,
        OrderSide::Buy,
        Price::from("3500.00"),
        Quantity::from("1.000"),
    );
    tc.cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .expect("cache insert");
    tc.client
        .submit_order(submit_cmd(&order))
        .expect("submit Ok");

    let event = drain_until(
        &mut tc.rx,
        |event| matches!(event, ExecutionEvent::Order(OrderEventAny::Rejected(_))),
        "OrderRejected event for rate limit",
    )
    .await;

    if let ExecutionEvent::Order(OrderEventAny::Rejected(rejected)) = event {
        let reason = rejected.reason.as_str();
        assert!(!rejected.due_post_only);
        assert!(
            reason.contains("-32000") && reason.contains("Rate limit"),
            "unexpected reject reason: {reason}",
        );
    } else {
        unreachable!();
    }

    tc.client.disconnect().await.expect("disconnect");
}

#[rstest]
#[case(false)]
#[case(true)]
#[tokio::test]
async fn test_submit_order_list_denies_every_leg_before_submission(#[case] invalid_first: bool) {
    let ws_state = WsState::default();
    let mut tc = build_client(RestState::default(), ws_state.clone()).await;
    tc.client.connect().await.unwrap();
    let instrument_id = InstrumentId::from("ETH-PERP.DERIVE");
    let valid = build_limit_order(
        instrument_id,
        ClientOrderId::from("LIST-VALID"),
        OrderSide::Buy,
        Price::from("3500.00"),
        Quantity::from("1.000"),
    );
    let invalid = build_limit_order_with_time_in_force(
        instrument_id,
        ClientOrderId::from("LIST-INVALID"),
        OrderSide::Sell,
        Price::from("3501.00"),
        Quantity::from("2.000"),
        TimeInForce::Day,
        false,
    );

    let orders = if invalid_first {
        vec![invalid.clone(), valid.clone()]
    } else {
        vec![valid.clone(), invalid.clone()]
    };

    for order in &orders {
        add_order_to_cache(&tc.cache, order.clone(), Some(ClientId::from("DERIVE")));
    }

    let list_id = OrderListId::from("DENIED-LIST");

    let order_list = OrderList::new(
        list_id,
        instrument_id,
        StrategyId::from("S-1"),
        orders.iter().map(Order::client_order_id).collect(),
        UnixNanos::default(),
    );

    let cmd = SubmitOrderList::new(
        TraderId::from("TRADER-001"),
        Some(ClientId::from("DERIVE")),
        StrategyId::from("S-1"),
        order_list,
        orders.iter().map(OrderInitialized::from).collect(),
        None,
        None,
        None,
        UUID4::new(),
        UnixNanos::default(),
        None,
    );
    tc.client.submit_order_list(cmd).unwrap();
    let mut denied = Vec::new();
    let mut submitted = Vec::new();

    while let Ok(event) = tc.rx.try_recv() {
        match event {
            ExecutionEvent::Order(OrderEventAny::Denied(event)) => {
                denied.push((event.client_order_id, event.reason));
            }
            ExecutionEvent::Order(OrderEventAny::Submitted(event)) => {
                submitted.push(event.client_order_id);
            }
            _ => {}
        }
    }

    tc.client.disconnect().await.unwrap();
    denied.sort_by_key(|entry| entry.0);

    assert_eq!(submitted, Vec::<ClientOrderId>::new());
    assert_eq!(
        denied,
        vec![
            (
                invalid.client_order_id(),
                Ustr::from("UNSUPPORTED_TIME_IN_FORCE: DAY")
            ),
            (
                valid.client_order_id(),
                Ustr::from("ORDER_LIST_DENIED: DENIED-LIST")
            ),
        ]
    );
    assert!(ws_state.submitted_orders.lock().await.is_empty());
}

#[rstest]
#[tokio::test]
async fn test_submit_order_list_delegates_per_order() {
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    let mut tc = build_client(rest_state, ws_state.clone()).await;
    tc.client.connect().await.expect("connect succeeds");

    let instrument_id = InstrumentId::from("ETH-PERP.DERIVE");
    let order_a = build_limit_order(
        instrument_id,
        ClientOrderId::from("STRAT-LIST-A"),
        OrderSide::Buy,
        Price::from("3500.00"),
        Quantity::from("1.000"),
    );
    let order_b = build_limit_order(
        instrument_id,
        ClientOrderId::from("STRAT-LIST-B"),
        OrderSide::Sell,
        Price::from("3501.00"),
        Quantity::from("1.000"),
    );
    tc.cache
        .borrow_mut()
        .add_order(order_a.clone(), None, None, false)
        .expect("insert A");
    tc.cache
        .borrow_mut()
        .add_order(order_b.clone(), None, None, false)
        .expect("insert B");

    let order_list = OrderList::new(
        OrderListId::from("OL-1"),
        instrument_id,
        StrategyId::from("S-1"),
        vec![order_a.client_order_id(), order_b.client_order_id()],
        UnixNanos::default(),
    );

    let cmd = SubmitOrderList::new(
        TraderId::from("TRADER-001"),
        Some(ClientId::from("DERIVE")),
        StrategyId::from("S-1"),
        order_list,
        vec![
            OrderInitialized::from(&order_a),
            OrderInitialized::from(&order_b),
        ],
        None,
        None,
        None,
        UUID4::new(),
        UnixNanos::default(),
        None,
    );
    tc.client
        .submit_order_list(cmd)
        .expect("submit_order_list Ok");

    wait_until(
        || {
            let state = ws_state.clone();
            async move { state.submitted_orders.lock().await.len() >= 2 }
        },
        "two submit posts",
    )
    .await;

    let posts = ws_state.submitted_orders.lock().await;
    let labels: Vec<&str> = posts
        .iter()
        .map(|b| b["label"].as_str().unwrap_or(""))
        .collect();
    assert!(labels.contains(&"STRAT-LIST-A"));
    assert!(labels.contains(&"STRAT-LIST-B"));

    tc.client.disconnect().await.expect("disconnect");
}

#[rstest]
#[case::cancel_stale(0)]
#[case::cancel_missing(1)]
#[case::modify_stale(2)]
#[case::cancel_all_stale(3)]
#[case::modify_quantity_only(4)]
#[case::modify_price_only(5)]
#[tokio::test]
async fn test_commands_use_current_replacement_binding(#[case] operation: u8) {
    let ws_state = WsState::default();
    let cid = ClientOrderId::from("CURRENT-BINDING");
    let instrument_id = InstrumentId::from("ETH-PERP.DERIVE");
    let old_id = VenueOrderId::from("current-old");
    let current_id = VenueOrderId::from("ord-replaced-1");
    *ws_state.order_reply.lock().await = Some(json!({"result": {"order": order_json_with(
        old_id.as_str(), cid.as_str(), "buy", "ETH-PERP", 1_700_000_001_000, "open",
    )}}));
    let mut tc = build_client(RestState::default(), ws_state.clone()).await;
    tc.client.connect().await.unwrap();
    let order = build_limit_order(
        instrument_id,
        cid,
        OrderSide::Buy,
        Price::from("3500.00"),
        Quantity::from("1.000"),
    );
    add_order_to_cache(&tc.cache, order.clone(), Some(ClientId::from("DERIVE")));
    tc.client.submit_order(submit_cmd(&order)).unwrap();
    let accepted = drain_until(
        &mut tc.rx,
        |event| matches!(event, ExecutionEvent::Order(OrderEventAny::Accepted(_))),
        "accepted",
    )
    .await;

    let ExecutionEvent::Order(accepted) = accepted else {
        unreachable!()
    };

    tc.cache.borrow_mut().update_order(&accepted).unwrap();
    let mut first_replacement = order_json_with(
        current_id.as_str(),
        cid.as_str(),
        "buy",
        "ETH-PERP",
        1_700_000_002_000,
        "open",
    );
    first_replacement["replaced_order_id"] = json!(old_id.as_str());
    *ws_state.replace_reply.lock().await = Some(json!({"result": {
        "order": first_replacement,
        "cancelled_order": order_json_with(old_id.as_str(), cid.as_str(), "buy", "ETH-PERP", 1_700_000_002_000, "cancelled"),
    }}));
    tc.client
        .modify_order(ModifyOrder::new(
            TraderId::from("TRADER-001"),
            Some(ClientId::from("DERIVE")),
            StrategyId::from("S-1"),
            instrument_id,
            cid,
            Some(old_id),
            Some(Quantity::from("2.000")),
            Some(Price::from("3505.00")),
            None,
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ))
        .unwrap();
    let updated = drain_until(
        &mut tc.rx,
        |event| matches!(event, ExecutionEvent::Order(OrderEventAny::Updated(_))),
        "updated",
    )
    .await;

    let ExecutionEvent::Order(OrderEventAny::Updated(updated)) = updated else {
        unreachable!()
    };

    assert_eq!(updated.venue_order_id, Some(current_id));

    if matches!(operation, 2 | 4 | 5) {
        let mut replacement = order_json_with(
            "current-next",
            cid.as_str(),
            "buy",
            "ETH-PERP",
            1_700_000_003_000,
            "open",
        );
        replacement["replaced_order_id"] = json!(current_id.as_str());
        *ws_state.replace_reply.lock().await = Some(json!({"result": {"order": replacement,
            "cancelled_order": order_json_with(current_id.as_str(), cid.as_str(), "buy", "ETH-PERP", 1_700_000_003_000, "cancelled"),
        }}));
        tc.client
            .modify_order(ModifyOrder::new(
                TraderId::from("TRADER-001"),
                Some(ClientId::from("DERIVE")),
                StrategyId::from("S-1"),
                instrument_id,
                cid,
                Some(old_id),
                (operation != 5).then_some(Quantity::from("2.000")),
                (operation != 4).then_some(Price::from("3506.00")),
                None,
                UUID4::new(),
                UnixNanos::default(),
                None,
                None,
            ))
            .unwrap();
    } else if operation == 3 {
        tc.client
            .cancel_all_orders(CancelAllOrders::new(
                TraderId::from("TRADER-001"),
                Some(ClientId::from("DERIVE")),
                StrategyId::from("S-1"),
                instrument_id,
                Some(OrderSide::Buy),
                UUID4::new(),
                UnixNanos::default(),
                None,
                None,
            ))
            .unwrap();
    } else {
        tc.client
            .cancel_order(CancelOrder::new(
                TraderId::from("TRADER-001"),
                Some(ClientId::from("DERIVE")),
                StrategyId::from("S-1"),
                instrument_id,
                cid,
                (operation == 0).then_some(old_id),
                UUID4::new(),
                UnixNanos::default(),
                None,
                None,
            ))
            .unwrap();
    }

    wait_until(
        || {
            let state = ws_state.clone();
            async move {
                if matches!(operation, 2 | 4 | 5) {
                    state.replace_orders.lock().await.len() == 2
                } else {
                    state.cancelled_orders.lock().await.len()
                        + state.cancelled_labels.lock().await.len()
                        == 1
                }
            }
        },
        "next command posted",
    )
    .await;

    let requested_id = if matches!(operation, 2 | 4 | 5) {
        ws_state.replace_orders.lock().await[1]["order_id_to_cancel"].clone()
    } else {
        ws_state
            .cancelled_orders
            .lock()
            .await
            .first()
            .map_or(Value::Null, |request| request["order_id"].clone())
    };

    if matches!(operation, 2 | 4 | 5) {
        let writes = ws_state.replace_orders.lock().await;
        assert_eq!(writes[1]["amount"], json!("2.000"));

        let expected_price = if operation == 4 { "3505.00" } else { "3506.00" };
        assert_eq!(writes[1]["limit_price"], json!(expected_price));
    }

    tc.client.disconnect().await.unwrap();

    assert_eq!(requested_id, json!(current_id.as_str()));
    assert!(ws_state.cancelled_labels.lock().await.is_empty());
}

#[rstest]
#[tokio::test]
async fn test_cancel_order_calls_private_cancel() {
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    let mut tc = build_client(rest_state, ws_state.clone()).await;
    tc.client.connect().await.expect("connect succeeds");

    let cancel = CancelOrder::new(
        TraderId::from("TRADER-001"),
        Some(ClientId::from("DERIVE")),
        StrategyId::from("S-1"),
        InstrumentId::from("ETH-PERP.DERIVE"),
        ClientOrderId::from("O-1"),
        Some(VenueOrderId::from("ord-mock-1")),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );
    tc.client.cancel_order(cancel).expect("cancel_order Ok");

    wait_until(
        || {
            let state = ws_state.clone();
            async move { !state.cancelled_orders.lock().await.is_empty() }
        },
        "cancel posted",
    )
    .await;

    let posts = ws_state.cancelled_orders.lock().await;
    let body = &posts[0];
    assert_eq!(body["subaccount_id"].as_u64(), Some(TEST_SUBACCOUNT));
    assert_eq!(body["instrument_name"].as_str(), Some("ETH-PERP"));
    assert_eq!(body["order_id"].as_str(), Some("ord-mock-1"));

    tc.client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test]
async fn test_cancel_trigger_order_calls_private_cancel_trigger_order() {
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    let mut tc = build_client(rest_state, ws_state.clone()).await;
    tc.client.connect().await.expect("connect succeeds");

    let instrument_id = InstrumentId::from("ETH-PERP.DERIVE");
    let client_order_id = ClientOrderId::from("STRAT-CXL-TRIGGER");
    *ws_state.cancel_trigger_reply.lock().await = Some(json!({"result": trigger_order_json_with(
        "trig-cancel-1", client_order_id.as_str(), "buy", "ETH-PERP", 1_700_000_002_000,
        "market", "cancelled", "3500", "3400", "mark", "stoploss",
    )}));
    let order = build_stop_market_order(
        instrument_id,
        client_order_id,
        OrderSide::Buy,
        Price::from("3400.00"),
        Quantity::from("1.000"),
    );
    tc.cache
        .borrow_mut()
        .add_order(order, None, None, false)
        .expect("cache insert");

    let cancel = CancelOrder::new(
        TraderId::from("TRADER-001"),
        Some(ClientId::from("DERIVE")),
        StrategyId::from("S-1"),
        instrument_id,
        client_order_id,
        Some(VenueOrderId::from("trig-cancel-1")),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );
    tc.client.cancel_order(cancel).expect("cancel_order Ok");

    wait_until(
        || {
            let state = ws_state.clone();
            async move { !state.cancelled_trigger_orders.lock().await.is_empty() }
        },
        "private/cancel_trigger_order posted",
    )
    .await;

    let posts = ws_state.cancelled_trigger_orders.lock().await;
    let body = &posts[0];
    assert_eq!(body["subaccount_id"].as_u64(), Some(TEST_SUBACCOUNT));
    assert_eq!(body["order_id"].as_str(), Some("trig-cancel-1"));
    assert!(
        body.get("instrument_name").is_none(),
        "trigger cancel params must not include instrument_name",
    );
    assert!(
        ws_state.cancelled_orders.lock().await.is_empty(),
        "trigger cancel must not post private/cancel",
    );
    drop(posts);

    let event = drain_until(
        &mut tc.rx,
        |event| matches!(event, ExecutionEvent::Order(OrderEventAny::Canceled(_))),
        "OrderCanceled from trigger cancel response",
    )
    .await;

    if let ExecutionEvent::Order(OrderEventAny::Canceled(canceled)) = event {
        assert_eq!(canceled.client_order_id, client_order_id);
        assert_eq!(canceled.instrument_id, instrument_id);
        assert_eq!(
            canceled.venue_order_id.map(|id| id.to_string()),
            Some("trig-cancel-1".to_string()),
        );
    } else {
        unreachable!();
    }

    tc.client.disconnect().await.expect("disconnect");
}

#[rstest]
#[case::single(0)]
#[case::batch(1)]
#[case::side_all(2)]
#[case::all(3)]
#[tokio::test]
async fn test_cancel_trigger_uses_native_activation_before_cache_update(
    #[case] route: u8,
    #[values("pending", "order", "fill", "startup")] source: &str,
) {
    let state = RestState::default();
    let ws = WsState::default();
    let cid = ClientOrderId::from("TRIGGER-ACTIVATION");
    let native_id = VenueOrderId::from("trigger-activation-order");
    let instrument_id = InstrumentId::from("ETH-PERP.DERIVE");

    let native = trigger_order_json_with(
        native_id.as_str(),
        cid.as_str(),
        "buy",
        "ETH-PERP",
        1_700_000_002_000,
        "limit",
        if source == "startup" {
            "open"
        } else {
            "untriggered"
        },
        "3500",
        "3600",
        "mark",
        "stoploss",
    );

    state
        .get_order_responses
        .lock()
        .await
        .insert(native_id.to_string(), native.clone());
    *state.open_orders_response.lock().await = json!({"orders": if source == "startup" { vec![native.clone()] } else { vec![] }, "subaccount_id": TEST_SUBACCOUNT});
    *state.trigger_orders_response.lock().await = json!({"orders": if source == "startup" { vec![] } else { vec![native.clone()] }, "subaccount_id": TEST_SUBACCOUNT});
    let mut cancelled = native.clone();
    cancelled["order_status"] = json!("cancelled");
    *ws.cancel_trigger_reply.lock().await = Some(json!({"result": cancelled}));
    let mut tc = build_report_client(state, ws.clone()).await;
    if source != "startup" {
        tc.client.connect().await.unwrap();
    }

    let mut builder = OrderTestBuilder::new(OrderType::StopLimit);
    builder
        .trader_id(TraderId::from("TRADER-001"))
        .strategy_id(StrategyId::from("S-1"))
        .instrument_id(instrument_id)
        .client_order_id(cid)
        .side(OrderSide::Buy)
        .quantity(Quantity::from("1.000"))
        .price(Price::from("3500.00"))
        .trigger_price(Price::from("3600.00"))
        .trigger_type(TriggerType::MarkPrice);
    let restored = accepted_order(builder.build(), native_id, AccountId::from("DERIVE-001"));
    add_order_to_cache(&tc.cache, restored, Some(ClientId::from("DERIVE")));
    tc.cache.borrow_mut().build_index();
    if source == "startup" {
        tc.client.connect().await.unwrap();
    } else {
        tc.client.start().unwrap();
    }

    observe_trigger_activation(&mut tc, &ws, native, source, cid, native_id).await;
    assert_eq!(
        tc.cache.borrow().order(&cid).unwrap().status(),
        OrderStatus::Accepted
    );
    assert_eq!(
        tc.cache.borrow().order(&cid).unwrap().filled_qty(),
        Quantity::from("0.000")
    );

    let cancel = CancelOrder::new(
        TraderId::from("TRADER-001"),
        Some(ClientId::from("DERIVE")),
        StrategyId::from("S-1"),
        instrument_id,
        cid,
        Some(native_id),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );

    match route {
        0 => tc.client.cancel_order(cancel).unwrap(),
        1 => tc
            .client
            .batch_cancel_orders(BatchCancelOrders::new(
                TraderId::from("TRADER-001"),
                Some(ClientId::from("DERIVE")),
                StrategyId::from("S-1"),
                instrument_id,
                vec![cancel],
                UUID4::new(),
                UnixNanos::default(),
                None,
                None,
            ))
            .unwrap(),
        2 | 3 => tc
            .client
            .cancel_all_orders(CancelAllOrders::new(
                TraderId::from("TRADER-001"),
                Some(ClientId::from("DERIVE")),
                StrategyId::from("S-1"),
                instrument_id,
                (route == 2).then_some(OrderSide::Buy),
                UUID4::new(),
                UnixNanos::default(),
                None,
                None,
            ))
            .unwrap(),
        _ => unreachable!(),
    }

    wait_until(
        || {
            let state = ws.clone();
            async move {
                if route == 3 {
                    !state.cancel_by_instrument_calls.lock().await.is_empty()
                } else {
                    !state.cancelled_orders.lock().await.is_empty()
                        || !state.cancelled_trigger_orders.lock().await.is_empty()
                }
            }
        },
        "native cancellation endpoint",
    )
    .await;

    let triggers = ws.cancelled_trigger_orders.lock().await;
    let ordinary = ws.cancelled_orders.lock().await;
    let instruments = ws.cancel_by_instrument_calls.lock().await;
    assert_eq!(triggers.len(), usize::from(source == "pending"));
    assert_eq!(
        ordinary.len(),
        usize::from(source != "pending" && route != 3)
    );
    assert_eq!(instruments.len(), usize::from(route == 3));

    for body in triggers.iter().chain(ordinary.iter()) {
        assert_eq!(body["subaccount_id"], json!(TEST_SUBACCOUNT));
        assert_eq!(body["order_id"], json!(native_id.as_str()));
    }

    assert!(ws.cancelled_labels.lock().await.is_empty());
    drop(triggers);
    drop(ordinary);
    drop(instruments);
    tc.client.disconnect().await.unwrap();
}

async fn observe_trigger_activation(
    tc: &mut TestClient,
    ws: &WsState,
    native: Value,
    source: &str,
    cid: ClientOrderId,
    native_id: VenueOrderId,
) {
    if source == "fill" {
        let mut trade = trade_json_with_label(
            "activation-fill",
            native_id.as_str(),
            "ETH-PERP",
            cid.as_str(),
        );
        trade["trade_amount"] = json!("0.3");
        ws.push_notification(make_subscription_frame(
            &format!("{TEST_SUBACCOUNT}.trades"),
            &json!([trade]),
        ));
        let event = drain_until(
            &mut tc.rx,
            |event| matches!(event, ExecutionEvent::Order(OrderEventAny::Filled(_))),
            "native activation fill",
        )
        .await;

        let ExecutionEvent::Order(OrderEventAny::Filled(fill)) = event else {
            unreachable!()
        };

        assert_eq!(fill.client_order_id, cid);
        assert_eq!(fill.venue_order_id, native_id);
        assert_eq!(fill.last_qty, Quantity::from("0.300"));
        assert_eq!(fill.order_type, OrderType::StopLimit);
    } else if source != "startup" {
        let mut observed = native;
        if source == "order" {
            observed["order_status"] = json!("open");
        }

        ws.push_notification(make_subscription_frame(
            &format!("{TEST_SUBACCOUNT}.orders"),
            &json!([
                observed,
                order_json_with(
                    "activation-barrier",
                    "ACTIVATION-BARRIER",
                    "buy",
                    "ETH-PERP",
                    1_700_000_003_000,
                    "open"
                ),
            ]),
        ));
        let event = drain_until(
            &mut tc.rx,
            |event| matches!(event, ExecutionEvent::Report(ExecutionReport::Order(_))),
            "native activation barrier",
        )
        .await;

        let ExecutionEvent::Report(ExecutionReport::Order(report)) = event else {
            unreachable!()
        };

        assert_eq!(
            report.venue_order_id,
            VenueOrderId::from("activation-barrier")
        );
    }
}

#[rstest]
#[case::stale_pending(false)]
#[case::no_longer_pending(true)]
#[tokio::test]
async fn test_cancel_trigger_rechecks_activation_after_label_lookup(#[case] empty: bool) {
    let state = RestState::default();
    let ws = WsState::default();
    let mut tc = build_report_client(state.clone(), ws.clone()).await;
    tc.client.connect().await.unwrap();
    let cid = ClientOrderId::from("TRIGGER-LOOKUP-RACE");
    let native_id = VenueOrderId::from("trigger-lookup-race-order");
    let instrument_id = InstrumentId::from("ETH-PERP.DERIVE");
    let mut builder = OrderTestBuilder::new(OrderType::StopLimit);
    builder
        .trader_id(TraderId::from("TRADER-001"))
        .strategy_id(StrategyId::from("S-1"))
        .instrument_id(instrument_id)
        .client_order_id(cid)
        .side(OrderSide::Buy)
        .quantity(Quantity::from("1.000"))
        .price(Price::from("3500.00"))
        .trigger_price(Price::from("3600.00"))
        .trigger_type(TriggerType::MarkPrice);
    let mut order = builder.build();
    order
        .apply(OrderEventAny::Submitted(OrderSubmitted::new(
            TraderId::from("TRADER-001"),
            StrategyId::from("S-1"),
            instrument_id,
            cid,
            AccountId::from("DERIVE-001"),
            UUID4::new(),
            UnixNanos::from(1),
            UnixNanos::from(1),
        )))
        .unwrap();
    add_order_to_cache(&tc.cache, order, Some(ClientId::from("DERIVE")));
    tc.client.start().unwrap();
    let pending = trigger_order_json_with(
        native_id.as_str(),
        cid.as_str(),
        "buy",
        "ETH-PERP",
        1_700_000_002_000,
        "limit",
        "untriggered",
        "3500",
        "3600",
        "mark",
        "stoploss",
    );
    let mut response = state.trigger_orders_response.lock().await;
    *response = json!({"orders": if empty { vec![] } else { vec![pending.clone()] }, "subaccount_id": TEST_SUBACCOUNT});
    tc.client
        .cancel_order(CancelOrder::new(
            TraderId::from("TRADER-001"),
            Some(ClientId::from("DERIVE")),
            StrategyId::from("S-1"),
            instrument_id,
            cid,
            None,
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ))
        .unwrap();
    wait_until(
        || {
            let state = state.clone();
            async move { state.trigger_orders_calls.lock().await.len() == 3 }
        },
        "blocked trigger lookup",
    )
    .await;

    let mut activated = pending;
    activated["order_status"] = json!("open");
    ws.push_notification(make_subscription_frame(
        &format!("{TEST_SUBACCOUNT}.orders"),
        &json!([activated]),
    ));
    let event = drain_until(
        &mut tc.rx,
        |event| matches!(event, ExecutionEvent::Order(OrderEventAny::Accepted(_))),
        "activation before lookup response",
    )
    .await;

    let ExecutionEvent::Order(OrderEventAny::Accepted(accepted)) = event else {
        unreachable!()
    };

    assert_eq!(accepted.client_order_id, cid);
    assert_eq!(accepted.venue_order_id, native_id);
    drop(response);
    wait_until(
        || {
            let state = ws.clone();
            async move { !state.cancelled_orders.lock().await.is_empty() }
        },
        "ordinary cancellation after activation",
    )
    .await;

    let ordinary = ws.cancelled_orders.lock().await;
    assert_eq!(ordinary.len(), 1);
    assert_eq!(ordinary[0]["order_id"], json!(native_id.as_str()));
    assert_eq!(ordinary[0]["subaccount_id"], json!(TEST_SUBACCOUNT));
    assert_eq!(ordinary[0]["instrument_name"], json!("ETH-PERP"));
    assert!(ws.cancelled_trigger_orders.lock().await.is_empty());
    assert!(ws.cancelled_labels.lock().await.is_empty());
    assert_eq!(
        tc.cache.borrow().order(&cid).unwrap().status(),
        OrderStatus::Submitted
    );
    assert_eq!(
        tc.cache.borrow().order(&cid).unwrap().venue_order_id(),
        None
    );
    drop(ordinary);
    tc.client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_cancel_trigger_order_without_venue_id_resolves_label() {
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    let instrument_id = InstrumentId::from("ETH-PERP.DERIVE");
    let client_order_id = ClientOrderId::from("STRAT-CXL-TRIGGER-BY-LABEL");

    let mut tc = build_client(rest_state.clone(), ws_state.clone()).await;
    tc.client.connect().await.expect("connect succeeds");
    *rest_state.trigger_orders_response.lock().await = json!({
        "orders": [trigger_order_json_with(
            "trig-resolved-by-label",
            client_order_id.as_str(),
            "buy",
            "ETH-PERP",
            1_700_000_001_000,
            "market",
            "untriggered",
            "3417",
            "3400",
            "mark",
            "stoploss",
        )],
        "subaccount_id": TEST_SUBACCOUNT,
    });

    *ws_state.cancel_trigger_reply.lock().await = Some(json!({"result": trigger_order_json_with(
        "trig-resolved-by-label", client_order_id.as_str(), "buy", "ETH-PERP", 1_700_000_002_000,
        "market", "cancelled", "3500", "3400", "mark", "stoploss",
    )}));
    let order = build_stop_market_order(
        instrument_id,
        client_order_id,
        OrderSide::Buy,
        Price::from("3400.00"),
        Quantity::from("1.000"),
    );
    tc.cache
        .borrow_mut()
        .add_order(order, None, None, false)
        .expect("cache insert");

    let cancel = CancelOrder::new(
        TraderId::from("TRADER-001"),
        Some(ClientId::from("DERIVE")),
        StrategyId::from("S-1"),
        instrument_id,
        client_order_id,
        None,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );
    tc.client.cancel_order(cancel).expect("cancel_order Ok");

    let event = drain_until(
        &mut tc.rx,
        |event| matches!(event, ExecutionEvent::Order(OrderEventAny::Canceled(_))),
        "OrderCanceled from trigger resolved by label",
    )
    .await;
    let posts = ws_state.cancelled_trigger_orders.lock().await;

    assert_eq!(rest_state.trigger_orders_calls.lock().await.len(), 3);
    assert_eq!(posts.len(), 1);
    assert_eq!(
        posts[0]["order_id"].as_str(),
        Some("trig-resolved-by-label")
    );
    assert!(ws_state.cancelled_labels.lock().await.is_empty());

    if let ExecutionEvent::Order(OrderEventAny::Canceled(canceled)) = event {
        assert_eq!(canceled.client_order_id, client_order_id);
        assert_eq!(
            canceled.venue_order_id.map(|id| id.to_string()),
            Some("trig-resolved-by-label".to_string()),
        );
    } else {
        unreachable!();
    }

    tc.client.disconnect().await.expect("disconnect");
}

#[rstest]
#[case(
    json!({"orders": [], "subaccount_id": TEST_SUBACCOUNT}),
    "trigger order not found for client_order_id",
    1
)]
#[case(
    json!({"id": 1, "error": {"code": -32000, "message": "trigger lookup unavailable"}}),
    "failed to resolve trigger order by label",
    2
)]
#[tokio::test]
async fn test_cancel_trigger_order_without_venue_id_rejects_lookup_failure(
    #[case] trigger_orders_response: Value,
    #[case] expected_reason: &str,
    #[case] expected_calls: usize,
) {
    let rest_state = RestState::default();
    let ws_state = WsState::default();

    let mut tc = build_client(rest_state.clone(), ws_state.clone()).await;
    tc.client.connect().await.expect("connect succeeds");
    *rest_state.trigger_orders_response.lock().await = trigger_orders_response;

    let instrument_id = InstrumentId::from("ETH-PERP.DERIVE");
    let client_order_id = ClientOrderId::from("STRAT-CXL-TRIGGER-LOOKUP-FAIL");
    let order = build_stop_market_order(
        instrument_id,
        client_order_id,
        OrderSide::Buy,
        Price::from("3400.00"),
        Quantity::from("1.000"),
    );
    tc.cache
        .borrow_mut()
        .add_order(order, None, None, false)
        .expect("cache insert");

    let cancel = CancelOrder::new(
        TraderId::from("TRADER-001"),
        Some(ClientId::from("DERIVE")),
        StrategyId::from("S-1"),
        instrument_id,
        client_order_id,
        None,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );
    tc.client.cancel_order(cancel).expect("cancel_order Ok");

    let event = drain_until(
        &mut tc.rx,
        |event| {
            matches!(
                event,
                ExecutionEvent::Order(OrderEventAny::CancelRejected(_))
            )
        },
        "OrderCancelRejected from trigger lookup",
    )
    .await;

    tc.client.disconnect().await.expect("disconnect");

    assert_eq!(
        rest_state.trigger_orders_calls.lock().await.len(),
        expected_calls + 2
    );
    assert!(ws_state.cancelled_trigger_orders.lock().await.is_empty());
    assert!(ws_state.cancelled_labels.lock().await.is_empty());

    if let ExecutionEvent::Order(OrderEventAny::CancelRejected(rejected)) = event {
        assert_eq!(rejected.client_order_id, client_order_id);
        assert!(rejected.venue_order_id.is_none());
        assert!(rejected.reason.contains(expected_reason));
    } else {
        unreachable!();
    }
}

#[rstest]
#[case(-1)]
#[case(2)]
#[tokio::test]
async fn test_cancel_order_without_venue_id_calls_private_cancel_by_label(#[case] count: i64) {
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    *ws_state.cancel_by_label_reply.lock().await = Some(
        serde_json::from_str(include_str!(
            "../../test_data/common/ws_cancel_by_label_nonzero.json"
        ))
        .expect("nonzero cancel-by-label fixture is valid JSON"),
    );
    ws_state
        .cancel_by_label_reply
        .lock()
        .await
        .as_mut()
        .unwrap()["result"]["cancelled_orders"] = json!(count);
    let mut tc = build_client(rest_state, ws_state.clone()).await;
    tc.client.connect().await.expect("connect succeeds");

    let instrument_id = InstrumentId::from("ETH-PERP.DERIVE");
    let client_order_id = ClientOrderId::from("STRAT-CXL-BY-LABEL");
    let order = build_limit_order(
        instrument_id,
        client_order_id,
        OrderSide::Buy,
        Price::from("3500.00"),
        Quantity::from("1.000"),
    );
    tc.cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .expect("cache insert");
    tc.client
        .submit_order(submit_cmd(&order))
        .expect("submit Ok");
    let _ = drain_until(
        &mut tc.rx,
        |event| matches!(event, ExecutionEvent::Order(OrderEventAny::Submitted(_))),
        "OrderSubmitted",
    )
    .await;

    let cancel = CancelOrder::new(
        TraderId::from("TRADER-001"),
        Some(ClientId::from("DERIVE")),
        StrategyId::from("S-1"),
        instrument_id,
        client_order_id,
        None,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );
    tc.client.cancel_order(cancel).expect("cancel_order Ok");

    wait_until(
        || {
            let state = ws_state.clone();
            async move { !state.cancelled_labels.lock().await.is_empty() }
        },
        "private/cancel_by_label posted",
    )
    .await;

    let posts = ws_state.cancelled_labels.lock().await;
    assert_eq!(posts.len(), 1);
    assert_eq!(posts[0]["subaccount_id"].as_u64(), Some(TEST_SUBACCOUNT));
    assert_eq!(posts[0]["label"].as_str(), Some(client_order_id.as_str()));
    assert!(ws_state.cancelled_orders.lock().await.is_empty());
    drop(posts);

    let outcome = tokio::time::timeout(Duration::from_millis(200), async {
        loop {
            match tc.rx.recv().await {
                Some(ExecutionEvent::Order(OrderEventAny::Canceled(_))) => {
                    return Some("OrderCanceled before venue notification");
                }
                Some(ExecutionEvent::Order(OrderEventAny::CancelRejected(_))) => {
                    return Some("OrderCancelRejected for nonzero count");
                }
                Some(_) => {}
                None => return Some("execution event channel closed"),
            }
        }
    })
    .await;

    assert!(
        outcome.is_err(),
        "nonzero cancel-by-label must wait for venue notification, was {outcome:?}",
    );

    let orders_channel = format!("{TEST_SUBACCOUNT}.orders");
    let canceled_frame = json!([order_json_with(
        "ord-canceled-by-label",
        client_order_id.as_str(),
        "buy",
        "ETH-PERP",
        1_700_000_002_000_i64,
        "cancelled",
    )]);
    ws_state.push_notification(make_subscription_frame(&orders_channel, &canceled_frame));
    let event = drain_until(
        &mut tc.rx,
        |event| matches!(event, ExecutionEvent::Order(OrderEventAny::Canceled(_))),
        "OrderCanceled after cancel_by_label",
    )
    .await;

    if let ExecutionEvent::Order(OrderEventAny::Canceled(canceled)) = event {
        assert_eq!(canceled.client_order_id, client_order_id);
        assert_eq!(
            canceled.venue_order_id.map(|id| id.to_string()),
            Some("ord-canceled-by-label".to_string()),
        );
    } else {
        unreachable!();
    }

    tc.client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test]
async fn test_cancel_order_by_label_zero_count_emits_cancel_rejected() {
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    *ws_state.cancel_by_label_reply.lock().await = Some(
        serde_json::from_str(include_str!(
            "../../test_data/common/ws_cancel_by_label_zero.json"
        ))
        .expect("zero cancel-by-label fixture is valid JSON"),
    );
    let mut tc = build_client(rest_state, ws_state.clone()).await;
    tc.client.connect().await.expect("connect succeeds");

    let client_order_id = ClientOrderId::from("STRAT-CXL-BY-LABEL-ZERO");

    let cancel = CancelOrder::new(
        TraderId::from("TRADER-001"),
        Some(ClientId::from("DERIVE")),
        StrategyId::from("S-1"),
        InstrumentId::from("ETH-PERP.DERIVE"),
        client_order_id,
        None,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );
    tc.client.cancel_order(cancel).expect("cancel_order Ok");

    let event = drain_until(
        &mut tc.rx,
        |event| {
            matches!(
                event,
                ExecutionEvent::Order(OrderEventAny::CancelRejected(_))
            )
        },
        "OrderCancelRejected for zero cancel-by-label count",
    )
    .await;

    assert_eq!(ws_state.cancelled_labels.lock().await.len(), 1);

    if let ExecutionEvent::Order(OrderEventAny::CancelRejected(rejected)) = event {
        assert_eq!(rejected.client_order_id, client_order_id);
        assert!(rejected.venue_order_id.is_none());
        assert_eq!(
            rejected.reason,
            "no open order matched the client_order_id label"
        );
    } else {
        unreachable!();
    }

    tc.client.disconnect().await.expect("disconnect");
}

#[rstest]
#[case::internal_error(-32603)]
#[case::order_confirmation_timeout(9000)]
#[case::engine_confirmation_timeout(9001)]
#[tokio::test]
async fn test_cancel_order_by_label_jsonrpc_ambiguous_does_not_emit_cancel_rejected(
    #[case] code: i64,
) {
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    *ws_state.cancel_by_label_reply.lock().await = Some(json!({
        "error": {"code": code, "message": "Internal venue error"}
    }));
    let mut tc = build_client(rest_state, ws_state.clone()).await;
    tc.client.connect().await.expect("connect succeeds");

    let instrument_id = InstrumentId::from("ETH-PERP.DERIVE");
    let client_order_id = ClientOrderId::from("STRAT-CXL-BY-LABEL-AMBIGUOUS");
    let order = build_limit_order(
        instrument_id,
        client_order_id,
        OrderSide::Buy,
        Price::from("3500.00"),
        Quantity::from("1.000"),
    );
    tc.cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .expect("cache insert");
    tc.client
        .submit_order(submit_cmd(&order))
        .expect("submit Ok");
    let _ = drain_until(
        &mut tc.rx,
        |event| matches!(event, ExecutionEvent::Order(OrderEventAny::Submitted(_))),
        "OrderSubmitted",
    )
    .await;

    let cancel = CancelOrder::new(
        TraderId::from("TRADER-001"),
        Some(ClientId::from("DERIVE")),
        StrategyId::from("S-1"),
        instrument_id,
        client_order_id,
        None,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );
    tc.client.cancel_order(cancel).expect("cancel_order Ok");
    wait_until(
        || {
            let state = ws_state.clone();
            async move { !state.cancelled_labels.lock().await.is_empty() }
        },
        "private/cancel_by_label posted",
    )
    .await;

    let outcome = tokio::time::timeout(Duration::from_millis(200), async {
        loop {
            match tc.rx.recv().await {
                Some(ExecutionEvent::Order(OrderEventAny::CancelRejected(_))) => {
                    return Some("OrderCancelRejected for ambiguous outcome");
                }
                Some(_) => {}
                None => return Some("execution event channel closed"),
            }
        }
    })
    .await;

    assert!(
        outcome.is_err(),
        "ambiguous cancel-by-label must not emit a terminal rejection, was {outcome:?}",
    );

    let orders_channel = format!("{TEST_SUBACCOUNT}.orders");
    let canceled_frame = json!([order_json_with(
        "ord-canceled-after-ambiguous-label",
        client_order_id.as_str(),
        "buy",
        "ETH-PERP",
        1_700_000_002_000_i64,
        "cancelled",
    )]);
    ws_state.push_notification(make_subscription_frame(&orders_channel, &canceled_frame));
    let event = drain_until(
        &mut tc.rx,
        |event| matches!(event, ExecutionEvent::Order(OrderEventAny::Canceled(_))),
        "OrderCanceled after ambiguous cancel-by-label",
    )
    .await;

    if let ExecutionEvent::Order(OrderEventAny::Canceled(canceled)) = event {
        assert_eq!(canceled.client_order_id, client_order_id);
        assert_eq!(
            canceled.venue_order_id.map(|id| id.to_string()),
            Some("ord-canceled-after-ambiguous-label".to_string()),
        );
    } else {
        unreachable!();
    }

    tc.client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test]
async fn test_cancel_order_by_label_rejection_emits_cancel_rejected() {
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    *ws_state.cancel_by_label_reply.lock().await = Some(json!({
        "error": {"code": -32602, "message": "No order with label"}
    }));
    let mut tc = build_client(rest_state, ws_state.clone()).await;
    tc.client.connect().await.expect("connect succeeds");

    let client_order_id = ClientOrderId::from("STRAT-CXL-BY-LABEL-REJECT");

    let cancel = CancelOrder::new(
        TraderId::from("TRADER-001"),
        Some(ClientId::from("DERIVE")),
        StrategyId::from("S-1"),
        InstrumentId::from("ETH-PERP.DERIVE"),
        client_order_id,
        None,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );
    tc.client.cancel_order(cancel).expect("cancel_order Ok");

    let event = drain_until(
        &mut tc.rx,
        |event| {
            matches!(
                event,
                ExecutionEvent::Order(OrderEventAny::CancelRejected(_))
            )
        },
        "OrderCancelRejected from cancel_by_label",
    )
    .await;

    if let ExecutionEvent::Order(OrderEventAny::CancelRejected(rejected)) = event {
        assert_eq!(rejected.client_order_id, client_order_id);
        assert!(rejected.venue_order_id.is_none());
        assert!(rejected.reason.contains("No order with label"));
    } else {
        unreachable!();
    }

    tc.client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test]
async fn test_cancel_all_orders_without_side_sends_cancel_by_instrument_for_empty_book() {
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    *ws_state.cancel_by_instrument_reply.lock().await =
        Some(json!({"result": {"cancelled_orders": 0}}));
    let mut tc = build_client(rest_state.clone(), ws_state.clone()).await;
    tc.client.connect().await.expect("connect succeeds");

    let cmd = CancelAllOrders::new(
        TraderId::from("TRADER-001"),
        Some(ClientId::from("DERIVE")),
        StrategyId::from("S-1"),
        InstrumentId::from("ETH-PERP.DERIVE"),
        None,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );
    tc.client.cancel_all_orders(cmd).expect("cancel_all Ok");

    wait_until(
        || {
            let state = ws_state.clone();
            async move { !state.cancel_by_instrument_calls.lock().await.is_empty() }
        },
        "cancel_by_instrument posted",
    )
    .await;

    let posts = ws_state.cancel_by_instrument_calls.lock().await;
    assert_eq!(
        posts.as_slice(),
        &[json!({
            "subaccount_id": TEST_SUBACCOUNT,
            "instrument_name": "ETH-PERP",
        })],
    );
    assert!(ws_state.cancelled_orders.lock().await.is_empty());
    assert!(ws_state.cancelled_trigger_orders.lock().await.is_empty());
    assert!(ws_state.cancel_all_calls.lock().await.is_empty());
    assert!(rest_state.open_orders_calls.lock().await.is_empty());
    assert_eq!(rest_state.trigger_orders_calls.lock().await.len(), 2);

    tc.client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test]
async fn test_cancel_all_orders_without_side_cancels_matching_triggers() {
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    let mut tc = build_client(rest_state.clone(), ws_state.clone()).await;
    tc.client.connect().await.expect("connect succeeds");

    let account_id = AccountId::from("DERIVE-001");
    let client_id = Some(ClientId::from("DERIVE"));
    add_order_to_cache(
        &tc.cache,
        accepted_order(
            build_stop_market_order(
                InstrumentId::from("ETH-PERP.DERIVE"),
                ClientOrderId::from("TRIGGER-ETH"),
                OrderSide::Buy,
                Price::from("3450.00"),
                Quantity::from("1.000"),
            ),
            VenueOrderId::from("trig-eth"),
            account_id,
        ),
        client_id,
    );
    add_order_to_cache(
        &tc.cache,
        accepted_order(
            build_stop_market_order(
                InstrumentId::from("BTC-PERP.DERIVE"),
                ClientOrderId::from("TRIGGER-BTC"),
                OrderSide::Sell,
                Price::from("66000.00"),
                Quantity::from("1.000"),
            ),
            VenueOrderId::from("trig-btc"),
            account_id,
        ),
        client_id,
    );
    add_order_to_cache(
        &tc.cache,
        accepted_order(
            build_limit_order(
                InstrumentId::from("ETH-PERP.DERIVE"),
                ClientOrderId::from("REGULAR-ETH"),
                OrderSide::Buy,
                Price::from("3500.00"),
                Quantity::from("1.000"),
            ),
            VenueOrderId::from("regular-eth"),
            account_id,
        ),
        client_id,
    );
    tc.cache.borrow_mut().build_index();

    let cmd = CancelAllOrders::new(
        TraderId::from("TRADER-001"),
        Some(ClientId::from("DERIVE")),
        StrategyId::from("S-1"),
        InstrumentId::from("ETH-PERP.DERIVE"),
        None,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );
    tc.client.cancel_all_orders(cmd).expect("cancel_all Ok");

    wait_until(
        || {
            let state = ws_state.clone();
            async move {
                !state.cancel_by_instrument_calls.lock().await.is_empty()
                    && !state.cancelled_trigger_orders.lock().await.is_empty()
            }
        },
        "trigger and instrument cancels posted",
    )
    .await;

    assert_eq!(
        ws_state.cancelled_trigger_orders.lock().await.as_slice(),
        &[json!({
            "subaccount_id": TEST_SUBACCOUNT,
            "order_id": "trig-eth",
        })],
    );
    assert_eq!(
        ws_state.cancel_by_instrument_calls.lock().await.as_slice(),
        &[json!({
            "subaccount_id": TEST_SUBACCOUNT,
            "instrument_name": "ETH-PERP",
        })],
    );
    assert!(ws_state.cancelled_orders.lock().await.is_empty());
    assert!(ws_state.cancel_all_calls.lock().await.is_empty());
    assert!(rest_state.open_orders_calls.lock().await.is_empty());
    assert_eq!(rest_state.trigger_orders_calls.lock().await.len(), 2);

    tc.client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test]
async fn test_cancel_all_orders_trigger_failure_still_sends_cancel_by_instrument() {
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    *ws_state.cancel_trigger_reply.lock().await = Some(json!({
        "error": {"code": -32603, "message": "Internal venue error"}
    }));
    let mut tc = build_client(rest_state.clone(), ws_state.clone()).await;
    tc.client.connect().await.expect("connect succeeds");

    add_order_to_cache(
        &tc.cache,
        accepted_order(
            build_stop_market_order(
                InstrumentId::from("ETH-PERP.DERIVE"),
                ClientOrderId::from("TRIGGER-ETH"),
                OrderSide::Buy,
                Price::from("3450.00"),
                Quantity::from("1.000"),
            ),
            VenueOrderId::from("trig-eth"),
            AccountId::from("DERIVE-001"),
        ),
        Some(ClientId::from("DERIVE")),
    );
    tc.cache.borrow_mut().build_index();

    let cmd = CancelAllOrders::new(
        TraderId::from("TRADER-001"),
        Some(ClientId::from("DERIVE")),
        StrategyId::from("S-1"),
        InstrumentId::from("ETH-PERP.DERIVE"),
        None,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );
    tc.client.cancel_all_orders(cmd).expect("cancel_all Ok");

    wait_until(
        || {
            let state = ws_state.clone();
            async move {
                !state.cancelled_trigger_orders.lock().await.is_empty()
                    && !state.cancel_by_instrument_calls.lock().await.is_empty()
            }
        },
        "trigger and instrument cancels posted",
    )
    .await;

    assert_eq!(
        ws_state.cancelled_trigger_orders.lock().await.as_slice(),
        &[json!({
            "subaccount_id": TEST_SUBACCOUNT,
            "order_id": "trig-eth",
        })],
    );
    assert_eq!(
        ws_state.cancel_by_instrument_calls.lock().await.as_slice(),
        &[json!({
            "subaccount_id": TEST_SUBACCOUNT,
            "instrument_name": "ETH-PERP",
        })],
    );

    let outcome = tokio::time::timeout(Duration::from_millis(200), async {
        loop {
            match tc.rx.recv().await {
                Some(ExecutionEvent::Order(event)) => return Some(event),
                Some(_) => {}
                None => return None,
            }
        }
    })
    .await;

    assert!(
        outcome.is_err(),
        "cancel-all failure must not emit a per-order event, was {outcome:?}",
    );
    assert!(rest_state.open_orders_calls.lock().await.is_empty());
    assert_eq!(rest_state.trigger_orders_calls.lock().await.len(), 2);

    tc.client.disconnect().await.expect("disconnect");
}

#[rstest]
#[case(OrderSide::Buy, "buy-eth", "trig-buy-eth")]
#[case(OrderSide::Sell, "sell-eth", "trig-sell-eth")]
#[tokio::test]
async fn test_cancel_all_orders_side_filter_iterates_matching_open_orders(
    #[case] side: OrderSide,
    #[case] regular_order_id: &str,
    #[case] trigger_order_id: &str,
) {
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    let mut tc = build_client(rest_state.clone(), ws_state.clone()).await;
    tc.client.connect().await.expect("connect succeeds");

    let instrument_id = InstrumentId::from("ETH-PERP.DERIVE");
    let other_instrument_id = InstrumentId::from("BTC-PERP.DERIVE");
    let account_id = AccountId::from("DERIVE-001");
    let other_account_id = AccountId::from("DERIVE-OTHER");
    let client_id = Some(ClientId::from("DERIVE"));
    let other_client_id = Some(ClientId::from("DERIVE-OTHER"));
    let regular_orders = [
        (
            "L-BUY-ETH",
            "buy-eth",
            OrderSide::Buy,
            instrument_id,
            account_id,
            client_id,
        ),
        (
            "L-SELL-ETH",
            "sell-eth",
            OrderSide::Sell,
            instrument_id,
            account_id,
            client_id,
        ),
        (
            "L-BUY-BTC",
            "buy-btc",
            OrderSide::Buy,
            other_instrument_id,
            account_id,
            client_id,
        ),
        (
            "L-BUY-OTHER-ACCOUNT",
            "buy-other-account",
            OrderSide::Buy,
            instrument_id,
            other_account_id,
            client_id,
        ),
        (
            "L-SELL-OTHER-ACCOUNT",
            "sell-other-account",
            OrderSide::Sell,
            instrument_id,
            other_account_id,
            client_id,
        ),
        (
            "L-BUY-OTHER-CLIENT",
            "buy-other-client",
            OrderSide::Buy,
            instrument_id,
            account_id,
            other_client_id,
        ),
        (
            "L-SELL-UNCLAIMED",
            "sell-unclaimed",
            OrderSide::Sell,
            instrument_id,
            account_id,
            None,
        ),
    ];

    for (order_id, venue_order_id, order_side, instrument, account, owner) in regular_orders {
        add_order_to_cache(
            &tc.cache,
            accepted_order(
                build_limit_order(
                    instrument,
                    ClientOrderId::from(order_id),
                    order_side,
                    Price::from("3500.00"),
                    Quantity::from("1.000"),
                ),
                VenueOrderId::from(venue_order_id),
                account,
            ),
            owner,
        );
    }

    for (order_id, venue_order_id, order_side) in [
        ("T-BUY-ETH", "trig-buy-eth", OrderSide::Buy),
        ("T-SELL-ETH", "trig-sell-eth", OrderSide::Sell),
    ] {
        add_order_to_cache(
            &tc.cache,
            accepted_order(
                build_stop_market_order(
                    instrument_id,
                    ClientOrderId::from(order_id),
                    order_side,
                    Price::from("3450.00"),
                    Quantity::from("1.000"),
                ),
                VenueOrderId::from(venue_order_id),
                account_id,
            ),
            client_id,
        );
    }

    tc.cache.borrow_mut().build_index();

    let cmd = CancelAllOrders::new(
        TraderId::from("TRADER-001"),
        Some(ClientId::from("DERIVE")),
        StrategyId::from("S-1"),
        instrument_id,
        Some(side),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );
    tc.client.cancel_all_orders(cmd).expect("cancel_all Ok");

    wait_until(
        || {
            let state = ws_state.clone();
            async move {
                !state.cancelled_orders.lock().await.is_empty()
                    && !state.cancelled_trigger_orders.lock().await.is_empty()
            }
        },
        "filtered cancels posted",
    )
    .await;

    let posts = ws_state.cancelled_orders.lock().await;
    assert_eq!(posts.len(), 1, "expected exactly one filtered cancel");
    let body = &posts[0];
    assert_eq!(body["order_id"].as_str(), Some(regular_order_id));
    assert_eq!(body["instrument_name"].as_str(), Some("ETH-PERP"));
    drop(posts);
    assert_eq!(
        ws_state.cancelled_trigger_orders.lock().await.as_slice(),
        &[json!({
            "subaccount_id": TEST_SUBACCOUNT,
            "order_id": trigger_order_id,
        })],
    );
    assert!(ws_state.cancel_by_instrument_calls.lock().await.is_empty(),);
    assert!(ws_state.cancel_all_calls.lock().await.is_empty());
    assert!(rest_state.open_orders_calls.lock().await.is_empty());
    assert_eq!(rest_state.trigger_orders_calls.lock().await.len(), 2);

    tc.client.disconnect().await.expect("disconnect");
}

#[rstest]
#[case(Some(OrderSide::Buy), OrderType::Limit)]
#[case(None, OrderType::StopMarket)]
#[tokio::test]
async fn test_cancel_all_orders_missing_cached_venue_id_fails_closed(
    #[case] order_side: Option<OrderSide>,
    #[case] order_type: OrderType,
) {
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    let mut tc = build_client(rest_state.clone(), ws_state.clone()).await;
    tc.client.connect().await.expect("connect succeeds");

    let instrument_id = InstrumentId::from("ETH-PERP.DERIVE");

    let build_order = |client_order_id| match order_type {
        OrderType::Limit => build_limit_order(
            instrument_id,
            client_order_id,
            OrderSide::Buy,
            Price::from("3500.00"),
            Quantity::from("1.000"),
        ),
        OrderType::StopMarket => build_stop_market_order(
            instrument_id,
            client_order_id,
            OrderSide::Buy,
            Price::from("3450.00"),
            Quantity::from("1.000"),
        ),
        _ => unreachable!(),
    };

    let account_id = AccountId::from("DERIVE-001");
    let client_id = Some(ClientId::from("DERIVE"));
    let valid = accepted_order(
        build_order(ClientOrderId::from("VALID")),
        VenueOrderId::from("valid-venue-id"),
        account_id,
    );
    let mut missing = accepted_order(
        build_order(ClientOrderId::from("MISSING")),
        VenueOrderId::from("removed-venue-id"),
        account_id,
    );

    match &mut missing {
        OrderAny::Limit(order) => order.venue_order_id = None,
        OrderAny::StopMarket(order) => order.venue_order_id = None,
        _ => unreachable!(),
    }

    add_order_to_cache(&tc.cache, valid, client_id);
    add_order_to_cache(&tc.cache, missing, client_id);
    tc.cache.borrow_mut().build_index();

    let cmd = CancelAllOrders::new(
        TraderId::from("TRADER-001"),
        Some(ClientId::from("DERIVE")),
        StrategyId::from("S-1"),
        instrument_id,
        order_side,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );
    tc.client.cancel_all_orders(cmd).expect("cancel_all Ok");
    tokio::time::sleep(Duration::from_millis(100)).await;

    assert!(ws_state.cancelled_orders.lock().await.is_empty());
    assert!(ws_state.cancelled_trigger_orders.lock().await.is_empty());
    assert!(ws_state.cancel_by_instrument_calls.lock().await.is_empty());
    assert!(ws_state.cancel_all_calls.lock().await.is_empty());
    assert!(rest_state.open_orders_calls.lock().await.is_empty());
    assert_eq!(rest_state.trigger_orders_calls.lock().await.len(), 2);

    tc.client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test]
async fn test_modify_order_posts_replace_and_emits_order_updated() {
    let rest_state = RestState::default();
    rest_state.get_order_responses.lock().await.insert(
        "ord-stale-1".to_string(),
        order_json_with(
            "ord-stale-1",
            "STRAT-MOD-1",
            "buy",
            "ETH-PERP",
            1_700_000_001_000,
            "open",
        ),
    );
    let ws_state = WsState::default();
    let mut tc = build_client(rest_state, ws_state.clone()).await;
    tc.client.connect().await.expect("connect succeeds");

    let instrument_id = InstrumentId::from("ETH-PERP.DERIVE");
    let client_order_id = ClientOrderId::from("STRAT-MOD-1");
    let order = build_limit_order(
        instrument_id,
        client_order_id,
        OrderSide::Buy,
        Price::from("3500.00"),
        Quantity::from("1.000"),
    );
    tc.cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .expect("cache insert");
    // Drain the initial account-state event emitted at connect.
    let _ = drain_until(
        &mut tc.rx,
        |e| matches!(e, ExecutionEvent::Account(_)),
        "initial AccountState",
    )
    .await;

    let cmd = ModifyOrder::new(
        TraderId::from("TRADER-001"),
        Some(ClientId::from("DERIVE")),
        StrategyId::from("S-1"),
        instrument_id,
        client_order_id,
        Some(VenueOrderId::from("ord-stale-1")),
        Some(Quantity::from("2.000")),
        Some(Price::from("3505.00")),
        None,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );
    tc.client.modify_order(cmd).expect("modify_order Ok");

    let event = drain_until(
        &mut tc.rx,
        |e| matches!(e, ExecutionEvent::Order(OrderEventAny::Updated(_))),
        "OrderUpdated event",
    )
    .await;

    if let ExecutionEvent::Order(OrderEventAny::Updated(updated)) = event {
        assert_eq!(updated.client_order_id, client_order_id);
        assert_eq!(updated.quantity, Quantity::from("2.000"));
        assert_eq!(updated.price, Some(Price::from("3505.00")));
        // Mock response carries `order.order_id = ord-replaced-1`.
        assert_eq!(
            updated.venue_order_id.map(|v| v.as_str().to_string()),
            Some("ord-replaced-1".to_string()),
        );
    } else {
        unreachable!();
    }

    // Exactly one replace request was sent, with the stale id in the cancel
    // clause and the new quantity/price in the signed envelope.
    let replaces = ws_state.replace_orders.lock().await;
    assert_eq!(replaces.len(), 1, "expected exactly one replace request");
    let body = &replaces[0];
    assert_eq!(body["order_id_to_cancel"].as_str(), Some("ord-stale-1"));
    assert_eq!(body["instrument_name"].as_str(), Some("ETH-PERP"));
    assert_eq!(body["direction"].as_str(), Some("buy"));
    assert_eq!(body["amount"].as_str(), Some("2.000"));
    assert_eq!(body["limit_price"].as_str(), Some("3505.00"));
    assert_eq!(body["label"].as_str(), Some("STRAT-MOD-1"));
    assert!(body["signature"].as_str().unwrap().starts_with("0x"));
    // The legacy cancel-only fallback must not fire any more.
    assert!(ws_state.cancelled_orders.lock().await.is_empty());

    tc.client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test]
async fn test_modify_order_rejects_missing_cached_order_with_canonical_reason() {
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    let mut tc = build_client(rest_state, ws_state).await;

    let instrument_id = InstrumentId::from("ETH-PERP.DERIVE");
    let client_order_id = ClientOrderId::from("STRAT-MOD-MISSING");

    let cmd = ModifyOrder::new(
        TraderId::from("TRADER-001"),
        Some(ClientId::from("DERIVE")),
        StrategyId::from("S-1"),
        instrument_id,
        client_order_id,
        Some(VenueOrderId::from("ord-missing-cache")),
        Some(Quantity::from("2.000")),
        Some(Price::from("3505.00")),
        None,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );
    tc.client.modify_order(cmd).expect("modify_order Ok");

    let event = drain_until(
        &mut tc.rx,
        |e| matches!(e, ExecutionEvent::Order(OrderEventAny::ModifyRejected(_))),
        "OrderModifyRejected event",
    )
    .await;

    if let ExecutionEvent::Order(OrderEventAny::ModifyRejected(rejected)) = event {
        assert_eq!(rejected.client_order_id, client_order_id);
        assert_eq!(rejected.reason, ORDER_NOT_FOUND);
        assert_eq!(
            rejected.venue_order_id.map(|v| v.as_str().to_string()),
            Some("ord-missing-cache".to_string()),
        );
    } else {
        unreachable!();
    }

    tc.client.stop().expect("stop");
}

#[rstest]
#[case(
    MIN_SIGNATURE_TTL.as_secs(),
    "must be greater than the Derive minimum"
)]
#[case(
    MIN_SIGNATURE_TTL.as_secs() - 1,
    "must be greater than the Derive minimum"
)]
#[case(i64::MAX as u64, "exceeds the Derive maximum")]
#[tokio::test]
async fn test_modify_order_rejects_invalid_signature_ttl_before_posting_replace(
    #[case] signature_expiry_secs: u64,
    #[case] reason_fragment: &str,
) {
    let rest_state = RestState::default();
    let ws_state = WsState::default();

    let mut tc = build_client_with_config(rest_state, ws_state.clone(), None, |mut config| {
        config.signature_expiry_secs = signature_expiry_secs;
        config
    })
    .await;

    tc.client.connect().await.expect("connect succeeds");

    let instrument_id = InstrumentId::from("ETH-PERP.DERIVE");
    let client_order_id = ClientOrderId::from("STRAT-MOD-BAD-TTL");
    let order = build_limit_order(
        instrument_id,
        client_order_id,
        OrderSide::Buy,
        Price::from("3500.00"),
        Quantity::from("1.000"),
    );
    tc.cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .expect("cache insert");
    let _ = drain_until(
        &mut tc.rx,
        |e| matches!(e, ExecutionEvent::Account(_)),
        "initial AccountState",
    )
    .await;

    let cmd = ModifyOrder::new(
        TraderId::from("TRADER-001"),
        Some(ClientId::from("DERIVE")),
        StrategyId::from("S-1"),
        instrument_id,
        client_order_id,
        Some(VenueOrderId::from("ord-stale-overflow")),
        Some(Quantity::from("2.000")),
        Some(Price::from("3505.00")),
        None,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );
    tc.client.modify_order(cmd).expect("modify_order Ok");

    let event = drain_until(
        &mut tc.rx,
        |e| matches!(e, ExecutionEvent::Order(OrderEventAny::ModifyRejected(_))),
        "OrderModifyRejected event",
    )
    .await;

    if let ExecutionEvent::Order(OrderEventAny::ModifyRejected(rejected)) = event {
        let reason = rejected.reason.as_str();
        assert!(
            reason.contains("replace expiry validation failed") && reason.contains(reason_fragment),
            "unexpected reject reason: {reason}",
        );
        assert_eq!(
            rejected.venue_order_id.map(|v| v.as_str().to_string()),
            Some("ord-stale-overflow".to_string()),
        );
    } else {
        unreachable!();
    }

    assert!(
        ws_state.replace_orders.lock().await.is_empty(),
        "invalid signature TTL must not post private/replace",
    );
    assert!(
        ws_state.cancelled_orders.lock().await.is_empty(),
        "invalid signature TTL must not fall back to private/cancel",
    );

    tc.client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test]
async fn test_modify_order_suppresses_replace_cancel_leg() {
    // Regression: Derive's `private/replace` cancels the old order and opens a
    // new one under the same label. The `.orders` channel pushes the old
    // order's cancellation, which must NOT terminate the order: `modify_order`
    // rebinds it to the replacement via OrderUpdated, and the cancel-of-old leg
    // is suppressed. Mirrors the Hyperliquid GH-3827 cancel-replace handling.
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    let mut tc = build_client(rest_state, ws_state.clone()).await;
    tc.client.connect().await.expect("connect succeeds");

    let instrument_id = InstrumentId::from("ETH-PERP.DERIVE");
    let client_order_id = ClientOrderId::from("STRAT-MOD-SUPPRESS");
    let order = build_limit_order(
        instrument_id,
        client_order_id,
        OrderSide::Buy,
        Price::from("3500.00"),
        Quantity::from("1.000"),
    );
    tc.cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .expect("cache insert");
    let _ = drain_until(
        &mut tc.rx,
        |e| matches!(e, ExecutionEvent::Account(_)),
        "initial AccountState",
    )
    .await;

    // Submit to register the tracked identity, then accept via an `.orders`
    // Open frame so the order binds to venue_order_id `ord-stale-1`.
    tc.client
        .submit_order(submit_cmd(&order))
        .expect("submit Ok");
    let _ = drain_until(
        &mut tc.rx,
        |e| matches!(e, ExecutionEvent::Order(OrderEventAny::Submitted(_))),
        "OrderSubmitted",
    )
    .await;

    let orders_channel = format!("{TEST_SUBACCOUNT}.orders");
    let open_frame = json!([order_json_with(
        "ord-stale-1",
        client_order_id.as_str(),
        "buy",
        "ETH-PERP",
        1_700_000_001_000_i64,
        "open",
    )]);
    ws_state.push_notification(make_subscription_frame(&orders_channel, &open_frame));
    let _ = drain_until(
        &mut tc.rx,
        |e| matches!(e, ExecutionEvent::Order(OrderEventAny::Accepted(_))),
        "OrderAccepted",
    )
    .await;

    // Modify: the default replace reply rebinds to `ord-replaced-1`.
    let cmd = ModifyOrder::new(
        TraderId::from("TRADER-001"),
        Some(ClientId::from("DERIVE")),
        StrategyId::from("S-1"),
        instrument_id,
        client_order_id,
        Some(VenueOrderId::from("ord-stale-1")),
        Some(Quantity::from("2.000")),
        Some(Price::from("3505.00")),
        None,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );
    tc.client.modify_order(cmd).expect("modify_order Ok");
    let updated = drain_until(
        &mut tc.rx,
        |e| matches!(e, ExecutionEvent::Order(OrderEventAny::Updated(_))),
        "OrderUpdated",
    )
    .await;

    if let ExecutionEvent::Order(OrderEventAny::Updated(updated)) = updated {
        assert_eq!(
            updated.venue_order_id.map(|v| v.as_str().to_string()),
            Some("ord-replaced-1".to_string()),
        );
    } else {
        unreachable!();
    }

    // The venue now pushes the replace's cancel-of-old leg on the `.orders`
    // channel. With the order rebound to `ord-replaced-1`, this stale cancel of
    // `ord-stale-1` must be suppressed (no OrderCanceled).
    let cancel_frame = json!([order_json_with(
        "ord-stale-1",
        client_order_id.as_str(),
        "buy",
        "ETH-PERP",
        1_700_000_002_000_i64,
        "cancelled",
    )]);
    ws_state.push_notification(make_subscription_frame(&orders_channel, &cancel_frame));

    let canceled = tokio::time::timeout(Duration::from_millis(300), async {
        loop {
            match tc.rx.recv().await {
                Some(ExecutionEvent::Order(OrderEventAny::Canceled(_))) => return true,
                Some(_) => {}
                None => return false,
            }
        }
    })
    .await;

    assert!(
        canceled.is_err(),
        "the replace's cancel-of-old leg must not emit OrderCanceled",
    );

    tc.client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test]
async fn test_modify_order_preserves_original_on_rejected_child_before_rpc_response() {
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    *ws_state.replace_reply.lock().await = Some(json!({
        "error": {"code": 11008, "message": "Post only order cannot cross the market"}
    }));
    let mut tc = build_client(rest_state, ws_state.clone()).await;
    tc.client.connect().await.expect("connect succeeds");

    let instrument_id = InstrumentId::from("ETH-PERP.DERIVE");
    let client_order_id = ClientOrderId::from("STRAT-MOD-EARLY-REJECT");
    let order = build_limit_order_with_time_in_force(
        instrument_id,
        client_order_id,
        OrderSide::Buy,
        Price::from("3500.00"),
        Quantity::from("1.000"),
        TimeInForce::Gtc,
        true,
    );
    tc.cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .expect("cache insert");
    let _ = drain_until(
        &mut tc.rx,
        |event| matches!(event, ExecutionEvent::Account(_)),
        "initial AccountState",
    )
    .await;

    tc.client
        .submit_order(submit_cmd(&order))
        .expect("submit Ok");
    let _ = drain_until(
        &mut tc.rx,
        |event| matches!(event, ExecutionEvent::Order(OrderEventAny::Submitted(_))),
        "OrderSubmitted",
    )
    .await;

    let orders_channel = format!("{TEST_SUBACCOUNT}.orders");
    let open_frame = json!([order_json_with(
        "ord-before-replace",
        client_order_id.as_str(),
        "buy",
        "ETH-PERP",
        1_700_000_001_000_i64,
        "open",
    )]);
    ws_state.push_notification(make_subscription_frame(&orders_channel, &open_frame));
    let _ = drain_until(
        &mut tc.rx,
        |event| matches!(event, ExecutionEvent::Order(OrderEventAny::Accepted(_))),
        "OrderAccepted",
    )
    .await;

    let mut rejected_order = order_json_with(
        "ord-replacement-rejected",
        client_order_id.as_str(),
        "buy",
        "ETH-PERP",
        1_700_000_002_000_i64,
        "rejected",
    );
    rejected_order["replaced_order_id"] = json!("ord-before-replace");
    rejected_order["cancel_reason"] = json!("Post only order cannot cross the market");
    rejected_order["time_in_force"] = json!("post_only");
    *ws_state.replace_notification_before_reply.lock().await = Some(make_subscription_frame(
        &orders_channel,
        &json!([rejected_order]),
    ));

    let cmd = ModifyOrder::new(
        TraderId::from("TRADER-001"),
        Some(ClientId::from("DERIVE")),
        StrategyId::from("S-1"),
        instrument_id,
        client_order_id,
        Some(VenueOrderId::from("ord-before-replace")),
        Some(Quantity::from("2.000")),
        Some(Price::from("3505.00")),
        None,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );
    tc.client.modify_order(cmd).expect("modify_order Ok");

    let event = drain_until(
        &mut tc.rx,
        |event| {
            matches!(
                event,
                ExecutionEvent::Order(OrderEventAny::ModifyRejected(_))
            )
        },
        "definitive command rejection after unaccepted child",
    )
    .await;

    if let ExecutionEvent::Order(OrderEventAny::ModifyRejected(rejected)) = event {
        assert_eq!(rejected.client_order_id, client_order_id);
        assert_eq!(
            rejected.venue_order_id,
            Some(VenueOrderId::from("ord-before-replace"))
        );
        assert_eq!(
            rejected.reason,
            "11008: Post only order cannot cross the market"
        );
    } else {
        unreachable!();
    }

    let late_terminal = tokio::time::timeout(Duration::from_millis(200), async {
        loop {
            match tc.rx.recv().await {
                Some(ExecutionEvent::Order(
                    OrderEventAny::Updated(_) | OrderEventAny::Rejected(_),
                )) => return true,
                Some(_) => {}
                None => return false,
            }
        }
    })
    .await;

    assert!(
        late_terminal.is_err(),
        "a rejected child must not update or reject the original order",
    );

    tc.client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test]
async fn test_modify_order_accepts_replacement_open_before_rpc_response() {
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    *ws_state.replace_reply.lock().await = Some(json!({
        "result": {
            "order": order_json_with(
                "ord-replaced-1",
                "STRAT-O-1",
                "buy",
                "ETH-PERP",
                1_700_000_003_000_i64,
                "open",
            ),
            "cancelled_order": order_json_with(
                "ord-before-replace",
                "STRAT-O-1",
                "buy",
                "ETH-PERP",
                1_700_000_002_000_i64,
                "cancelled",
            ),
        },
    }));
    let mut tc = build_client(rest_state, ws_state.clone()).await;
    tc.client.connect().await.expect("connect succeeds");

    let instrument_id = InstrumentId::from("ETH-PERP.DERIVE");
    let client_order_id = ClientOrderId::from("STRAT-O-1");
    let order = build_limit_order(
        instrument_id,
        client_order_id,
        OrderSide::Buy,
        Price::from("3500.00"),
        Quantity::from("1.000"),
    );
    tc.cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .expect("cache insert");
    let _ = drain_until(
        &mut tc.rx,
        |event| matches!(event, ExecutionEvent::Account(_)),
        "initial AccountState",
    )
    .await;

    tc.client
        .submit_order(submit_cmd(&order))
        .expect("submit Ok");
    let _ = drain_until(
        &mut tc.rx,
        |event| matches!(event, ExecutionEvent::Order(OrderEventAny::Submitted(_))),
        "OrderSubmitted",
    )
    .await;

    let orders_channel = format!("{TEST_SUBACCOUNT}.orders");
    let open_frame = json!([order_json_with(
        "ord-before-replace",
        client_order_id.as_str(),
        "buy",
        "ETH-PERP",
        1_700_000_001_000_i64,
        "open",
    )]);
    ws_state.push_notification(make_subscription_frame(&orders_channel, &open_frame));
    let _ = drain_until(
        &mut tc.rx,
        |event| matches!(event, ExecutionEvent::Order(OrderEventAny::Accepted(_))),
        "OrderAccepted",
    )
    .await;

    let mut replacement_order = order_json_with(
        "ord-replaced-1",
        client_order_id.as_str(),
        "buy",
        "ETH-PERP",
        1_700_000_002_000_i64,
        "open",
    );
    replacement_order["replaced_order_id"] = json!("ord-before-replace");
    let replacement_frame = json!([replacement_order]);
    *ws_state.replace_notification_before_reply.lock().await =
        Some(make_subscription_frame(&orders_channel, &replacement_frame));

    let cmd = ModifyOrder::new(
        TraderId::from("TRADER-001"),
        Some(ClientId::from("DERIVE")),
        StrategyId::from("S-1"),
        instrument_id,
        client_order_id,
        Some(VenueOrderId::from("ord-before-replace")),
        Some(Quantity::from("2.000")),
        Some(Price::from("3505.00")),
        None,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );
    tc.client.modify_order(cmd).expect("modify_order Ok");

    let mut duplicate_accepted = false;

    let updated = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            match tc.rx.recv().await {
                Some(ExecutionEvent::Order(OrderEventAny::Accepted(_))) => {
                    duplicate_accepted = true;
                }
                Some(ExecutionEvent::Order(OrderEventAny::Updated(updated))) => return updated,
                Some(_) => {}
                None => panic!("event channel closed before OrderUpdated"),
            }
        }
    })
    .await
    .expect("OrderUpdated after replacement Open frame");

    assert!(
        !duplicate_accepted,
        "replacement emitted a second OrderAccepted"
    );
    assert_eq!(
        updated.venue_order_id,
        Some(VenueOrderId::from("ord-replaced-1"))
    );

    tc.client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test]
async fn test_modify_order_unexpected_response_shape_does_not_emit_updated() {
    let rest_state = RestState::default();
    rest_state.get_order_responses.lock().await.insert(
        "ord-stale-ambig".to_string(),
        order_json_with(
            "ord-stale-ambig",
            "STRAT-MOD-AMBIG",
            "buy",
            "ETH-PERP",
            1_700_000_001_000,
            "open",
        ),
    );
    let ws_state = WsState::default();
    // Venue returned `result: {}` with no coherent replace outcome. The typed
    // handle rejects the inconsistent fields as a `Serde` error. A response the
    // client cannot trust leaves the replace outcome ambiguous (the venue may
    // have applied it), so the adapter emits no terminal event and lets
    // reconciliation settle the order rather than rebinding to a stale VOI or
    // falsely rejecting a live order.
    *ws_state.replace_reply.lock().await = Some(json!({"result": {}}));
    let mut tc = build_client(rest_state, ws_state.clone()).await;
    tc.client.connect().await.expect("connect succeeds");

    let instrument_id = InstrumentId::from("ETH-PERP.DERIVE");
    let client_order_id = ClientOrderId::from("STRAT-MOD-AMBIG");
    let order = build_limit_order(
        instrument_id,
        client_order_id,
        OrderSide::Buy,
        Price::from("3500.00"),
        Quantity::from("1.000"),
    );
    tc.cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .expect("cache insert");
    let _ = drain_until(
        &mut tc.rx,
        |e| matches!(e, ExecutionEvent::Account(_)),
        "initial AccountState",
    )
    .await;

    let cmd = ModifyOrder::new(
        TraderId::from("TRADER-001"),
        Some(ClientId::from("DERIVE")),
        StrategyId::from("S-1"),
        instrument_id,
        client_order_id,
        Some(VenueOrderId::from("ord-stale-ambig")),
        Some(Quantity::from("2.000")),
        Some(Price::from("3501.00")),
        None,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );
    tc.client.modify_order(cmd).expect("modify_order Ok");

    // Replace must still post even though the response shape is unexpected.
    wait_until(
        || {
            let state = ws_state.clone();
            async move { !state.replace_orders.lock().await.is_empty() }
        },
        "replace posted",
    )
    .await;

    // No OrderUpdated (would rebind a stale VOI) and no ModifyRejected (would
    // falsely reject a possibly-live order): the ambiguous outcome is silent.
    let terminal = tokio::time::timeout(Duration::from_millis(300), async {
        loop {
            match tc.rx.recv().await {
                Some(ExecutionEvent::Order(
                    OrderEventAny::Updated(_) | OrderEventAny::ModifyRejected(_),
                )) => return true,
                Some(_) => {}
                None => return false,
            }
        }
    })
    .await;

    assert!(
        terminal.is_err(),
        "malformed replace result must not emit a terminal modify event",
    );

    tc.client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test]
async fn test_modify_order_partial_replace_failure_emits_cancelled_once() {
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    *ws_state.replace_reply.lock().await = Some(json!({
        "result": {
            "order": null,
            "cancelled_order": order_json_with(
                "ord-partial-old",
                "STRAT-MOD-PARTIAL",
                "buy",
                "ETH-PERP",
                1_700_000_002_000_i64,
                "cancelled",
            ),
            "create_order_error": {
                "code": 10001,
                "message": "insufficient margin",
            },
        },
    }));
    let mut tc = build_client(rest_state, ws_state.clone()).await;
    tc.client.connect().await.expect("connect succeeds");

    let instrument_id = InstrumentId::from("ETH-PERP.DERIVE");
    let client_order_id = ClientOrderId::from("STRAT-MOD-PARTIAL");
    let order = build_limit_order(
        instrument_id,
        client_order_id,
        OrderSide::Buy,
        Price::from("3500.00"),
        Quantity::from("1.000"),
    );
    tc.cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .expect("cache insert");
    let _ = drain_until(
        &mut tc.rx,
        |event| matches!(event, ExecutionEvent::Account(_)),
        "initial AccountState",
    )
    .await;

    tc.client
        .submit_order(submit_cmd(&order))
        .expect("submit Ok");
    let _ = drain_until(
        &mut tc.rx,
        |event| matches!(event, ExecutionEvent::Order(OrderEventAny::Submitted(_))),
        "OrderSubmitted",
    )
    .await;

    let orders_channel = format!("{TEST_SUBACCOUNT}.orders");
    let open_frame = json!([order_json_with(
        "ord-partial-old",
        client_order_id.as_str(),
        "buy",
        "ETH-PERP",
        1_700_000_001_000_i64,
        "open",
    )]);
    ws_state.push_notification(make_subscription_frame(&orders_channel, &open_frame));
    let _ = drain_until(
        &mut tc.rx,
        |event| matches!(event, ExecutionEvent::Order(OrderEventAny::Accepted(_))),
        "OrderAccepted",
    )
    .await;

    let cancel_frame = json!([order_json_with(
        "ord-partial-old",
        client_order_id.as_str(),
        "buy",
        "ETH-PERP",
        1_700_000_002_000_i64,
        "cancelled",
    )]);
    *ws_state.replace_notification_before_reply.lock().await =
        Some(make_subscription_frame(&orders_channel, &cancel_frame));

    let cmd = ModifyOrder::new(
        TraderId::from("TRADER-001"),
        Some(ClientId::from("DERIVE")),
        StrategyId::from("S-1"),
        instrument_id,
        client_order_id,
        Some(VenueOrderId::from("ord-partial-old")),
        Some(Quantity::from("2.000")),
        Some(Price::from("3505.00")),
        None,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );
    tc.client.modify_order(cmd).expect("modify_order Ok");

    let event = drain_until(
        &mut tc.rx,
        |event| matches!(event, ExecutionEvent::Order(OrderEventAny::Canceled(_))),
        "OrderCanceled after partial replace",
    )
    .await;

    let ExecutionEvent::Order(OrderEventAny::Canceled(cancelled)) = event else {
        unreachable!();
    };

    assert_eq!(cancelled.client_order_id, client_order_id);
    assert_eq!(
        cancelled.venue_order_id,
        Some(VenueOrderId::from("ord-partial-old")),
    );

    let duplicate_terminal = tokio::time::timeout(Duration::from_millis(200), async {
        loop {
            match tc.rx.recv().await {
                Some(ExecutionEvent::Order(
                    OrderEventAny::Canceled(_)
                    | OrderEventAny::Updated(_)
                    | OrderEventAny::ModifyRejected(_),
                )) => return true,
                Some(_) => {}
                None => return false,
            }
        }
    })
    .await;

    assert!(
        duplicate_terminal.is_err(),
        "partial replace must emit exactly one terminal order event",
    );

    tc.client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test]
async fn test_modify_order_jsonrpc_rejection_emits_modify_rejected() {
    let rest_state = RestState::default();
    rest_state.get_order_responses.lock().await.insert(
        "ord-stale-rej".to_string(),
        order_json_with(
            "ord-stale-rej",
            "STRAT-MOD-REJ",
            "sell",
            "ETH-PERP",
            1_700_000_001_000,
            "open",
        ),
    );
    let ws_state = WsState::default();
    // Venue surfaces a structured JSON-RPC error envelope.
    *ws_state.replace_reply.lock().await = Some(json!({
        "error": {"code": -32602, "message": "Invalid params"}
    }));
    let mut tc = build_client(rest_state, ws_state.clone()).await;
    tc.client.connect().await.expect("connect succeeds");

    let instrument_id = InstrumentId::from("ETH-PERP.DERIVE");
    let client_order_id = ClientOrderId::from("STRAT-MOD-REJ");
    let order = build_limit_order(
        instrument_id,
        client_order_id,
        OrderSide::Sell,
        Price::from("3500.00"),
        Quantity::from("1.000"),
    );
    tc.cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .expect("cache insert");
    let _ = drain_until(
        &mut tc.rx,
        |e| matches!(e, ExecutionEvent::Account(_)),
        "initial AccountState",
    )
    .await;

    let cmd = ModifyOrder::new(
        TraderId::from("TRADER-001"),
        Some(ClientId::from("DERIVE")),
        StrategyId::from("S-1"),
        instrument_id,
        client_order_id,
        Some(VenueOrderId::from("ord-stale-rej")),
        Some(Quantity::from("0.500")),
        None,
        None,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );
    tc.client.modify_order(cmd).expect("modify_order Ok");

    let event = drain_until(
        &mut tc.rx,
        |e| matches!(e, ExecutionEvent::Order(OrderEventAny::ModifyRejected(_))),
        "OrderModifyRejected event",
    )
    .await;

    if let ExecutionEvent::Order(OrderEventAny::ModifyRejected(rejected)) = event {
        let reason = rejected.reason.as_str();
        assert!(
            reason.contains("-32602") && reason.contains("Invalid params"),
            "unexpected reject reason: {reason}",
        );
        assert_eq!(
            rejected.venue_order_id.map(|v| v.as_str().to_string()),
            Some("ord-stale-rej".to_string()),
        );
    } else {
        unreachable!();
    }

    // One replace request, no OrderUpdated should land.
    let replaces = ws_state.replace_orders.lock().await;
    assert_eq!(replaces.len(), 1, "expected exactly one replace request");

    tc.client.disconnect().await.expect("disconnect");
}

#[rstest]
#[case::internal_error(-32603)]
#[case::order_confirmation_timeout(9000)]
#[case::engine_confirmation_timeout(9001)]
#[tokio::test]
async fn test_modify_order_jsonrpc_ambiguous_does_not_emit_modify_rejected(#[case] code: i64) {
    let rest_state = RestState::default();
    rest_state.get_order_responses.lock().await.insert(
        "ord-stale-retry".to_string(),
        order_json_with(
            "ord-stale-retry",
            "STRAT-MOD-RETRY",
            "sell",
            "ETH-PERP",
            1_700_000_001_000,
            "open",
        ),
    );
    let ws_state = WsState::default();
    *ws_state.replace_reply.lock().await = Some(json!({
        "error": {"code": code, "message": "Internal venue error"}
    }));
    let mut tc = build_client(rest_state, ws_state.clone()).await;
    tc.client.connect().await.expect("connect succeeds");

    let instrument_id = InstrumentId::from("ETH-PERP.DERIVE");
    let client_order_id = ClientOrderId::from("STRAT-MOD-RETRY");
    let order = build_limit_order(
        instrument_id,
        client_order_id,
        OrderSide::Sell,
        Price::from("3500.00"),
        Quantity::from("1.000"),
    );
    tc.cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .expect("cache insert");
    let _ = drain_until(
        &mut tc.rx,
        |e| matches!(e, ExecutionEvent::Account(_)),
        "initial AccountState",
    )
    .await;

    let cmd = ModifyOrder::new(
        TraderId::from("TRADER-001"),
        Some(ClientId::from("DERIVE")),
        StrategyId::from("S-1"),
        instrument_id,
        client_order_id,
        Some(VenueOrderId::from("ord-stale-retry")),
        Some(Quantity::from("0.500")),
        None,
        None,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );
    tc.client.modify_order(cmd).expect("modify_order Ok");
    wait_until(
        || {
            let state = ws_state.clone();
            async move { !state.replace_orders.lock().await.is_empty() }
        },
        "replace posted",
    )
    .await;

    let outcome = tokio::time::timeout(Duration::from_millis(200), async {
        loop {
            match tc.rx.recv().await {
                Some(ExecutionEvent::Order(OrderEventAny::ModifyRejected(_))) => {
                    return Some("unexpected OrderModifyRejected on retryable code");
                }
                Some(ExecutionEvent::Order(OrderEventAny::Updated(_))) => {
                    return Some("unexpected OrderUpdated on retryable code");
                }
                Some(_) => {}
                None => return None,
            }
        }
    })
    .await;

    assert!(
        outcome.is_err(),
        "retryable JSON-RPC code must not emit a terminal modify event, was {outcome:?}",
    );

    assert_eq!(ws_state.replace_orders.lock().await.len(), 1);

    tc.client.disconnect().await.expect("disconnect");
}

#[rstest]
#[case::no_venue_order_id(None, true, "venue_order_id is required")]
#[case::order_not_in_cache(Some(VenueOrderId::from("ord-x")), false, "order not found in cache")]
#[tokio::test]
async fn test_modify_order_rejects_invalid_command(
    #[case] venue_order_id: Option<VenueOrderId>,
    #[case] pre_insert_order: bool,
    #[case] reason_fragment: &str,
) {
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    let mut tc = build_client(rest_state, ws_state.clone()).await;
    tc.client.connect().await.expect("connect succeeds");

    let instrument_id = InstrumentId::from("ETH-PERP.DERIVE");
    let client_order_id = ClientOrderId::from("STRAT-MOD-INVALID");

    if pre_insert_order {
        let order = build_limit_order(
            instrument_id,
            client_order_id,
            OrderSide::Buy,
            Price::from("3500.00"),
            Quantity::from("1.000"),
        );
        tc.cache
            .borrow_mut()
            .add_order(order, None, None, false)
            .expect("cache insert");
    }

    let _ = drain_until(
        &mut tc.rx,
        |e| matches!(e, ExecutionEvent::Account(_)),
        "initial AccountState",
    )
    .await;

    let cmd = ModifyOrder::new(
        TraderId::from("TRADER-001"),
        Some(ClientId::from("DERIVE")),
        StrategyId::from("S-1"),
        instrument_id,
        client_order_id,
        venue_order_id,
        None,
        None,
        None,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );
    tc.client.modify_order(cmd).expect("modify_order Ok");

    let event = drain_until(
        &mut tc.rx,
        |e| matches!(e, ExecutionEvent::Order(OrderEventAny::ModifyRejected(_))),
        "OrderModifyRejected event",
    )
    .await;

    if let ExecutionEvent::Order(OrderEventAny::ModifyRejected(rejected)) = event {
        assert!(
            rejected.reason.contains(reason_fragment),
            "expected reason to contain `{reason_fragment}`, was `{}`",
            rejected.reason.as_str(),
        );
    } else {
        unreachable!();
    }

    assert!(
        ws_state.replace_orders.lock().await.is_empty(),
        "validation failure must not post to the venue",
    );

    tc.client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test]
async fn test_modify_order_rejects_trigger_order() {
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    let mut tc = build_client(rest_state, ws_state.clone()).await;
    tc.client.connect().await.expect("connect succeeds");

    let instrument_id = InstrumentId::from("ETH-PERP.DERIVE");
    let client_order_id = ClientOrderId::from("STRAT-MOD-TRIGGER");
    let order = build_limit_if_touched_order(
        instrument_id,
        client_order_id,
        OrderSide::Buy,
        Price::from("3700.00"),
        Price::from("3600.00"),
        Quantity::from("1.000"),
    );
    tc.cache
        .borrow_mut()
        .add_order(order, None, None, false)
        .expect("cache insert");

    let cmd = ModifyOrder::new(
        TraderId::from("TRADER-001"),
        Some(ClientId::from("DERIVE")),
        StrategyId::from("S-1"),
        instrument_id,
        client_order_id,
        Some(VenueOrderId::from("trig-mod-1")),
        Some(Quantity::from("2.000")),
        Some(Price::from("3710.00")),
        None,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );
    tc.client.modify_order(cmd).expect("modify_order Ok");

    let event = drain_until(
        &mut tc.rx,
        |e| matches!(e, ExecutionEvent::Order(OrderEventAny::ModifyRejected(_))),
        "OrderModifyRejected event",
    )
    .await;

    if let ExecutionEvent::Order(OrderEventAny::ModifyRejected(rejected)) = event {
        assert_eq!(
            rejected.reason,
            "Derive trigger orders cannot be modified; cancel and resubmit",
        );
        assert_eq!(
            rejected.venue_order_id.map(|v| v.as_str().to_string()),
            Some("trig-mod-1".to_string()),
        );
    } else {
        unreachable!();
    }

    assert!(
        ws_state.replace_orders.lock().await.is_empty(),
        "trigger modify must not post private/replace",
    );

    tc.client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test]
async fn test_batch_cancel_orders_fans_out_per_order() {
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    let mut tc = build_client(rest_state, ws_state.clone()).await;
    tc.client.connect().await.expect("connect succeeds");

    let inner = |voi: &str| {
        CancelOrder::new(
            TraderId::from("TRADER-001"),
            Some(ClientId::from("DERIVE")),
            StrategyId::from("S-1"),
            InstrumentId::from("ETH-PERP.DERIVE"),
            ClientOrderId::from(voi),
            Some(VenueOrderId::from(voi)),
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        )
    };

    let cmd = BatchCancelOrders::new(
        TraderId::from("TRADER-001"),
        Some(ClientId::from("DERIVE")),
        StrategyId::from("S-1"),
        InstrumentId::from("ETH-PERP.DERIVE"),
        vec![inner("ord-A"), inner("ord-B")],
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );
    tc.client.batch_cancel_orders(cmd).expect("batch_cancel Ok");

    wait_until(
        || {
            let state = ws_state.clone();
            async move { state.cancelled_orders.lock().await.len() >= 2 }
        },
        "two cancels posted",
    )
    .await;

    let posts = ws_state.cancelled_orders.lock().await;
    let ids: Vec<&str> = posts
        .iter()
        .map(|b| b["order_id"].as_str().unwrap_or(""))
        .collect();
    assert!(ids.contains(&"ord-A") && ids.contains(&"ord-B"));

    tc.client.disconnect().await.expect("disconnect");
}

#[rstest]
#[case::venue_id(Some(VenueOrderId::from("ord-mock-1")))]
#[case::client_label(None)]
#[tokio::test]
async fn test_query_order_emits_order_status_report(#[case] venue_order_id: Option<VenueOrderId>) {
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    let mut tc = build_report_client(rest_state.clone(), ws_state).await;
    tc.client.connect().await.expect("connect succeeds");

    let cmd = QueryOrder::new(
        TraderId::from("TRADER-001"),
        Some(ClientId::from("DERIVE")),
        StrategyId::from("S-1"),
        InstrumentId::from("ETH-PERP.DERIVE"),
        ClientOrderId::from("STRAT-O-1"),
        venue_order_id,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );
    tc.client.query_order(cmd).expect("query_order Ok");

    let event = drain_until(
        &mut tc.rx,
        |event| matches!(event, ExecutionEvent::Report(ExecutionReport::Order(_))),
        "OrderStatusReport event",
    )
    .await;

    if let ExecutionEvent::Report(ExecutionReport::Order(report)) = event {
        let mut expected = OrderStatusReport::new(
            AccountId::from("DERIVE-001"),
            InstrumentId::from("ETH-PERP.DERIVE"),
            None,
            VenueOrderId::from("ord-mock-1"),
            Some(OrderSide::Buy),
            OrderType::Limit,
            TimeInForce::Gtc,
            OrderStatus::Accepted,
            Quantity::from("1"),
            Quantity::from("0"),
            UnixNanos::from(1_700_000_000_000_000_000),
            UnixNanos::from(1_700_000_001_000_000_000),
            report.ts_init,
            Some(report.report_id),
        )
        .with_client_order_id(ClientOrderId::from("STRAT-O-1"))
        .with_price(Price::from("3500"));
        expected.avg_px = Some(dec!(3500));
        assert_eq!(*report, expected);
    } else {
        unreachable!();
    }

    tc.client.disconnect().await.expect("disconnect");
}

#[rstest]
#[case::missing_label("UNKNOWN", "ETH-PERP.DERIVE", false)]
#[case::instrument_mismatch("STRAT-O-1", "BTC-PERP.DERIVE", false)]
#[case::malformed_response("STRAT-O-1", "ETH-PERP.DERIVE", true)]
#[tokio::test]
async fn test_query_order_by_label_does_not_emit_invalid_report(
    #[case] label: &str,
    #[case] instrument: &str,
    #[case] malformed: bool,
) {
    let rest_state = RestState::default();
    if malformed {
        *rest_state.open_orders_response.lock().await = json!({});
    }

    let mut tc = build_report_client(rest_state.clone(), WsState::default()).await;
    tc.client.connect().await.expect("connect succeeds");
    tc.client
        .query_order(QueryOrder::new(
            TraderId::from("TRADER-001"),
            Some(ClientId::from("DERIVE")),
            StrategyId::from("S-1"),
            InstrumentId::from(instrument),
            ClientOrderId::from(label),
            None,
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ))
        .expect("query_order succeeds");

    wait_until_async(
        || async { !rest_state.open_orders_calls.lock().await.is_empty() },
        Duration::from_secs(5),
    )
    .await;

    let outcome = tokio::time::timeout(Duration::from_millis(200), async {
        loop {
            if let Some(ExecutionEvent::Report(ExecutionReport::Order(report))) = tc.rx.recv().await
            {
                return report;
            }
        }
    })
    .await;

    assert!(outcome.is_err(), "unexpected report: {outcome:?}");
    assert_eq!(rest_state.open_orders_calls.lock().await.len(), 1);
    assert!(rest_state.get_order_calls.lock().await.is_empty());
    tc.client.disconnect().await.expect("disconnect");
}

#[rstest]
#[case("failed_to_fetch")]
#[case("missing_currency")]
#[case("null_health")]
#[case("invalid_position")]
#[tokio::test]
async fn test_exec_client_incomplete_snapshot_emits_no_account_state(#[case] invalid: &str) {
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    let mut subaccount = sample_subaccount_json();
    match invalid {
        "failed_to_fetch" => subaccount["failed_to_fetch"] = json!(true),
        "missing_currency" => {
            subaccount.as_object_mut().unwrap().remove("currency");
        }
        "null_health" => subaccount["initial_margin"] = Value::Null,
        "invalid_position" => {
            subaccount["positions"] =
                json!([sample_position_json("ETH-PERP", "0.0000000000000000001")]);
        }
        _ => unreachable!(),
    }

    *rest_state.subaccount_response.lock().await = subaccount;
    let mut tc = build_client(rest_state.clone(), ws_state).await;

    let err = tc
        .client
        .connect()
        .await
        .expect_err("incomplete account must reject startup");

    assert!(
        err.to_string()
            .contains("failed initial Derive account state refresh")
    );
    assert!(!tc.client.is_connected());
    assert_eq!(rest_state.get_subaccount_calls.lock().await.len(), 1);

    while let Ok(event) = tc.rx.try_recv() {
        assert!(
            !matches!(event, ExecutionEvent::Account(_)),
            "incomplete snapshot emits no account state"
        );
    }
}

#[rstest]
#[tokio::test]
async fn test_exec_client_unknown_order_status_preserves_account_state() {
    let rest = RestState::default();
    let mut subaccount = sample_subaccount_json();
    subaccount["collaterals"][0]["amount"] = json!("1234.56");
    subaccount["positions_value"] = json!("150");
    subaccount["positions_initial_margin"] = json!("-25");
    subaccount["positions_maintenance_margin"] = json!("-10");
    subaccount["open_orders_margin"] = json!("-5");
    let known = serde_json::from_value::<DeriveSubaccount>(subaccount.clone()).unwrap();
    let (_, _, expected_info) = parse_derive_subaccount_to_balances(&known).unwrap();
    let mut order = sample_order_json();
    order["order_status"] = json!("unknown");
    subaccount["open_orders"] = json!([order]);
    *rest.subaccount_response.lock().await = subaccount;
    let mut tc = build_report_client(rest.clone(), WsState::default()).await;
    tc.client.connect().await.unwrap();
    let connected = tc.client.is_connected();
    let event = drain_until(
        &mut tc.rx,
        |event| matches!(event, ExecutionEvent::Account(_)),
        "authoritative account state",
    )
    .await;

    let ExecutionEvent::Account(state) = event else {
        unreachable!()
    };

    tc.client.disconnect().await.unwrap();

    assert!(connected);
    assert_eq!(state.account_id, AccountId::from("DERIVE-001"));
    assert_eq!(state.account_type, AccountType::Margin);
    assert_eq!(state.base_currency, None);
    assert!(state.is_reported);
    assert_eq!(
        state.balances,
        vec![
            AccountBalance::from_total_and_locked(dec!(1234.56), dec!(0), Currency::USDC())
                .unwrap()
        ]
    );
    assert_eq!(
        state.margins,
        vec![MarginBalance::new(
            Money::from_decimal(dec!(180), Currency::USD()).unwrap(),
            Money::from_decimal(dec!(160), Currency::USD()).unwrap(),
            None,
        )]
    );
    assert_eq!(state.info, Some(expected_info));
}

#[rstest]
#[tokio::test]
async fn test_exec_client_preserves_account_projection_with_unsupported_portfolio_instruments() {
    let rest_state = RestState::default();
    let mut subaccount: Value = serde_json::from_str(include_str!(
        "../../test_data/common/http_subaccount_unknown_variants.json"
    ))
    .unwrap();
    subaccount["open_orders"].as_array_mut().unwrap().remove(1);
    subaccount["subaccount_id"] = json!(TEST_SUBACCOUNT);
    for order in subaccount["open_orders"].as_array_mut().unwrap() {
        order["subaccount_id"] = json!(TEST_SUBACCOUNT);
    }

    let expected_positions = serde_json::to_value(
        serde_json::from_value::<Vec<DerivePosition>>(subaccount["positions"].clone()).unwrap(),
    )
    .unwrap();
    let mut definition = sample_instrument_json();
    definition["instrument_type"] = json!("structured");
    *rest_state.get_instrument_response.lock().await = definition;
    *rest_state.subaccount_response.lock().await = subaccount;
    let mut tc = build_report_client(rest_state, WsState::default()).await;
    let error = tc
        .client
        .connect()
        .await
        .expect_err("unsupported required instrument must block connection");
    assert!(format!("{error:#}").contains("unsupported required Derive portfolio instrument"));
    assert!(!tc.client.is_connected());
    let event = drain_until(
        &mut tc.rx,
        |event| matches!(event, ExecutionEvent::Account(_)),
        "open-variant account state",
    )
    .await;

    let ExecutionEvent::Account(state) = event else {
        unreachable!()
    };

    assert_eq!(
        state.balances,
        vec![AccountBalance::from_total_and_locked(dec!(1000), dec!(0), Currency::USDC()).unwrap()]
    );
    assert_eq!(state.margins.len(), 1);
    assert_eq!(
        state.margins[0].initial,
        Money::from_decimal(dec!(150), Currency::USD()).unwrap()
    );
    assert_eq!(
        state.margins[0].maintenance,
        Money::from_decimal(dec!(80), Currency::USD()).unwrap()
    );
    let info = state.info.unwrap();
    assert_eq!(info["margin_type"], "PM3");
    assert_eq!(info["manager_id"], 12);
    assert_eq!(info["currency"], json!(["ETH", "BTC"]));
    assert_eq!(info["collaterals"][0]["asset_type"], "unknown");
    assert_eq!(info["net_initial_margin"], "905");
    assert_eq!(info["net_maintenance_margin"], "925");

    let retained = tc
        .client
        .http_client()
        .get_subaccount(&DeriveGetSubaccountParams::new(TEST_SUBACCOUNT))
        .await
        .unwrap();
    assert_eq!(
        serde_json::to_value(retained.positions).unwrap(),
        expected_positions
    );

    tc.client.disconnect().await.expect("disconnect");
}

#[rstest]
#[case("balance", false)]
#[case("balance", true)]
#[case("reconnect", false)]
#[case("reconnect", true)]
#[tokio::test]
async fn test_incomplete_snapshot_refresh_emits_no_account_or_mass_status(
    #[case] trigger: &str,
    #[case] malformed: bool,
) {
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    let registry = SocketReconnectRegistry::default();
    let mut tc = build_client_with_config(
        rest_state.clone(),
        ws_state.clone(),
        Some(&registry),
        |config| config,
    )
    .await;
    tc.client.connect().await.expect("connect succeeds");
    drain_initial_account_state(&mut tc).await;
    let mut snapshot = sample_subaccount_json();
    snapshot["margin_type"] = json!("PM2");
    if malformed {
        snapshot["mm_credits"] = Value::Null;
    } else {
        snapshot["failed_to_fetch"] = json!(true);
    }

    *rest_state.subaccount_response.lock().await = snapshot;

    if trigger == "balance" {
        ws_state.push_notification(make_subscription_frame(
            &format!("{TEST_SUBACCOUNT}.balances"),
            &json!([{"name": "USDC", "new_balance": "1000", "previous_balance": "999", "update_type": "asset_deposit"}]),
        ));
    } else {
        let handle = registry
            .handle(ClientId::from("DERIVE"), Ustr::from("derive-user-streams"))
            .unwrap();
        assert_eq!(
            handle.request_reconnect(),
            SocketReconnectRequestOutcome::Accepted
        );
    }

    wait_until(
        || {
            let rest_state = rest_state.clone();
            async move { rest_state.get_subaccount_calls.lock().await.len() == 3 }
        },
        "rejected refreshed snapshot requested",
    )
    .await;

    let event = tokio::time::timeout(Duration::from_millis(100), tc.rx.recv()).await;

    assert!(
        event.is_err(),
        "rejected refresh must emit no execution event"
    );
    assert_eq!(rest_state.get_subaccount_calls.lock().await.len(), 3);
    assert!(rest_state.open_orders_calls.lock().await.is_empty());
    assert_eq!(rest_state.trigger_orders_calls.lock().await.len(), 2);
    assert!(rest_state.order_history_calls.lock().await.is_empty());
    assert!(rest_state.trade_history_calls.lock().await.is_empty());
    assert!(rest_state.positions_calls.lock().await.is_empty());
    assert!(tc.client.is_connected());

    tc.client.disconnect().await.expect("disconnect");
}

#[rstest]
#[case("failed_to_fetch")]
#[case("null_order")]
#[tokio::test]
async fn test_query_account_incomplete_snapshot_emits_no_account_state(#[case] invalid: &str) {
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    let mut tc = build_client(rest_state.clone(), ws_state).await;
    tc.client.connect().await.expect("connect succeeds");
    drain_initial_account_state(&mut tc).await;
    let mut subaccount = sample_subaccount_json();
    if invalid == "failed_to_fetch" {
        subaccount["failed_to_fetch"] = json!(true);
    } else {
        let mut order = sample_order_json();
        order["amount"] = Value::Null;
        subaccount["open_orders"] = json!([order]);
    }

    *rest_state.subaccount_response.lock().await = subaccount;
    let calls_before = rest_state.get_subaccount_calls.lock().await.len();

    tc.client
        .query_account(QueryAccount::new(
            TraderId::from("TRADER-001"),
            Some(ClientId::from("DERIVE")),
            AccountId::from("DERIVE-001"),
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ))
        .unwrap();
    wait_until(
        || {
            let rest = rest_state.clone();
            async move { rest.get_subaccount_calls.lock().await.len() == calls_before + 1 }
        },
        "account query completed",
    )
    .await;

    let event = tokio::time::timeout(Duration::from_millis(200), async {
        loop {
            match tc.rx.recv().await {
                Some(ExecutionEvent::Account(state)) => return Some(state),
                Some(_) => {}
                None => return None,
            }
        }
    })
    .await;

    assert!(
        event.is_err(),
        "incomplete query snapshot must emit no account state"
    );
    tc.client.disconnect().await.expect("disconnect");
}

#[rstest]
#[case("SM")]
#[case("PM2")]
#[case("PM2-next")]
#[tokio::test]
async fn test_query_account_emits_account_state_event(#[case] margin_type: &str) {
    let subaccount = {
        let mut value = sample_subaccount_json();
        value["margin_type"] = json!(margin_type);
        value["manager_id"] = json!(57);
        value["risk_universe_id"] = json!(19);
        value["positions_initial_margin"] = json!("3.125");
        value["positions_maintenance_margin"] = json!("3.905");
        value["positions_value"] = json!("6.25");
        value["open_orders_margin"] = json!("-0.01");
        value["mm_credits"] = json!("0.123456789123");
        value["projected_margin_change"] = json!("-0.987654321987");
        value
    };

    let rest_state = RestState::default();
    *rest_state.subaccount_response.lock().await = subaccount;
    let ws_state = WsState::default();
    let mut tc = build_client(rest_state.clone(), ws_state).await;
    tc.client.connect().await.expect("connect succeeds");
    // Drain the initial account-state event emitted at connect time so the
    // explicit query_account event below is the one we inspect.
    let _initial = drain_until(
        &mut tc.rx,
        |e| matches!(e, ExecutionEvent::Account(_)),
        "initial AccountState",
    )
    .await;

    let cmd = QueryAccount::new(
        TraderId::from("TRADER-001"),
        Some(ClientId::from("DERIVE")),
        AccountId::from("DERIVE-001"),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );
    tc.client.query_account(cmd).expect("query_account Ok");

    let event = drain_until(
        &mut tc.rx,
        |e| matches!(e, ExecutionEvent::Account(_)),
        "AccountState event",
    )
    .await;

    if let ExecutionEvent::Account(state) = event {
        assert_eq!(state.account_id, AccountId::from("DERIVE-001"));
        assert_eq!(state.account_type, AccountType::Margin);
        assert_eq!(state.base_currency, None);
        assert!(state.is_reported);
        assert_eq!(state.balances.len(), 1);
        assert_eq!(state.balances[0].total.as_decimal(), dec!(1000));
        assert_eq!(state.balances[0].locked.as_decimal(), dec!(0));
        assert_eq!(state.balances[0].free.as_decimal(), dec!(1000));
        assert_eq!(state.margins.len(), 1);
        assert_eq!(state.margins[0].initial.as_decimal(), dec!(3.14));
        assert_eq!(state.margins[0].maintenance.as_decimal(), dec!(2.34));
        assert_eq!(state.balances[0].total.currency, Currency::USDC());
        assert_eq!(state.margins[0].initial.currency, Currency::USD());
        assert_eq!(state.margins[0].maintenance.currency, Currency::USD());
        assert_eq!(state.margins[0].instrument_id, None);
        let info = state.info.expect("account state carries risk info");
        assert_eq!(info["currency"], json!(["ETH", "BTC"]));
        assert_eq!(info["margin_type"], margin_type);
        assert_eq!(info["manager_id"], 57);
        assert_eq!(info["risk_universe_id"], 19);
        assert_eq!(info["positions_initial_margin"], "3.125");
        assert_eq!(info["positions_maintenance_margin"], "3.905");
        assert_eq!(info["positions_value"], "6.25");
        assert_eq!(info["open_orders_margin"], "-0.01");
        assert_eq!(info["mm_credits"], "0.123456789123");
        assert_eq!(info["projected_margin_change"], "-0.987654321987");
        assert_eq!(info.get("net_initial_margin"), Some(&json!("100")));
        assert_eq!(info.get("net_maintenance_margin"), Some(&json!("50")));
    } else {
        unreachable!();
    }

    let calls = rest_state.get_subaccount_calls.lock().await;
    // At least one call (connect refresh) plus the explicit query.
    assert!(calls.len() >= 2);

    tc.client.disconnect().await.expect("disconnect");
}

#[rstest]
#[case("SM")]
#[case("PM2")]
#[tokio::test]
async fn test_balance_subscription_refreshes_authoritative_account_state(
    #[case] margin_type: &str,
) {
    let rest_state = RestState::default();
    let mut subaccount = sample_subaccount_json();
    subaccount["margin_type"] = json!(margin_type);
    *rest_state.subaccount_response.lock().await = subaccount;
    let ws_state = WsState::default();
    let mut tc = build_client(rest_state.clone(), ws_state.clone()).await;
    tc.client.connect().await.expect("connect succeeds");
    drain_initial_account_state(&mut tc).await;

    let mut updated_subaccount = sample_subaccount_json();
    updated_subaccount["margin_type"] = json!(margin_type);
    updated_subaccount["collaterals"][0]["amount"] = json!("1250");
    updated_subaccount["collaterals"][0]["mark_value"] = json!("1250");
    updated_subaccount["collaterals_value"] = json!("1250");
    updated_subaccount["subaccount_value"] = json!("1250");
    *rest_state.subaccount_response.lock().await = updated_subaccount;

    ws_state.push_notification(make_subscription_frame(
        &format!("{TEST_SUBACCOUNT}.balances"),
        &json!([{
            "name": "USDC",
            "new_balance": "1250",
            "previous_balance": "1000",
            "update_type": "asset_deposit",
        }]),
    ));

    let event = drain_until(
        &mut tc.rx,
        |event| matches!(event, ExecutionEvent::Account(_)),
        "balance refresh AccountState",
    )
    .await;

    if let ExecutionEvent::Account(state) = event {
        assert_eq!(state.info.as_ref().unwrap()["margin_type"], margin_type);
        assert_eq!(state.margins[0].initial.currency, Currency::USD());
        assert_eq!(state.balances.len(), 1);
        assert_eq!(state.balances[0].total.as_decimal(), dec!(1250));
        assert_eq!(state.balances[0].locked.as_decimal(), dec!(0));
        assert_eq!(state.balances[0].free.as_decimal(), dec!(1250));
    } else {
        unreachable!();
    }

    assert!(rest_state.get_subaccount_calls.lock().await.len() >= 2);

    tc.client.disconnect().await.expect("disconnect");
}

async fn build_report_client(rest_state: RestState, ws_state: WsState) -> TestClient {
    let tc = build_client(rest_state, ws_state).await;
    let mut definitions = Vec::new();

    for symbol in ["ETH-PERP", "BTC-PERP", "SOL-PERP"] {
        let mut definition = sample_instrument_json();
        definition["instrument_name"] = json!(symbol);
        definitions.push(definition);
    }

    for (symbol, kind) in [("ETH-20260626-3500-C", "C"), ("ETH-20260626-3500-P", "P")] {
        definitions.push(option_instrument_json(symbol, kind, "3500"));
    }

    for definition in definitions {
        let raw: DeriveInstrument = serde_json::from_value(definition).unwrap();
        let native = parse_derive_instrument_any(&raw, UnixNanos::default())
            .unwrap()
            .unwrap();
        tc.client.cache_instrument(raw).unwrap();
        tc.cache.borrow_mut().add_instrument(native).unwrap();
    }

    tc
}

#[rstest]
#[case(true)]
#[case(false)]
#[tokio::test]
async fn test_report_request_rejects_missing_active_metadata(#[case] order_report: bool) {
    let rest_state = RestState::default();
    *rest_state.open_orders_response.lock().await = json!({
        "orders": [order_json_with("ord-missing-metadata", "L-MISSING", "buy", "UNLOADED-PERP", 1_700_000_001_000, "open")],
        "subaccount_id": TEST_SUBACCOUNT,
    });
    *rest_state.positions_response.lock().await = json!({
        "positions": [sample_position_json("UNLOADED-PERP", "0.3")], "subaccount_id": TEST_SUBACCOUNT,
    });
    let mut tc = build_client(rest_state.clone(), WsState::default()).await;
    tc.client.connect().await.unwrap();
    let error = if order_report {
        tc.client
            .generate_order_status_reports(&GenerateOrderStatusReports::new(
                UUID4::new(),
                UnixNanos::default(),
                true,
                None,
                None,
                None,
                None,
                None,
            ))
            .await
            .unwrap_err()
    } else {
        tc.client
            .generate_position_status_reports(&GeneratePositionStatusReports::new(
                UUID4::new(),
                UnixNanos::default(),
                None,
                None,
                None,
                None,
                None,
            ))
            .await
            .unwrap_err()
    };

    assert_eq!(
        error.to_string(),
        "missing Derive instrument metadata for UNLOADED-PERP.DERIVE"
    );
    assert_eq!(rest_state.get_instrument_calls.lock().await.len(), 0);
    tc.client.disconnect().await.unwrap();
}

#[rstest]
#[case::unbounded(None)]
#[case::bounded(Some(10_000_000))]
#[tokio::test]
async fn test_mass_status_missing_historical_metadata_preserves_rows_incomplete(
    #[case] lookback: Option<u64>,
) {
    let rest_state = RestState::default();
    *rest_state.open_orders_response.lock().await =
        json!({"orders": [], "subaccount_id": TEST_SUBACCOUNT});
    *rest_state.positions_response.lock().await =
        json!({"positions": [], "subaccount_id": TEST_SUBACCOUNT});
    *rest_state.order_history_response.lock().await = json!({
        "orders": [order_json_with("ord-unloaded-history", "L-HIST-MISSING", "buy", "EXPIRED-PERP", 1_700_000_002_000, "filled")],
        "pagination": {"count": 1, "num_pages": 1}, "subaccount_id": TEST_SUBACCOUNT,
    });
    *rest_state.trade_history_response.lock().await = json!({
        "trades": [sample_trade_json("trade-unloaded-history", "ord-unloaded-history", "EXPIRED-PERP")],
        "pagination": {"count": 1, "num_pages": 1}, "subaccount_id": TEST_SUBACCOUNT,
    });
    let mut tc = build_client(rest_state.clone(), WsState::default()).await;
    tc.client.connect().await.unwrap();
    let mass = tc
        .client
        .generate_mass_status(lookback)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(mass.reports_complete(), lookback.is_none());
    assert_eq!(mass.order_reports().len(), 1);
    assert_eq!(
        mass.order_reports()[&VenueOrderId::from("ord-unloaded-history")].instrument_id,
        InstrumentId::from("EXPIRED-PERP.DERIVE")
    );
    assert_eq!(mass.fill_reports().len(), 1);
    assert_eq!(
        mass.fill_reports()[&VenueOrderId::from("ord-unloaded-history")][0].trade_id,
        TradeId::from("trade-unloaded-history")
    );
    assert_eq!(rest_state.get_instrument_calls.lock().await.len(), 0);
    assert_eq!(mass.position_reports().len(), 0);
    tc.client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_generate_order_status_reports_open_only_includes_trigger_orders() {
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    // Distinct payloads so the routing branch is observable.
    *rest_state.open_orders_response.lock().await = json!({
        "orders": [order_json_with(
            "from-open", "L-OPEN", "buy", "ETH-PERP", 1_700_000_001_000, "open",
        )],
        "subaccount_id": TEST_SUBACCOUNT,
    });

    *rest_state.order_history_response.lock().await = json!({
        "orders": [order_json_with(
            "from-history", "L-HIST", "buy", "ETH-PERP", 1_700_000_001_000, "filled",
        )],
        "pagination": {"count": 1, "num_pages": 1},
        "subaccount_id": TEST_SUBACCOUNT,
    });
    let mut tc = build_report_client(rest_state.clone(), ws_state).await;
    tc.client.connect().await.expect("connect succeeds");
    *rest_state.trigger_orders_response.lock().await = json!({
        "orders": [trigger_order_json_with(
            "from-trigger",
            "L-TRIGGER",
            "sell",
            "ETH-PERP",
            1_700_000_001_500,
            "market",
            "untriggered",
            "3582",
            "3600",
            "mark",
            "stoploss",
        )],
        "subaccount_id": TEST_SUBACCOUNT,
    });

    let cmd = GenerateOrderStatusReports::new(
        UUID4::new(),
        UnixNanos::default(),
        true,
        Some(InstrumentId::from("ETH-PERP.DERIVE")),
        None,
        None,
        None,
        None,
    );
    let reports = tc
        .client
        .generate_order_status_reports(&cmd)
        .await
        .expect("reports");
    assert_eq!(reports.len(), 2);
    assert_eq!(reports[0].venue_order_id.as_str(), "from-open");
    assert_eq!(reports[1].venue_order_id.as_str(), "from-trigger");
    assert_eq!(reports[1].order_type, OrderType::StopMarket);
    assert_eq!(reports[1].order_status, OrderStatus::Accepted);
    assert_eq!(reports[1].trigger_price, Some(Price::from("3600")));
    assert!(!rest_state.open_orders_calls.lock().await.is_empty());
    assert!(!rest_state.trigger_orders_calls.lock().await.is_empty());
    assert!(rest_state.order_history_calls.lock().await.is_empty());

    tc.client.disconnect().await.expect("disconnect");
}

#[rstest]
#[case::unbounded(false)]
#[case::bounded(true)]
#[tokio::test]
async fn test_generate_order_status_reports_history_path_when_not_open_only(#[case] bounded: bool) {
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    *rest_state.open_orders_response.lock().await = json!({
        "orders": [order_json_with(
            "from-open", "L-OPEN", "buy", "ETH-PERP", 1_700_000_001_000, "open",
        )],
        "subaccount_id": TEST_SUBACCOUNT,
    });
    *rest_state.order_history_response.lock().await = json!({
        "orders": [order_json_with(
            "from-history", "L-HIST", "buy", "ETH-PERP", 1_700_000_001_000, "filled",
        )],
        "pagination": {"count": 1, "num_pages": 1},
        "subaccount_id": TEST_SUBACCOUNT,
    });
    let mut tc = build_report_client(rest_state.clone(), ws_state).await;
    tc.client.connect().await.expect("connect succeeds");

    let cmd = GenerateOrderStatusReports::new(
        UUID4::new(),
        UnixNanos::default(),
        false,
        Some(InstrumentId::from("ETH-PERP.DERIVE")),
        bounded.then_some(UnixNanos::from(1_700_000_002_000_000_000_u64)),
        bounded.then_some(UnixNanos::from(1_700_000_004_000_000_000_u64)),
        None,
        None,
    );
    let reports = tc
        .client
        .generate_order_status_reports(&cmd)
        .await
        .expect("reports");

    let expected_ids = if bounded {
        vec!["from-open"]
    } else {
        vec!["from-history", "from-open"]
    };

    assert_eq!(
        reports
            .iter()
            .map(|report| report.venue_order_id.as_str())
            .collect::<Vec<_>>(),
        expected_ids
    );
    let calls = rest_state.order_history_calls.lock().await;
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0]["page_size"].as_u64(), Some(500));

    tc.client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test]
async fn test_generate_order_status_reports_paginates_across_multiple_pages() {
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    *rest_state.order_history_pages.lock().await = vec![
        json!({
            "orders": [order_json_with(
                "order-page-1", "L-PAGE-1", "buy", "ETH-PERP", 1_700_000_000_500, "filled",
            )],
            "pagination": {"count": 2, "num_pages": 2},
            "subaccount_id": TEST_SUBACCOUNT,
        }),
        json!({
            "orders": [order_json_with(
                "order-page-2", "L-PAGE-2", "sell", "ETH-PERP", 1_700_000_001_500, "filled",
            )],
            "pagination": {"count": 2, "num_pages": 2},
            "subaccount_id": TEST_SUBACCOUNT,
        }),
    ];
    let mut tc = build_report_client(rest_state.clone(), ws_state).await;
    tc.client.connect().await.expect("connect succeeds");

    let cmd = GenerateOrderStatusReports::new(
        UUID4::new(),
        UnixNanos::default(),
        false,
        Some(InstrumentId::from("ETH-PERP.DERIVE")),
        Some(UnixNanos::from(1_700_000_000_000_123_456_u64)),
        Some(UnixNanos::from(1_700_000_002_000_999_999_u64)),
        None,
        None,
    );
    let reports = tc
        .client
        .generate_order_status_reports(&cmd)
        .await
        .expect("reports");

    let mut venue_order_ids: Vec<&str> = reports
        .iter()
        .map(|report| report.venue_order_id.as_str())
        .collect();
    venue_order_ids.sort_unstable();
    assert_eq!(
        venue_order_ids,
        vec!["ord-mock-1", "order-page-1", "order-page-2"]
    );

    let calls = rest_state.order_history_calls.lock().await;
    assert_eq!(calls.len(), 2, "must request both pages");
    assert_eq!(calls[0]["page"].as_u64(), Some(1));
    assert_eq!(calls[0]["page_size"].as_u64(), Some(500));
    assert_eq!(calls[0]["from_timestamp"].as_i64(), Some(1_700_000_000_000));
    assert_eq!(calls[0]["to_timestamp"].as_i64(), Some(1_700_000_002_000));
    assert_eq!(calls[1]["page"].as_u64(), Some(2));
    assert_eq!(calls[1]["page_size"].as_u64(), Some(500));
    assert_eq!(calls[1]["from_timestamp"].as_i64(), Some(1_700_000_000_000));
    assert_eq!(calls[1]["to_timestamp"].as_i64(), Some(1_700_000_002_000));

    tc.client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test]
async fn test_generate_order_status_reports_open_only_ignores_time_window() {
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    *rest_state.open_orders_response.lock().await = json!({
        "orders": [
            order_json_with("early", "E", "buy", "ETH-PERP", 100, "open"),
            order_json_with("middle", "M", "buy", "ETH-PERP", 200, "open"),
            order_json_with("late", "L", "buy", "ETH-PERP", 300, "open"),
        ],
        "subaccount_id": TEST_SUBACCOUNT,
    });
    let mut tc = build_report_client(rest_state, ws_state).await;
    tc.client.connect().await.expect("connect succeeds");

    let cmd = GenerateOrderStatusReports::new(
        UUID4::new(),
        UnixNanos::default(),
        true,
        Some(InstrumentId::from("ETH-PERP.DERIVE")),
        Some(UnixNanos::from(150_000_000_u64)), // 150 ms
        Some(UnixNanos::from(250_000_000_u64)), // 250 ms
        None,
        None,
    );
    let reports = tc
        .client
        .generate_order_status_reports(&cmd)
        .await
        .expect("reports");
    assert_eq!(
        reports
            .iter()
            .map(|report| report.venue_order_id.as_str())
            .collect::<Vec<_>>(),
        vec!["early", "middle", "late"],
    );

    tc.client.disconnect().await.expect("disconnect");
}

#[rstest]
#[case::status_report(false)]
#[case::query_order(true)]
#[tokio::test]
async fn test_generate_order_status_report_falls_back_to_history_by_label(
    #[case] query_order: bool,
) {
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    *rest_state.open_orders_response.lock().await = json!({
        "orders": [],
        "subaccount_id": TEST_SUBACCOUNT,
    });

    let mut filled_order = order_json_with(
        "ord-hist-1",
        "STRAT-LABEL",
        "sell",
        "ETH-PERP",
        1_700_000_001_000,
        "filled",
    );
    filled_order["amount"] = json!("1.25");
    filled_order["filled_amount"] = json!("1.25");
    *rest_state.order_history_pages.lock().await = vec![
        json!({
            "orders": [order_json_with(
                "ord-unrelated", "OTHER-LABEL", "buy", "ETH-PERP", 1_700_000_000_500, "filled",
            )],
            "pagination": {"count": 2, "num_pages": 2},
            "subaccount_id": TEST_SUBACCOUNT,
        }),
        json!({
            "orders": [filled_order],
            "pagination": {"count": 2, "num_pages": 2},
            "subaccount_id": TEST_SUBACCOUNT,
        }),
    ];
    let mut tc = build_report_client(rest_state.clone(), ws_state).await;
    tc.client.connect().await.expect("connect succeeds");

    let cmd = GenerateOrderStatusReport::new(
        UUID4::new(),
        UnixNanos::default(),
        Some(InstrumentId::from("ETH-PERP.DERIVE")),
        Some(ClientOrderId::from("STRAT-LABEL")),
        None,
        None,
        None,
    );

    let report = if query_order {
        query_order_report(&mut tc, &cmd).await
    } else {
        tc.client
            .generate_order_status_report(&cmd)
            .await
            .expect("report")
            .expect("some")
    };

    let mut expected = OrderStatusReport::new(
        AccountId::from("DERIVE-001"),
        InstrumentId::from("ETH-PERP.DERIVE"),
        None,
        VenueOrderId::from("ord-hist-1"),
        Some(OrderSide::Sell),
        OrderType::Limit,
        TimeInForce::Gtc,
        OrderStatus::Filled,
        Quantity::from("1.25"),
        Quantity::from("1.25"),
        UnixNanos::from(1_700_000_000_000_000_000),
        UnixNanos::from(1_700_000_001_000_000_000),
        report.ts_init,
        Some(report.report_id),
    )
    .with_client_order_id(ClientOrderId::from("STRAT-LABEL"))
    .with_price(Price::from("3500"));
    expected.avg_px = Some(dec!(3500));
    assert_eq!(report, expected);
    assert!(rest_state.get_order_calls.lock().await.is_empty());
    assert_eq!(rest_state.open_orders_calls.lock().await.len(), 1);
    assert_eq!(rest_state.trigger_orders_calls.lock().await.len(), 3);
    assert_eq!(
        *rest_state.order_history_calls.lock().await,
        vec![
            json!({"subaccount_id": TEST_SUBACCOUNT, "page": 1, "page_size": 500, "instrument_name": "ETH-PERP"}),
            json!({"subaccount_id": TEST_SUBACCOUNT, "page": 2, "page_size": 500, "instrument_name": "ETH-PERP"}),
        ],
    );

    tc.client.disconnect().await.expect("disconnect");
}

#[rstest]
#[case::status_report(false)]
#[case::query_order(true)]
#[tokio::test]
async fn test_generate_order_status_report_finds_trigger_order_by_label_before_history(
    #[case] query_order: bool,
) {
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    *rest_state.open_orders_response.lock().await = json!({
        "orders": [],
        "subaccount_id": TEST_SUBACCOUNT,
    });

    *rest_state.order_history_response.lock().await = json!({
        "orders": [order_json_with(
            "ord-hist-1", "STRAT-TRIG-LABEL", "buy", "ETH-PERP", 1, "filled",
        )],
        "pagination": {"count": 1, "num_pages": 1},
        "subaccount_id": TEST_SUBACCOUNT,
    });
    let mut tc = build_report_client(rest_state.clone(), ws_state).await;
    tc.client.connect().await.expect("connect succeeds");
    *rest_state.trigger_orders_response.lock().await = json!({
        "orders": [trigger_order_json_with(
            "trig-label-1",
            "STRAT-TRIG-LABEL",
            "sell",
            "ETH-PERP",
            1_700_000_001_000,
            "limit",
            "untriggered",
            "3700",
            "3800",
            "mark",
            "takeprofit",
        )],
        "subaccount_id": TEST_SUBACCOUNT,
    });

    let cmd = GenerateOrderStatusReport::new(
        UUID4::new(),
        UnixNanos::default(),
        Some(InstrumentId::from("ETH-PERP.DERIVE")),
        Some(ClientOrderId::from("STRAT-TRIG-LABEL")),
        None,
        None,
        None,
    );

    let report = if query_order {
        query_order_report(&mut tc, &cmd).await
    } else {
        tc.client
            .generate_order_status_report(&cmd)
            .await
            .expect("report")
            .expect("some")
    };

    let expected = OrderStatusReport::new(
        AccountId::from("DERIVE-001"),
        InstrumentId::from("ETH-PERP.DERIVE"),
        None,
        VenueOrderId::from("trig-label-1"),
        Some(OrderSide::Sell),
        OrderType::LimitIfTouched,
        TimeInForce::Gtc,
        OrderStatus::Accepted,
        Quantity::from("1"),
        Quantity::from("0"),
        UnixNanos::from(1_700_000_000_000_000_000),
        UnixNanos::from(1_700_000_001_000_000_000),
        report.ts_init,
        Some(report.report_id),
    )
    .with_client_order_id(ClientOrderId::from("STRAT-TRIG-LABEL"))
    .with_price(Price::from("3700"))
    .with_trigger_price(Price::from("3800"))
    .with_trigger_type(TriggerType::MarkPrice);
    assert_eq!(report, expected);
    assert!(!rest_state.open_orders_calls.lock().await.is_empty());
    assert!(!rest_state.trigger_orders_calls.lock().await.is_empty());
    assert!(
        rest_state.order_history_calls.lock().await.is_empty(),
        "trigger match must skip order history",
    );

    tc.client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test]
async fn test_generate_order_status_report_returns_none_on_instrument_mismatch() {
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    // Default get_order response has instrument_name = "ETH-PERP"; ask for BTC.
    let mut tc = build_report_client(rest_state, ws_state).await;
    tc.client.connect().await.expect("connect succeeds");

    let cmd = GenerateOrderStatusReport::new(
        UUID4::new(),
        UnixNanos::default(),
        Some(InstrumentId::from("BTC-PERP.DERIVE")),
        None,
        Some(VenueOrderId::from("ord-mock-1")),
        None,
        None,
    );
    let report = tc
        .client
        .generate_order_status_report(&cmd)
        .await
        .expect("report");
    assert!(report.is_none());

    tc.client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test]
async fn test_generate_fill_reports_filters_by_venue_order_id() {
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    *rest_state.trade_history_response.lock().await = json!({
        "trades": [
            sample_trade_json("trade-a", "ord-1", "ETH-PERP"),
            sample_trade_json("trade-b", "ord-2", "ETH-PERP"),
        ],
        "pagination": {"count": 2, "num_pages": 1},
        "subaccount_id": TEST_SUBACCOUNT,
    });
    let mut tc = build_report_client(rest_state, ws_state).await;
    tc.client.connect().await.expect("connect succeeds");

    let cmd = GenerateFillReports::new(
        UUID4::new(),
        UnixNanos::default(),
        Some(InstrumentId::from("ETH-PERP.DERIVE")),
        Some(VenueOrderId::from("ord-2")),
        None,
        None,
        None,
        None,
    );
    let reports = tc.client.generate_fill_reports(cmd).await.expect("fills");
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].trade_id.as_str(), "trade-b");
    assert_eq!(reports[0].venue_order_id.as_str(), "ord-2");

    tc.client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test]
async fn test_v3_fill_history_deduplicates_overlapping_pages_without_claiming_emission() {
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    let mut first = sample_trade_json("trade-page-1", "order-page-1", "ETH-PERP");
    first["batch_status"] = Value::Null;
    first["tx_hash"] = Value::Null;
    first["op_uuid"] = Value::Null;
    let mut second = sample_trade_json("trade-page-2", "order-page-2", "ETH-PERP");
    second["batch_status"] = json!("ExecutingError");
    let pages = vec![
        json!({"trades": [first.clone(), first.clone()], "pagination": {"count": 2, "num_pages": 2}, "subaccount_id": TEST_SUBACCOUNT}),
        json!({"trades": [first, second], "pagination": {"count": 2, "num_pages": 2}, "subaccount_id": TEST_SUBACCOUNT}),
    ];
    *rest_state.trade_history_pages.lock().await = pages.clone();
    let mut tc = build_report_client(rest_state.clone(), ws_state.clone()).await;
    tc.client.connect().await.expect("connect succeeds");

    let generate = || {
        GenerateFillReports::new(
            UUID4::new(),
            UnixNanos::default(),
            Some(InstrumentId::from("ETH-PERP.DERIVE")),
            None,
            Some(UnixNanos::from(1_700_000_000_000_000_000)),
            Some(UnixNanos::from(1_700_000_003_000_000_000)),
            None,
            None,
        )
    };

    let reports = tc.client.generate_fill_reports(generate()).await.unwrap();
    *rest_state.trade_history_pages.lock().await = pages;
    let retry = tc.client.generate_fill_reports(generate()).await.unwrap();
    assert_eq!(reports.len(), 2);
    assert_eq!(retry.len(), 2);

    for (i, report) in reports.iter().enumerate() {
        assert_eq!(
            report.trade_id.as_str(),
            ["trade-page-1", "trade-page-2"][i]
        );
        assert_eq!(
            report.venue_order_id.as_str(),
            ["order-page-1", "order-page-2"][i]
        );
        assert_eq!(report.instrument_id, InstrumentId::from("ETH-PERP.DERIVE"));
        assert_eq!(report.last_qty.as_decimal(), rust_decimal_macros::dec!(1));
        assert_eq!(report.last_px.as_decimal(), rust_decimal_macros::dec!(3505));
        assert_eq!(
            report.commission.as_decimal(),
            rust_decimal_macros::dec!(0.5)
        );
        assert_eq!(report.commission.currency, Currency::USDC());
        assert_eq!(report.order_side, OrderSide::Buy);
        assert_eq!(report.ts_event, UnixNanos::from(1_700_000_002_000_000_000));
    }

    let calls = rest_state.trade_history_calls.lock().await;
    assert_eq!(calls.len(), 4);

    for (i, call) in calls.iter().enumerate() {
        assert_eq!(call["page"], json!(i % 2 + 1));
        assert_eq!(call["page_size"], json!(500));
        assert_eq!(call["from_timestamp"], json!(1_700_000_000_000_i64));
        assert_eq!(call["to_timestamp"], json!(1_700_000_003_000_i64));
        assert_eq!(call["instrument_name"], json!("ETH-PERP"));
    }

    drop(calls);
    tc.client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_generate_position_status_reports_filters_by_instrument() {
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    *rest_state.positions_response.lock().await = json!({
        "positions": [
            sample_position_json("ETH-PERP", "3"),
            sample_position_json("BTC-PERP", "-1"),
        ],
        "subaccount_id": TEST_SUBACCOUNT,
    });
    let mut tc = build_report_client(rest_state, ws_state).await;
    tc.client.connect().await.expect("connect succeeds");

    let cmd = GeneratePositionStatusReports::new(
        UUID4::new(),
        UnixNanos::default(),
        Some(InstrumentId::from("ETH-PERP.DERIVE")),
        None,
        None,
        None,
        None,
    );
    let reports = tc
        .client
        .generate_position_status_reports(&cmd)
        .await
        .expect("positions");
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].instrument_id.symbol.as_str(), "ETH-PERP");
    assert_eq!(reports[0].signed_decimal_qty, dec!(3));

    tc.client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test]
async fn test_mass_status_timestamp_precedes_delayed_report_completion() {
    let rest_state = RestState::default();
    let mut tc = build_report_client(rest_state.clone(), WsState::default()).await;
    tc.client.connect().await.unwrap();

    let response = rest_state.order_history_response.lock().await;
    let clock = get_atomic_clock_realtime();
    let started = UnixNanos::from(1_700_000_000_000_000_071);
    let completed = UnixNanos::from(1_700_000_005_000_000_093);
    clock.make_static();
    clock.set_time(started);
    let (mass, ()) = tokio::join!(tc.client.generate_mass_status(None), async {
        wait_until(
            || async { !rest_state.order_history_calls.lock().await.is_empty() },
            "history request blocked on response",
        )
        .await;
        clock.set_time(completed);
        drop(response);
    });
    let mass = mass.unwrap().unwrap();
    let finished = clock.get_time_ns();
    clock.make_realtime();
    tc.client.disconnect().await.unwrap();

    assert_eq!(mass.ts_init, started);
    assert_eq!(finished, completed);
    assert_eq!(rest_state.order_history_calls.lock().await.len(), 1);
}

#[rstest]
#[case(true)]
#[case(false)]
#[tokio::test]
async fn test_mass_status_preserves_sources_with_incomplete_history(#[case] request_fails: bool) {
    let rest_state = RestState::default();
    let open = order_json_with(
        "ord-live-coverage",
        "L-COVERAGE",
        "buy",
        "ETH-PERP",
        1_700_000_001_000,
        "open",
    );
    *rest_state.open_orders_response.lock().await = json!({
        "orders": [open], "subaccount_id": TEST_SUBACCOUNT,
    });

    *rest_state.order_history_response.lock().await = if request_fails {
        json!({"id": 1, "error": {"code": -32602, "message": "history unavailable"}})
    } else {
        json!({"orders": [order_json_with(" ", "BAD-HISTORY", "buy", "ETH-PERP", 1_700_000_002_000, "filled")],
            "pagination": {"count": 1, "num_pages": 1}, "subaccount_id": TEST_SUBACCOUNT})
    };

    *rest_state.trade_history_response.lock().await = json!({
        "trades": [sample_trade_json("trade-coverage", "ord-live-coverage", "ETH-PERP")],
        "pagination": {"count": 1, "num_pages": 1}, "subaccount_id": TEST_SUBACCOUNT,
    });
    *rest_state.positions_response.lock().await = json!({
        "positions": [sample_position_json("ETH-PERP", "0.3")], "subaccount_id": TEST_SUBACCOUNT,
    });
    let mut tc = build_report_client(rest_state.clone(), WsState::default()).await;
    tc.client
        .cache_instrument(serde_json::from_value(sample_instrument_json()).unwrap())
        .unwrap();
    tc.client.connect().await.unwrap();
    let mass = tc
        .client
        .generate_mass_status(Some(10_000_000))
        .await
        .unwrap()
        .unwrap();
    assert!(!mass.reports_complete());
    assert_eq!(
        mass.lookback_start(),
        Some(
            mass.ts_init
                .saturating_sub(DurationNanos::try_from_mins(10_000_000).unwrap())
        )
    );
    assert_eq!(mass.order_reports().len(), 1);
    assert_eq!(
        mass.order_reports()[&VenueOrderId::from("ord-live-coverage")].instrument_id,
        InstrumentId::from("ETH-PERP.DERIVE")
    );
    assert_eq!(mass.fill_reports().len(), 1);
    assert_eq!(
        mass.fill_reports()[&VenueOrderId::from("ord-live-coverage")][0].trade_id,
        TradeId::from("trade-coverage")
    );
    assert_eq!(mass.position_reports().len(), 1);
    assert_eq!(
        mass.position_reports()[&InstrumentId::from("ETH-PERP.DERIVE")][0].signed_decimal_qty,
        dec!(0.3)
    );
    assert_eq!(rest_state.order_history_calls.lock().await.len(), 1);
    tc.client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_mass_status_records_salvaged_trade_coverage() {
    let rest_state = RestState::default();
    let valid = sample_trade_json("trade-good-coverage", "ord-live-coverage", "ETH-PERP");
    let mut invalid = valid.clone();
    invalid["trade_id"] = json!("trade-bad-coverage");
    invalid["direction"] = json!("short");
    *rest_state.open_orders_response.lock().await = json!({
        "orders": [order_json_with("ord-live-coverage", "L-COVERAGE", "buy", "ETH-PERP", 1_700_000_001_000, "open")],
        "subaccount_id": TEST_SUBACCOUNT,
    });
    *rest_state.trade_history_response.lock().await = json!({
        "trades": [valid, invalid], "pagination": {"count": 2, "num_pages": 1},
        "subaccount_id": TEST_SUBACCOUNT,
    });
    let mut tc = build_report_client(rest_state, WsState::default()).await;
    tc.client
        .cache_instrument(serde_json::from_value(sample_instrument_json()).unwrap())
        .unwrap();
    tc.client.connect().await.unwrap();
    let mass = tc
        .client
        .generate_mass_status(Some(10_000_000))
        .await
        .unwrap()
        .unwrap();
    assert!(!mass.reports_complete());
    assert_eq!(mass.order_reports().len(), 1);
    assert_eq!(
        mass.fill_reports()[&VenueOrderId::from("ord-live-coverage")].len(),
        1
    );
    assert_eq!(
        mass.fill_reports()[&VenueOrderId::from("ord-live-coverage")][0].trade_id,
        TradeId::from("trade-good-coverage")
    );
    tc.client.disconnect().await.unwrap();
}

#[rstest]
#[case("ord-link", "ETH-PERP", true)]
#[case("ord-orphan", "ETH-PERP", false)]
#[case("ord-link", "BTC-PERP", false)]
#[tokio::test]
async fn test_mass_status_fill_order_linkage_controls_completeness(
    #[case] fill_order_id: &str,
    #[case] fill_instrument: &str,
    #[case] complete: bool,
) {
    let rest_state = RestState::default();
    *rest_state.open_orders_response.lock().await =
        json!({"orders": [], "subaccount_id": TEST_SUBACCOUNT});
    *rest_state.positions_response.lock().await =
        json!({"positions": [], "subaccount_id": TEST_SUBACCOUNT});
    *rest_state.order_history_response.lock().await = json!({
        "orders": [order_json_with("ord-link", "L-LINK", "buy", "ETH-PERP", 1_700_000_002_000, "filled")],
        "pagination": {"count": 1, "num_pages": 1}, "subaccount_id": TEST_SUBACCOUNT,
    });
    *rest_state.trade_history_response.lock().await = json!({
        "trades": [sample_trade_json("trade-link", fill_order_id, fill_instrument)],
        "pagination": {"count": 1, "num_pages": 1}, "subaccount_id": TEST_SUBACCOUNT,
    });
    let mut tc = build_report_client(rest_state.clone(), WsState::default()).await;
    tc.client.connect().await.unwrap();
    let mass = tc
        .client
        .generate_mass_status(Some(10_000_000))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(mass.reports_complete(), complete);
    assert_eq!(mass.order_reports().len(), 1);
    assert_eq!(mass.fill_reports().len(), 1);
    let report = &mass.fill_reports()[&VenueOrderId::from(fill_order_id)][0];
    assert_eq!(report.trade_id, TradeId::from("trade-link"));
    assert_eq!(
        report.instrument_id,
        InstrumentId::from(format!("{fill_instrument}.DERIVE"))
    );
    assert_eq!(report.last_qty.as_decimal(), dec!(1));
    assert_eq!(report.last_px.as_decimal(), dec!(3505));
    assert_eq!(
        report.commission,
        Money::from_decimal(dec!(0.5), Currency::USDC()).unwrap()
    );
    assert_eq!(rest_state.get_instrument_calls.lock().await.len(), 0);
    assert_eq!(rest_state.get_order_calls.lock().await.len(), 0);
    tc.client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_mass_status_never_infers_flat_for_spot() {
    let rest_state = RestState::default();
    *rest_state.open_orders_response.lock().await =
        json!({"orders": [], "subaccount_id": TEST_SUBACCOUNT});
    *rest_state.positions_response.lock().await =
        json!({"positions": [], "subaccount_id": TEST_SUBACCOUNT});
    *rest_state.order_history_response.lock().await = json!({
        "orders": [order_json_with("ord-spot-coverage", "L-SPOT", "buy", "ETH-USDC", 1_700_000_002_000, "filled")],
        "pagination": {"count": 1, "num_pages": 1}, "subaccount_id": TEST_SUBACCOUNT,
    });
    *rest_state.trade_history_response.lock().await = json!({
        "trades": [sample_trade_json("trade-spot-coverage", "ord-spot-coverage", "ETH-USDC")],
        "pagination": {"count": 1, "num_pages": 1}, "subaccount_id": TEST_SUBACCOUNT,
    });
    let mut tc = build_report_client(rest_state, WsState::default()).await;
    let definition =
        serde_json::from_str(include_str!("../../test_data/spot/instrument_eth.json")).unwrap();
    tc.client.cache_instrument(definition).unwrap();
    tc.client.connect().await.unwrap();
    let mass = tc
        .client
        .generate_mass_status(Some(10_000_000))
        .await
        .unwrap()
        .unwrap();
    assert!(mass.reports_complete());
    assert_eq!(mass.order_reports().len(), 1);
    assert_eq!(
        mass.fill_reports()[&VenueOrderId::from("ord-spot-coverage")][0].trade_id,
        TradeId::from("trade-spot-coverage")
    );
    assert_eq!(mass.position_reports().len(), 0);
    tc.client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_generate_mass_status_builds_startup_snapshot_from_http_reports() {
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    *rest_state.open_orders_response.lock().await = json!({
        "orders": [order_json_with(
            "ord-open-1", "L-OPEN", "buy", "ETH-PERP", 1_700_000_001_000, "open",
        )],
        "subaccount_id": TEST_SUBACCOUNT,
    });
    *rest_state.order_history_response.lock().await = json!({
        "orders": [order_json_with(
            "ord-filled-1", "L-FILLED", "sell", "ETH-PERP", 1_700_000_002_000, "filled",
        )],
        "pagination": {"count": 1, "num_pages": 1},
        "subaccount_id": TEST_SUBACCOUNT,
    });
    *rest_state.trade_history_response.lock().await = json!({
        "trades": [sample_trade_json("trade-fill-1", "ord-filled-1", "ETH-PERP")],
        "pagination": {"count": 1, "num_pages": 1},
        "subaccount_id": TEST_SUBACCOUNT,
    });
    *rest_state.positions_response.lock().await = json!({
        "positions": [sample_position_json("ETH-PERP", "0.3")],
        "subaccount_id": TEST_SUBACCOUNT,
    });
    let mut tc = build_report_client(rest_state.clone(), ws_state).await;
    tc.client.connect().await.expect("connect succeeds");

    let mass_status = tc
        .client
        .generate_mass_status(Some(10_000_000))
        .await
        .expect("mass status request succeeds")
        .expect("Derive returns mass status");

    let order_reports = mass_status.order_reports();
    let fill_reports = mass_status.fill_reports();
    let position_reports = mass_status.position_reports();
    let eth_position_reports = position_reports
        .get(&InstrumentId::from("ETH-PERP.DERIVE"))
        .expect("ETH-PERP position report");

    assert_eq!(mass_status.client_id, ClientId::from("DERIVE"));
    assert_eq!(mass_status.account_id, AccountId::from("DERIVE-001"));
    assert_eq!(mass_status.venue, *DERIVE_VENUE);
    assert_eq!(
        mass_status.lookback_start(),
        Some(
            mass_status
                .ts_init
                .saturating_sub(DurationNanos::try_from_mins(10_000_000).unwrap())
        ),
    );
    assert!(mass_status.reports_complete());
    assert_eq!(order_reports.len(), 2);
    assert!(order_reports.contains_key(&VenueOrderId::from("ord-open-1")));
    assert!(order_reports.contains_key(&VenueOrderId::from("ord-filled-1")));
    assert_eq!(fill_reports.len(), 1);
    assert!(fill_reports.contains_key(&VenueOrderId::from("ord-filled-1")));
    assert_eq!(eth_position_reports.len(), 1);
    assert_eq!(eth_position_reports[0].signed_decimal_qty, dec!(0.3));

    let open_order_calls = rest_state.open_orders_calls.lock().await;
    let order_history_calls = rest_state.order_history_calls.lock().await;
    let trade_history_calls = rest_state.trade_history_calls.lock().await;
    let position_calls = rest_state.positions_calls.lock().await;

    assert_eq!(open_order_calls.len(), 1);
    assert_eq!(order_history_calls.len(), 1);
    assert_eq!(trade_history_calls.len(), 1);
    assert_eq!(position_calls.len(), 1);
    assert!(open_order_calls[0].get("from_timestamp").is_none());
    assert!(
        order_history_calls[0]
            .get("from_timestamp")
            .and_then(Value::as_i64)
            .is_some()
    );
    assert!(
        trade_history_calls[0]
            .get("from_timestamp")
            .and_then(Value::as_i64)
            .is_some()
    );
    assert!(position_calls[0].get("from_timestamp").is_none());

    tc.client.disconnect().await.expect("disconnect");
}

#[rstest]
#[case(" ")]
#[case("external-\u{03bb}")]
#[tokio::test]
async fn test_generate_mass_status_preserves_external_unrepresentable_labels(#[case] label: &str) {
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    *rest_state.open_orders_response.lock().await = json!({
        "orders": [],
        "subaccount_id": TEST_SUBACCOUNT,
    });
    *rest_state.order_history_response.lock().await = json!({
        "orders": [
            order_json_with("ord-external-1", label, "buy", "ETH-PERP", 1_700_000_001_000, "filled"),
            order_json_with("ord-external-2", label, "sell", "ETH-PERP", 1_700_000_002_000, "cancelled"),
        ],
        "pagination": {"count": 2, "num_pages": 1},
        "subaccount_id": TEST_SUBACCOUNT,
    });
    *rest_state.trade_history_response.lock().await = json!({
        "trades": [trade_json_with_label("trade-external-1", "ord-external-1", "ETH-PERP", label)],
        "pagination": {"count": 1, "num_pages": 1},
        "subaccount_id": TEST_SUBACCOUNT,
    });
    let mut tc = build_report_client(rest_state, ws_state).await;
    tc.client.connect().await.expect("connect succeeds");

    let mass_status = tc
        .client
        .generate_mass_status(None)
        .await
        .expect("mass status")
        .expect("mass status report");
    let orders = mass_status.order_reports();
    let fills = mass_status.fill_reports();
    let filled = orders.get(&VenueOrderId::from("ord-external-1")).unwrap();
    let cancelled = orders.get(&VenueOrderId::from("ord-external-2")).unwrap();
    let fill = &fills[&VenueOrderId::from("ord-external-1")][0];

    assert_eq!(orders.len(), 2);
    assert_eq!(filled.client_order_id, None);
    assert_eq!(filled.order_status, OrderStatus::Filled);
    assert_eq!(cancelled.client_order_id, None);
    assert_eq!(cancelled.order_status, OrderStatus::Canceled);
    assert_eq!(fills.len(), 1);
    assert_eq!(fills[&VenueOrderId::from("ord-external-1")].len(), 1);
    assert_eq!(fill.client_order_id, None);
    assert_eq!(fill.trade_id, TradeId::from("trade-external-1"));
    assert_eq!(fill.last_qty.as_decimal(), dec!(1));
    assert_eq!(fill.last_px.as_decimal(), dec!(3505));
    assert_eq!(fill.commission.as_decimal(), dec!(0.5));
    assert_eq!(fill.commission.currency, Currency::USDC());

    tc.client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test]
async fn test_generate_mass_status_normalizes_duplicate_labels() {
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    let old_order = order_json_with(
        "ord-replace-old",
        "L-REPLACE",
        "buy",
        "ETH-PERP",
        1_700_000_001_000,
        "cancelled",
    );
    let mut replacement_order = order_json_with(
        "ord-replace-new",
        "L-REPLACE",
        "buy",
        "ETH-PERP",
        1_700_000_002_000,
        "open",
    );
    replacement_order["replaced_order_id"] = json!("ord-replace-old");
    let reused_order_1 = order_json_with(
        "ord-reused-1",
        "L-REUSED",
        "sell",
        "ETH-PERP",
        1_700_000_003_000,
        "filled",
    );
    let reused_order_2 = order_json_with(
        "ord-reused-2",
        "L-REUSED",
        "sell",
        "ETH-PERP",
        1_700_000_004_000,
        "cancelled",
    );
    let mixed_old_order = order_json_with(
        "ord-mixed-old",
        "L-MIXED",
        "buy",
        "ETH-PERP",
        1_700_000_005_000,
        "cancelled",
    );
    let mut mixed_replacement_order = order_json_with(
        "ord-mixed-new",
        "L-MIXED",
        "buy",
        "ETH-PERP",
        1_700_000_006_000,
        "filled",
    );
    mixed_replacement_order["replaced_order_id"] = json!("ord-mixed-old");
    let mixed_unrelated_order = order_json_with(
        "ord-mixed-unrelated",
        "L-MIXED",
        "sell",
        "ETH-PERP",
        1_700_000_007_000,
        "cancelled",
    );
    let active_history_order = order_json_with(
        "ord-active",
        "L-ACTIVE",
        "buy",
        "ETH-PERP",
        1_700_000_008_000,
        "open",
    );
    let active_label_reuse = order_json_with(
        "ord-active-reuse",
        "L-ACTIVE",
        "sell",
        "ETH-PERP",
        1_700_000_009_000,
        "filled",
    );
    *rest_state.open_orders_response.lock().await = json!({
        "orders": [active_history_order.clone()],
        "subaccount_id": TEST_SUBACCOUNT,
    });
    *rest_state.order_history_response.lock().await = json!({
        "orders": [
            old_order,
            replacement_order,
            reused_order_1,
            reused_order_2,
            mixed_old_order,
            mixed_replacement_order,
            mixed_unrelated_order,
            active_history_order,
            active_label_reuse,
        ],
        "pagination": {"count": 9, "num_pages": 1},
        "subaccount_id": TEST_SUBACCOUNT,
    });
    *rest_state.trade_history_response.lock().await = json!({
        "trades": [trade_json_with_label(
            "trade-reused-1",
            "ord-reused-1",
            "ETH-PERP",
            "L-REUSED",
        )],
        "pagination": {"count": 1, "num_pages": 1},
        "subaccount_id": TEST_SUBACCOUNT,
    });
    let mut tc = build_report_client(rest_state, ws_state).await;
    tc.client.connect().await.expect("connect succeeds");

    let history_cmd = GenerateOrderStatusReports::new(
        UUID4::new(),
        UnixNanos::default(),
        false,
        Some(InstrumentId::from("ETH-PERP.DERIVE")),
        None,
        None,
        None,
        None,
    );
    let history_reports = tc
        .client
        .generate_order_status_reports(&history_cmd)
        .await
        .expect("history reports");
    assert_eq!(history_reports.len(), 9);

    for report in &history_reports {
        let expected = match report.venue_order_id.as_str() {
            "ord-replace-old" | "ord-replace-new" => ClientOrderId::from("L-REPLACE"),
            "ord-reused-1" | "ord-reused-2" => ClientOrderId::from("L-REUSED"),
            "ord-mixed-old" | "ord-mixed-new" | "ord-mixed-unrelated" => {
                ClientOrderId::from("L-MIXED")
            }
            "ord-active" | "ord-active-reuse" => ClientOrderId::from("L-ACTIVE"),
            venue_order_id => panic!("unexpected venue order ID {venue_order_id}"),
        };

        assert_eq!(report.client_order_id, Some(expected));
    }

    let mass_status = tc
        .client
        .generate_mass_status(Some(10_000_000))
        .await
        .expect("mass status request succeeds")
        .expect("Derive returns mass status");
    let reports = mass_status.order_reports();
    let old_report = reports
        .get(&VenueOrderId::from("ord-replace-old"))
        .expect("superseded order report");
    let replacement_report = reports
        .get(&VenueOrderId::from("ord-replace-new"))
        .expect("replacement order report");

    assert_eq!(
        old_report.client_order_id,
        Some(ClientOrderId::from("L-REPLACE")),
    );
    assert_eq!(
        replacement_report.client_order_id,
        Some(ClientOrderId::from("L-REPLACE")),
    );

    for venue_order_id in ["ord-reused-1", "ord-reused-2"] {
        let report = reports
            .get(&VenueOrderId::from(venue_order_id))
            .expect("reused-label order report");
        assert!(report.client_order_id.is_none());
    }

    for venue_order_id in ["ord-mixed-old", "ord-mixed-new", "ord-mixed-unrelated"] {
        let report = reports
            .get(&VenueOrderId::from(venue_order_id))
            .expect("mixed-label order report");
        assert!(report.client_order_id.is_none());
    }

    assert_eq!(
        reports
            .get(&VenueOrderId::from("ord-active"))
            .expect("open order report")
            .client_order_id,
        Some(ClientOrderId::from("L-ACTIVE")),
    );
    assert!(
        reports
            .get(&VenueOrderId::from("ord-active-reuse"))
            .expect("reused active-label order report")
            .client_order_id
            .is_none(),
    );
    let fills = mass_status.fill_reports();
    let reused_fills = fills
        .get(&VenueOrderId::from("ord-reused-1"))
        .expect("reused-label fill reports");
    assert_eq!(reused_fills.len(), 1);
    assert_eq!(reused_fills[0].trade_id, TradeId::from("trade-reused-1"));
    assert!(reused_fills[0].client_order_id.is_none());

    tc.client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test]
async fn test_generate_mass_status_adds_flat_position_without_current_position() {
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    *rest_state.open_orders_response.lock().await = json!({
        "orders": [],
        "subaccount_id": TEST_SUBACCOUNT,
    });
    *rest_state.order_history_response.lock().await = json!({
        "orders": [order_json_with(
            "ord-filled-flat", "L-FILLED-FLAT", "buy", "ETH-PERP", 1_700_000_002_000, "filled",
        )],
        "pagination": {"count": 1, "num_pages": 1},
        "subaccount_id": TEST_SUBACCOUNT,
    });
    *rest_state.trade_history_response.lock().await = json!({
        "trades": [sample_trade_json("trade-flat-1", "ord-filled-flat", "ETH-PERP")],
        "pagination": {"count": 1, "num_pages": 1},
        "subaccount_id": TEST_SUBACCOUNT,
    });
    *rest_state.positions_response.lock().await = json!({
        "positions": [],
        "subaccount_id": TEST_SUBACCOUNT,
    });
    let mut tc = build_report_client(rest_state, ws_state).await;
    tc.client.connect().await.expect("connect succeeds");

    let mass_status = tc
        .client
        .generate_mass_status(Some(10_000_000))
        .await
        .expect("mass status request succeeds")
        .expect("Derive returns mass status");

    let position_reports = mass_status.position_reports();
    let eth_reports = position_reports
        .get(&InstrumentId::from("ETH-PERP.DERIVE"))
        .expect("ETH-PERP flat position report");

    assert_eq!(eth_reports.len(), 1);
    assert_eq!(eth_reports[0].position_side, PositionSide::Flat);
    assert_eq!(eth_reports[0].signed_decimal_qty, dec!(0));

    tc.client.disconnect().await.expect("disconnect");
}

#[rstest]
#[case("0.1234567890123456789012345678912345")]
#[case("0.0001")]
#[tokio::test]
async fn test_generate_mass_status_rejects_unrepresentable_position(#[case] amount: &str) {
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    *rest_state.open_orders_response.lock().await = json!({
        "orders": [],
        "subaccount_id": TEST_SUBACCOUNT,
    });
    *rest_state.order_history_response.lock().await = json!({
        "orders": [
            order_json_with(
                "ord-held-eth", "L-HELD-ETH", "buy", "ETH-PERP", 1_700_000_003_000, "filled",
            ),
            order_json_with(
                "ord-flat-btc", "L-FLAT-BTC", "sell", "BTC-PERP", 1_700_000_004_000, "filled",
            ),
        ],
        "pagination": {"count": 2, "num_pages": 1},
        "subaccount_id": TEST_SUBACCOUNT,
    });
    *rest_state.trade_history_response.lock().await = json!({
        "trades": [],
        "pagination": {"count": 0, "num_pages": 0},
        "subaccount_id": TEST_SUBACCOUNT,
    });
    *rest_state.positions_response.lock().await = json!({
        "positions": [
            sample_position_json("ETH-PERP", amount),
            sample_position_json("SOL-PERP", "2.5"),
        ],
        "subaccount_id": TEST_SUBACCOUNT,
    });
    let mut tc = build_report_client(rest_state, ws_state).await;
    tc.client
        .cache_instrument(serde_json::from_value(sample_instrument_json()).unwrap())
        .unwrap();
    tc.client.connect().await.expect("connect succeeds");

    let err = tc
        .client
        .generate_mass_status(Some(10_000_000))
        .await
        .unwrap_err();

    assert!(format!("{err:#}").contains("cannot be represented exactly"));

    tc.client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test]
async fn test_generate_mass_status_without_lookback_omits_time_window() {
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    *rest_state.open_orders_response.lock().await = json!({
        "orders": [],
        "subaccount_id": TEST_SUBACCOUNT,
    });
    *rest_state.order_history_response.lock().await = json!({
        "orders": [],
        "pagination": {"count": 0, "num_pages": 0},
        "subaccount_id": TEST_SUBACCOUNT,
    });
    *rest_state.trade_history_response.lock().await = json!({
        "trades": [],
        "pagination": {"count": 0, "num_pages": 0},
        "subaccount_id": TEST_SUBACCOUNT,
    });
    *rest_state.positions_response.lock().await = json!({
        "positions": [],
        "subaccount_id": TEST_SUBACCOUNT,
    });
    let mut tc = build_report_client(rest_state.clone(), ws_state).await;
    tc.client.connect().await.expect("connect succeeds");

    let mass_status = tc
        .client
        .generate_mass_status(None)
        .await
        .expect("mass status request succeeds")
        .expect("Derive returns mass status");

    let open_order_calls = rest_state.open_orders_calls.lock().await;
    let order_history_calls = rest_state.order_history_calls.lock().await;
    let trade_history_calls = rest_state.trade_history_calls.lock().await;
    let position_calls = rest_state.positions_calls.lock().await;

    assert!(mass_status.order_reports().is_empty());
    assert!(mass_status.fill_reports().is_empty());
    assert!(mass_status.position_reports().is_empty());
    assert_eq!(open_order_calls.len(), 1);
    assert_eq!(order_history_calls.len(), 1);
    assert_eq!(trade_history_calls.len(), 1);
    assert_eq!(position_calls.len(), 1);
    assert!(open_order_calls[0].get("from_timestamp").is_none());
    assert!(order_history_calls[0].get("from_timestamp").is_none());
    assert!(trade_history_calls[0].get("from_timestamp").is_none());
    assert!(position_calls[0].get("from_timestamp").is_none());

    tc.client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test]
async fn test_generate_mass_status_rejects_conflicting_native_order_identity() {
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    *rest_state.open_orders_response.lock().await = json!({
        "orders": [order_json_with(
            "ord-overlap-1", "L-OPEN", "buy", "ETH-PERP", 1_700_000_003_000, "open",
        )],
        "subaccount_id": TEST_SUBACCOUNT,
    });
    *rest_state.order_history_response.lock().await = json!({
        "orders": [order_json_with(
            "ord-overlap-1", "L-HISTORY", "buy", "ETH-PERP", 1_700_000_002_000, "filled",
        )],
        "pagination": {"count": 1, "num_pages": 1},
        "subaccount_id": TEST_SUBACCOUNT,
    });
    let mut tc = build_report_client(rest_state, ws_state).await;
    tc.client.connect().await.expect("connect succeeds");

    let result = tc.client.generate_mass_status(Some(10_000_000)).await;

    assert_eq!(
        result.unwrap_err().to_string(),
        "Conflicting Derive order identity for ord-overlap-1",
    );

    tc.client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test]
async fn test_generate_order_status_report_by_venue_id_uses_get_order() {
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    let mut tc = build_report_client(rest_state.clone(), ws_state).await;
    tc.client.connect().await.expect("connect succeeds");

    let cmd = GenerateOrderStatusReport::new(
        UUID4::new(),
        UnixNanos::default(),
        Some(InstrumentId::from("ETH-PERP.DERIVE")),
        None,
        Some(VenueOrderId::from("ord-mock-1")),
        None,
        None,
    );
    let report = tc
        .client
        .generate_order_status_report(&cmd)
        .await
        .expect("report")
        .expect("some");
    assert_eq!(report.venue_order_id.as_str(), "ord-mock-1");
    let calls = rest_state.get_order_calls.lock().await;
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0]["order_id"].as_str(), Some("ord-mock-1"));

    tc.client.disconnect().await.expect("disconnect");
}

#[rstest]
#[case(" ")]
#[case("external-\u{03bb}")]
#[tokio::test]
async fn test_generate_order_status_report_invalid_id_returns_error(#[case] invalid_id: &str) {
    let rest_state = RestState::default();
    *rest_state.get_order_response.lock().await = order_json_with(
        invalid_id,
        "STRAT-QUERY",
        "sell",
        "ETH-PERP",
        1_700_000_003_000,
        "open",
    );
    let mut tc = build_report_client(rest_state.clone(), WsState::default()).await;
    tc.client.connect().await.expect("connect");

    let cmd = GenerateOrderStatusReport::new(
        UUID4::new(),
        UnixNanos::default(),
        Some(InstrumentId::from("ETH-PERP.DERIVE")),
        Some(ClientOrderId::from("STRAT-QUERY")),
        Some(VenueOrderId::from("ord-query-known")),
        None,
        None,
    );
    let error = tc.client.generate_order_status_report(&cmd).await.expect_err(
        "invalid returned venue identity must leave targeted coverage incomplete, not report absence",
    );
    assert_eq!(error.to_string(), "invalid Derive order_id");
    assert_eq!(
        *rest_state.get_order_calls.lock().await,
        vec![json!({"subaccount_id": TEST_SUBACCOUNT, "order_id": "ord-query-known"})],
    );
    assert_eq!(rest_state.trigger_orders_calls.lock().await.len(), 2);
    tc.client.disconnect().await.expect("disconnect");
}

#[rstest]
#[case(" ")]
#[case("external-\u{03bb}")]
#[tokio::test]
async fn test_generate_order_status_report_invalid_label_uses_command_id(#[case] label: &str) {
    let rest_state = RestState::default();
    let mut order = order_json_with(
        "ord-query-valid",
        label,
        "sell",
        "ETH-PERP",
        1_700_000_003_000,
        "filled",
    );
    order["amount"] = json!("1.25");
    order["filled_amount"] = json!("1.25");
    *rest_state.get_order_response.lock().await = order;
    let mut tc = build_report_client(rest_state.clone(), WsState::default()).await;
    tc.client.connect().await.expect("connect");
    let client_order_id = ClientOrderId::from("STRAT-QUERY-FALLBACK");
    let venue_order_id = VenueOrderId::from("ord-query-valid");

    let cmd = GenerateOrderStatusReport::new(
        UUID4::new(),
        UnixNanos::default(),
        Some(InstrumentId::from("ETH-PERP.DERIVE")),
        Some(client_order_id),
        Some(venue_order_id),
        None,
        None,
    );
    let report = tc
        .client
        .generate_order_status_report(&cmd)
        .await
        .expect("report")
        .expect("some");
    let mut expected = OrderStatusReport::new(
        AccountId::from("DERIVE-001"),
        InstrumentId::from("ETH-PERP.DERIVE"),
        None,
        venue_order_id,
        Some(OrderSide::Sell),
        OrderType::Limit,
        TimeInForce::Gtc,
        OrderStatus::Filled,
        Quantity::from("1.25"),
        Quantity::from("1.25"),
        UnixNanos::from(1_700_000_000_000_000_000),
        UnixNanos::from(1_700_000_003_000_000_000),
        report.ts_init,
        Some(report.report_id),
    )
    .with_client_order_id(client_order_id)
    .with_price(Price::from("3500"));
    expected.avg_px = Some(dec!(3500));
    assert_eq!(report, expected);
    assert_eq!(
        *rest_state.get_order_calls.lock().await,
        vec![json!({"subaccount_id": TEST_SUBACCOUNT, "order_id": "ord-query-valid"})],
    );
    assert_eq!(rest_state.trigger_orders_calls.lock().await.len(), 2);
    tc.client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test]
async fn test_generate_order_status_report_by_venue_id_falls_back_to_trigger_orders() {
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    *rest_state.get_order_response.lock().await = json!({
        "jsonrpc": "2.0",
        "error": {"code": -32602, "message": "Order not found"},
    });

    let mut tc = build_report_client(rest_state.clone(), ws_state).await;
    tc.client.connect().await.expect("connect succeeds");
    *rest_state.trigger_orders_response.lock().await = json!({
        "orders": [trigger_order_json_with(
            "trig-venue-1",
            "STRAT-TRIG-VENUE",
            "buy",
            "ETH-PERP",
            1_700_000_001_000,
            "market",
            "untriggered",
            "3417",
            "3400",
            "mark",
            "stoploss",
        )],
        "subaccount_id": TEST_SUBACCOUNT,
    });

    let cmd = GenerateOrderStatusReport::new(
        UUID4::new(),
        UnixNanos::default(),
        Some(InstrumentId::from("ETH-PERP.DERIVE")),
        None,
        Some(VenueOrderId::from("trig-venue-1")),
        None,
        None,
    );
    let report = tc
        .client
        .generate_order_status_report(&cmd)
        .await
        .expect("report")
        .expect("some");
    assert_eq!(report.venue_order_id.as_str(), "trig-venue-1");
    assert_eq!(report.order_type, OrderType::StopMarket);
    assert_eq!(report.order_status, OrderStatus::Accepted);
    assert_eq!(
        report.client_order_id,
        Some(ClientOrderId::from("STRAT-TRIG-VENUE"))
    );
    assert_eq!(rest_state.get_order_calls.lock().await.len(), 1);
    assert_eq!(rest_state.trigger_orders_calls.lock().await.len(), 3);

    tc.client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test]
async fn test_ws_orders_notification_emits_order_status_report() {
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    let mut tc = build_client(rest_state, ws_state.clone()).await;
    tc.client.connect().await.expect("connect succeeds");

    // Wait for the connect-time subscribe to land before pushing a
    // notification so the order of operations matches the live venue.
    wait_until(
        || {
            let state = ws_state.clone();
            async move { !state.subscribe_frames.lock().await.is_empty() }
        },
        "subscribe acknowledged",
    )
    .await;

    // Drain the initial account-state event emitted at connect.
    let _ = drain_until(
        &mut tc.rx,
        |e| matches!(e, ExecutionEvent::Account(_)),
        "initial AccountState",
    )
    .await;

    let channel = format!("{TEST_SUBACCOUNT}.orders");
    let data = json!([sample_order_json()]);
    let frame = make_subscription_frame(&channel, &data);
    ws_state.push_notification(frame);

    let event = drain_until(
        &mut tc.rx,
        |e| matches!(e, ExecutionEvent::Report(ExecutionReport::Order(_))),
        "OrderStatusReport from WS",
    )
    .await;

    if let ExecutionEvent::Report(ExecutionReport::Order(report)) = event {
        assert_eq!(report.venue_order_id.as_str(), "ord-mock-1");
    } else {
        unreachable!();
    }

    tc.client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test]
async fn test_ws_trades_notification_emits_fill_report() {
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    let mut tc = build_client(rest_state, ws_state.clone()).await;
    tc.client.connect().await.expect("connect succeeds");

    wait_until(
        || {
            let state = ws_state.clone();
            async move { !state.subscribe_frames.lock().await.is_empty() }
        },
        "subscribe acknowledged",
    )
    .await;

    let _ = drain_until(
        &mut tc.rx,
        |e| matches!(e, ExecutionEvent::Account(_)),
        "initial AccountState",
    )
    .await;

    let channel = format!("{TEST_SUBACCOUNT}.trades");
    let data = json!([sample_trade_json("trade-ws-1", "ord-ws-1", "ETH-PERP")]);
    let frame = make_subscription_frame(&channel, &data);
    ws_state.push_notification(frame);

    let event = drain_until(
        &mut tc.rx,
        |e| matches!(e, ExecutionEvent::Report(ExecutionReport::Fill(_))),
        "FillReport from WS",
    )
    .await;

    if let ExecutionEvent::Report(ExecutionReport::Fill(report)) = event {
        assert_eq!(report.trade_id.as_str(), "trade-ws-1");
        assert_eq!(report.venue_order_id.as_str(), "ord-ws-1");
    } else {
        unreachable!();
    }

    tc.client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test]
async fn test_ws_trades_dedup_suppresses_repeated_trade_id() {
    // The same trade arriving twice on the WS .trades channel (typical
    // immediately after a reconnect replay) must emit only one FillReport.
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    let mut tc = build_client(rest_state, ws_state.clone()).await;
    tc.client.connect().await.expect("connect succeeds");

    wait_until(
        || {
            let state = ws_state.clone();
            async move { !state.subscribe_frames.lock().await.is_empty() }
        },
        "subscribe acknowledged",
    )
    .await;

    let _ = drain_until(
        &mut tc.rx,
        |e| matches!(e, ExecutionEvent::Account(_)),
        "initial AccountState",
    )
    .await;

    let channel = format!("{TEST_SUBACCOUNT}.trades");
    let data = json!([sample_trade_json("trade-dup-1", "ord-dup-1", "ETH-PERP")]);
    let mut early = data.clone();
    early[0]["batch_status"] = Value::Null;
    early[0]["tx_hash"] = Value::Null;
    early[0]["op_uuid"] = Value::Null;
    let mut later = data.clone();
    later[0]["batch_status"] = json!("SettlingError");
    ws_state.push_notification(make_subscription_frame(&channel, &early));
    ws_state.push_notification(make_subscription_frame(&channel, &later));

    let first = drain_until(
        &mut tc.rx,
        |e| matches!(e, ExecutionEvent::Report(ExecutionReport::Fill(_))),
        "first FillReport from WS",
    )
    .await;

    if let ExecutionEvent::Report(ExecutionReport::Fill(report)) = first {
        assert_eq!(report.trade_id.as_str(), "trade-dup-1");
    } else {
        unreachable!();
    }

    // The second frame must be suppressed. Give the dispatch loop enough
    // headroom to process it; if dedup is wired correctly nothing arrives.
    let second = tokio::time::timeout(Duration::from_millis(300), async {
        loop {
            match tc.rx.recv().await {
                Some(ExecutionEvent::Report(ExecutionReport::Fill(_))) => return true,
                Some(_) => {}
                None => return false,
            }
        }
    })
    .await;

    assert!(
        second.is_err(),
        "duplicate trade_id must not produce a second FillReport",
    );

    tc.client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test]
async fn test_cross_source_dedup_skips_ws_trade_in_generate_fill_reports() {
    // WS dispatches a fill first; a subsequent HTTP reconciliation pull whose
    // window overlaps the live stream returns the same trade_id. The HTTP
    // path must drop the duplicate so the reconciler does not re-apply a
    // fill the live engine has already processed.
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    // Reconciliation response carries one trade with the same trade_id the
    // WS will have already emitted, plus one fresh trade that should pass.
    *rest_state.trade_history_response.lock().await = json!({
        "trades": [
            sample_trade_json("trade-shared-1", "ord-1", "ETH-PERP"),
            sample_trade_json("trade-fresh-1", "ord-2", "ETH-PERP"),
        ],
        "pagination": {"count": 2, "num_pages": 1},
        "subaccount_id": TEST_SUBACCOUNT,
    });
    let mut tc = build_report_client(rest_state.clone(), ws_state.clone()).await;
    tc.client.connect().await.expect("connect succeeds");

    wait_until(
        || {
            let state = ws_state.clone();
            async move { !state.subscribe_frames.lock().await.is_empty() }
        },
        "subscribe acknowledged",
    )
    .await;

    let _ = drain_until(
        &mut tc.rx,
        |e| matches!(e, ExecutionEvent::Account(_)),
        "initial AccountState",
    )
    .await;

    // Push the shared trade through WS first so it enters the dedup set.
    let channel = format!("{TEST_SUBACCOUNT}.trades");
    let data = json!([sample_trade_json("trade-shared-1", "ord-1", "ETH-PERP")]);
    ws_state.push_notification(make_subscription_frame(&channel, &data));
    let ws_event = drain_until(
        &mut tc.rx,
        |e| matches!(e, ExecutionEvent::Report(ExecutionReport::Fill(_))),
        "WS FillReport for shared trade",
    )
    .await;

    if let ExecutionEvent::Report(ExecutionReport::Fill(report)) = ws_event {
        assert_eq!(report.trade_id.as_str(), "trade-shared-1");
    } else {
        unreachable!();
    }

    // HTTP reconciliation now returns the same trade plus a fresh one; only
    // the fresh one should survive dedup.
    let cmd = GenerateFillReports::new(
        UUID4::new(),
        UnixNanos::default(),
        Some(InstrumentId::from("ETH-PERP.DERIVE")),
        None,
        None,
        None,
        None,
        None,
    );
    let reports = tc.client.generate_fill_reports(cmd).await.expect("fills");
    assert_eq!(reports.len(), 1, "shared trade must be deduplicated");
    assert_eq!(reports[0].trade_id.as_str(), "trade-fresh-1");

    tc.client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test]
async fn test_generate_fill_reports_does_not_mark_unconsumed_trades_emitted() {
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    *rest_state.trade_history_response.lock().await = json!({
        "trades": [sample_trade_json("trade-retry-1", "ord-1", "ETH-PERP")],
        "pagination": {"count": 1, "num_pages": 1},
        "subaccount_id": TEST_SUBACCOUNT,
    });
    let mut tc = build_report_client(rest_state, ws_state).await;
    tc.client.connect().await.expect("connect succeeds");

    let generate = || {
        GenerateFillReports::new(
            UUID4::new(),
            UnixNanos::default(),
            Some(InstrumentId::from("ETH-PERP.DERIVE")),
            None,
            None,
            None,
            None,
            None,
        )
    };

    let first = tc
        .client
        .generate_fill_reports(generate())
        .await
        .expect("first fill generation succeeds");
    let retry = tc
        .client
        .generate_fill_reports(generate())
        .await
        .expect("retry fill generation succeeds");

    assert_eq!(first.len(), 1);
    assert_eq!(retry.len(), 1);
    assert_eq!(first[0].trade_id.as_str(), "trade-retry-1");
    assert_eq!(retry[0].trade_id.as_str(), "trade-retry-1");

    tc.client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test]
async fn test_ws_trades_failed_commission_conversion_does_not_record_dedup() {
    // The failed construction must not poison dedup for the same trade_id
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    let mut tc = build_client(rest_state, ws_state.clone()).await;
    tc.client.connect().await.expect("connect succeeds");

    wait_until(
        || {
            let state = ws_state.clone();
            async move { !state.subscribe_frames.lock().await.is_empty() }
        },
        "subscribe acknowledged",
    )
    .await;

    let _ = drain_until(
        &mut tc.rx,
        |e| matches!(e, ExecutionEvent::Account(_)),
        "initial AccountState",
    )
    .await;

    let channel = format!("{TEST_SUBACCOUNT}.trades");
    let mut bad_fee = sample_trade_json("trade-fee-ws-1", "ord-fee-ws-1", "ETH-PERP");
    bad_fee["trade_fee"] = json!("79228162514264337593543950335");
    ws_state.push_notification(make_subscription_frame(&channel, &json!([bad_fee])));
    ws_state.push_notification(make_subscription_frame(
        &channel,
        &json!([sample_trade_json(
            "trade-fee-ws-1",
            "ord-fee-ws-1",
            "ETH-PERP"
        )]),
    ));

    let event = drain_until(
        &mut tc.rx,
        |e| matches!(e, ExecutionEvent::Report(ExecutionReport::Fill(_))),
        "FillReport after failed commission conversion",
    )
    .await;

    if let ExecutionEvent::Report(ExecutionReport::Fill(report)) = event {
        assert_eq!(report.trade_id.as_str(), "trade-fee-ws-1");
        assert_eq!(report.commission.as_decimal(), dec!(0.5));
    } else {
        unreachable!();
    }

    tc.client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test]
async fn test_generate_fill_reports_rejects_unrepresentable_commission_and_retries() {
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    let mut bad_fee = sample_trade_json("trade-fee-rest-1", "ord-1", "ETH-PERP");
    bad_fee["trade_fee"] = json!("79228162514264337593543950335");
    *rest_state.trade_history_response.lock().await = json!({
        "trades": [
            bad_fee,
            sample_trade_json("trade-fee-rest-2", "ord-2", "ETH-PERP"),
        ],
        "pagination": {"count": 2, "num_pages": 1},
        "subaccount_id": TEST_SUBACCOUNT,
    });
    let mut tc = build_report_client(rest_state.clone(), ws_state).await;
    tc.client.connect().await.expect("connect succeeds");

    let generate = || {
        GenerateFillReports::new(
            UUID4::new(),
            UnixNanos::default(),
            Some(InstrumentId::from("ETH-PERP.DERIVE")),
            None,
            None,
            None,
            None,
            None,
        )
    };

    let error = tc
        .client
        .generate_fill_reports(generate())
        .await
        .expect_err("unrepresentable commission prevents a partial fill report");
    assert_eq!(
        error.to_string(),
        "failed to construct Derive fill \"trade-fee-rest-1\" for order \"ord-1\" on \"ETH-PERP\"",
    );
    assert!(tc.client.generate_mass_status(Some(1)).await.is_err());

    *rest_state.trade_history_response.lock().await = json!({
        "trades": [sample_trade_json("trade-fee-rest-1", "ord-1", "ETH-PERP")],
        "pagination": {"count": 1, "num_pages": 1},
        "subaccount_id": TEST_SUBACCOUNT,
    });
    let retried = tc
        .client
        .generate_fill_reports(generate())
        .await
        .expect("retry fill generation succeeds");
    assert_eq!(retried.len(), 1);
    assert_eq!(retried[0].trade_id.as_str(), "trade-fee-rest-1");

    tc.client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test]
async fn test_ws_dispatch_tracked_order_open_emits_order_accepted_once() {
    // Submit an order so its identity is registered, then push the venue's
    // `.orders` Open notice twice (the second simulates a reconnect replay).
    // The dispatch must route the first frame to a proper `OrderAccepted`
    // event and suppress the duplicate on the second.
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    let mut tc = build_client(rest_state.clone(), ws_state.clone()).await;
    tc.client.connect().await.expect("connect");

    wait_until(
        || {
            let state = ws_state.clone();
            async move { !state.subscribe_frames.lock().await.is_empty() }
        },
        "subscribe acknowledged",
    )
    .await;

    let _ = drain_until(
        &mut tc.rx,
        |e| matches!(e, ExecutionEvent::Account(_)),
        "initial AccountState",
    )
    .await;

    let instrument_id = InstrumentId::from("ETH-PERP.DERIVE");
    let client_order_id = ClientOrderId::from("STRAT-TRACKED-OPEN");
    let order = build_limit_order(
        instrument_id,
        client_order_id,
        OrderSide::Buy,
        Price::from("3500.00"),
        Quantity::from("1.000"),
    );
    tc.cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .expect("cache insert");
    tc.client
        .submit_order(submit_cmd(&order))
        .expect("submit Ok");

    // OrderSubmitted fires synchronously from `submit_order`.
    let _ = drain_until(
        &mut tc.rx,
        |e| matches!(e, ExecutionEvent::Order(OrderEventAny::Submitted(_))),
        "OrderSubmitted",
    )
    .await;

    let channel = format!("{TEST_SUBACCOUNT}.orders");
    let frame = json!([order_json_with(
        "ord-tracked-1",
        client_order_id.as_str(),
        "buy",
        "ETH-PERP",
        1_700_000_001_000_i64,
        "open",
    )]);
    ws_state.push_notification(make_subscription_frame(&channel, &frame));

    let event = drain_until(
        &mut tc.rx,
        |e| matches!(e, ExecutionEvent::Order(OrderEventAny::Accepted(_))),
        "OrderAccepted on first Open",
    )
    .await;

    if let ExecutionEvent::Order(OrderEventAny::Accepted(accepted)) = event {
        assert_eq!(accepted.client_order_id, client_order_id);
        assert_eq!(accepted.venue_order_id.as_str(), "ord-tracked-1");
        // Identity fields captured at submit must propagate to the event.
        assert_eq!(accepted.strategy_id, StrategyId::from("S-1"));
        assert_eq!(accepted.instrument_id, instrument_id);
    } else {
        unreachable!();
    }

    // Replay the same Open frame. The dispatch must suppress the duplicate
    // Accepted and must not emit an OrderStatusReport fallback.
    ws_state.push_notification(make_subscription_frame(&channel, &frame));

    let outcome = tokio::time::timeout(Duration::from_millis(200), async {
        loop {
            match tc.rx.recv().await {
                Some(ExecutionEvent::Order(OrderEventAny::Accepted(_))) => {
                    return Some("duplicate Accepted");
                }
                Some(ExecutionEvent::Report(ExecutionReport::Order(_))) => {
                    return Some("fallback OrderStatusReport");
                }
                Some(_) => {}
                None => return None,
            }
        }
    })
    .await;

    assert!(
        outcome.is_err(),
        "tracked replay must not emit further events, was {outcome:?}",
    );

    tc.client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test]
async fn test_ws_dispatch_tracked_fill_emits_order_filled_and_dedupes_by_trade_id() {
    // Submit an order, then push a `.trades` frame whose label matches the
    // tracked order. The dispatch must synthesize Accepted (since no Open
    // came first), emit OrderFilled (not FillReport), and drop a replayed
    // trade with the same trade_id.
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    let mut tc = build_client(rest_state.clone(), ws_state.clone()).await;
    tc.client.connect().await.expect("connect");

    wait_until(
        || {
            let state = ws_state.clone();
            async move { !state.subscribe_frames.lock().await.is_empty() }
        },
        "subscribe acknowledged",
    )
    .await;

    let _ = drain_until(
        &mut tc.rx,
        |e| matches!(e, ExecutionEvent::Account(_)),
        "initial AccountState",
    )
    .await;

    let instrument_id = InstrumentId::from("ETH-PERP.DERIVE");
    let client_order_id = ClientOrderId::from("STRAT-TRACKED-FILL");
    let order = build_limit_order(
        instrument_id,
        client_order_id,
        OrderSide::Buy,
        Price::from("3500.00"),
        Quantity::from("1.000"),
    );
    tc.cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .expect("cache insert");
    tc.client
        .submit_order(submit_cmd(&order))
        .expect("submit Ok");

    let _ = drain_until(
        &mut tc.rx,
        |e| matches!(e, ExecutionEvent::Order(OrderEventAny::Submitted(_))),
        "OrderSubmitted",
    )
    .await;

    let channel = format!("{TEST_SUBACCOUNT}.trades");
    let frame = json!([trade_json_with_label(
        "trade-tracked-1",
        "ord-tracked-1",
        "ETH-PERP",
        client_order_id.as_str(),
    )]);
    ws_state.push_notification(make_subscription_frame(&channel, &frame));

    // Synthesized Accepted lands before the Filled, in lifecycle order.
    let _ = drain_until(
        &mut tc.rx,
        |e| matches!(e, ExecutionEvent::Order(OrderEventAny::Accepted(_))),
        "synthesized OrderAccepted",
    )
    .await;
    let event = drain_until(
        &mut tc.rx,
        |e| matches!(e, ExecutionEvent::Order(OrderEventAny::Filled(_))),
        "OrderFilled on tracked trade",
    )
    .await;

    if let ExecutionEvent::Order(OrderEventAny::Filled(filled)) = event {
        assert_eq!(filled.client_order_id, client_order_id);
        assert_eq!(filled.trade_id.as_str(), "trade-tracked-1");
    } else {
        unreachable!();
    }

    // Replay the same trade. Dedup must drop it: no further Filled, no
    // fallback FillReport.
    ws_state.push_notification(make_subscription_frame(&channel, &frame));

    let outcome = tokio::time::timeout(Duration::from_millis(200), async {
        loop {
            match tc.rx.recv().await {
                Some(ExecutionEvent::Order(OrderEventAny::Filled(_))) => {
                    return Some("duplicate Filled");
                }
                Some(ExecutionEvent::Report(ExecutionReport::Fill(_))) => {
                    return Some("fallback FillReport");
                }
                Some(_) => {}
                None => return None,
            }
        }
    })
    .await;

    assert!(
        outcome.is_err(),
        "replayed trade must be deduped, was {outcome:?}",
    );

    tc.client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test]
async fn test_ws_dispatch_orders_filled_before_trades_still_emits_tracked_fill() {
    // Venue split-channel ordering: the `.orders` Filled notice can arrive
    // before the matching `.trades` record. The dispatch must keep the
    // tracked identity alive across that gap so the trade still emits
    // `OrderFilled` instead of falling through to `FillReport`.
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    let mut tc = build_client(rest_state.clone(), ws_state.clone()).await;
    tc.client.connect().await.expect("connect");

    wait_until(
        || {
            let state = ws_state.clone();
            async move { !state.subscribe_frames.lock().await.is_empty() }
        },
        "subscribe acknowledged",
    )
    .await;

    let _ = drain_until(
        &mut tc.rx,
        |e| matches!(e, ExecutionEvent::Account(_)),
        "initial AccountState",
    )
    .await;

    let instrument_id = InstrumentId::from("ETH-PERP.DERIVE");
    let client_order_id = ClientOrderId::from("STRAT-SPLIT-CHAN");
    let order = build_limit_order(
        instrument_id,
        client_order_id,
        OrderSide::Buy,
        Price::from("3500.00"),
        Quantity::from("1.000"),
    );
    tc.cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .expect("cache insert");
    tc.client
        .submit_order(submit_cmd(&order))
        .expect("submit Ok");

    let _ = drain_until(
        &mut tc.rx,
        |e| matches!(e, ExecutionEvent::Order(OrderEventAny::Submitted(_))),
        "OrderSubmitted",
    )
    .await;

    // `.orders` Filled arrives first.
    let orders_channel = format!("{TEST_SUBACCOUNT}.orders");
    let orders_frame = json!([order_json_with(
        "ord-split-1",
        client_order_id.as_str(),
        "buy",
        "ETH-PERP",
        1_700_000_005_000_i64,
        "filled",
    )]);
    ws_state.push_notification(make_subscription_frame(&orders_channel, &orders_frame));
    let _ = drain_until(
        &mut tc.rx,
        |e| matches!(e, ExecutionEvent::Order(OrderEventAny::Accepted(_))),
        "synthesized OrderAccepted from terminal `.orders` Filled",
    )
    .await;

    // Now the matching `.trades` frame lands. Must take the tracked path
    // and emit `OrderFilled`, not a `FillReport`.
    let trades_channel = format!("{TEST_SUBACCOUNT}.trades");
    let trades_frame = json!([trade_json_with_label(
        "trade-split-1",
        "ord-split-1",
        "ETH-PERP",
        client_order_id.as_str(),
    )]);
    ws_state.push_notification(make_subscription_frame(&trades_channel, &trades_frame));

    let event = drain_until(
        &mut tc.rx,
        |e| {
            matches!(
                e,
                ExecutionEvent::Order(OrderEventAny::Filled(_))
                    | ExecutionEvent::Report(ExecutionReport::Fill(_))
            )
        },
        "fill emission",
    )
    .await;

    match event {
        ExecutionEvent::Order(OrderEventAny::Filled(filled)) => {
            assert_eq!(filled.client_order_id, client_order_id);
            assert_eq!(filled.trade_id.as_str(), "trade-split-1");
        }
        ExecutionEvent::Report(_) => {
            panic!(
                "tracked fill must not fall back to FillReport when `.orders` Filled came first"
            );
        }
        _ => unreachable!(),
    }

    tc.client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test]
async fn test_ws_dispatch_external_order_falls_back_to_status_report() {
    // A `.orders` frame whose label has no registered identity (external or
    // pre-existing order) must take the report path so the reconciler can
    // ingest the state without misrouted lifecycle events.
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    let mut tc = build_client(rest_state.clone(), ws_state.clone()).await;
    tc.client.connect().await.expect("connect");

    wait_until(
        || {
            let state = ws_state.clone();
            async move { !state.subscribe_frames.lock().await.is_empty() }
        },
        "subscribe acknowledged",
    )
    .await;

    let _ = drain_until(
        &mut tc.rx,
        |e| matches!(e, ExecutionEvent::Account(_)),
        "initial AccountState",
    )
    .await;

    let channel = format!("{TEST_SUBACCOUNT}.orders");
    let frame = json!([order_json_with(
        "ord-external-1",
        "EXTERNAL-LABEL",
        "buy",
        "ETH-PERP",
        1_700_000_001_000_i64,
        "open",
    )]);
    ws_state.push_notification(make_subscription_frame(&channel, &frame));

    let event = drain_until(
        &mut tc.rx,
        |e| matches!(e, ExecutionEvent::Report(ExecutionReport::Order(_))),
        "OrderStatusReport for external order",
    )
    .await;

    if let ExecutionEvent::Report(ExecutionReport::Order(report)) = event {
        assert_eq!(report.venue_order_id.as_str(), "ord-external-1");
        assert_eq!(
            report.client_order_id.map(|c| c.as_str().to_string()),
            Some("EXTERNAL-LABEL".to_string()),
        );
    } else {
        unreachable!();
    }

    tc.client.disconnect().await.expect("disconnect");
}

#[rstest]
#[case::canceled("cancelled", "canceled")]
#[case::expired("expired", "expired")]
#[tokio::test]
async fn test_ws_dispatch_tracked_terminal_status_emits_once_and_suppresses_replay(
    #[case] status: &str,
    #[case] expected: &str,
) {
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    let mut tc = build_client(rest_state.clone(), ws_state.clone()).await;
    tc.client.connect().await.expect("connect");

    wait_until(
        || {
            let state = ws_state.clone();
            async move { !state.subscribe_frames.lock().await.is_empty() }
        },
        "subscribe acknowledged",
    )
    .await;

    let _ = drain_until(
        &mut tc.rx,
        |e| matches!(e, ExecutionEvent::Account(_)),
        "initial AccountState",
    )
    .await;

    let instrument_id = InstrumentId::from("ETH-PERP.DERIVE");
    let client_order_id = ClientOrderId::from("STRAT-TERMINAL");
    let order = build_limit_order(
        instrument_id,
        client_order_id,
        OrderSide::Buy,
        Price::from("3500.00"),
        Quantity::from("1.000"),
    );
    tc.cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .expect("cache insert");
    tc.client
        .submit_order(submit_cmd(&order))
        .expect("submit Ok");

    let _ = drain_until(
        &mut tc.rx,
        |e| matches!(e, ExecutionEvent::Order(OrderEventAny::Submitted(_))),
        "OrderSubmitted",
    )
    .await;

    // Distinct creation vs. last_update lets us verify which one each event
    // carries: the synthesized Accepted must use creation_timestamp, the
    // terminal event must use last_update_timestamp.
    let creation_ms: i64 = 1_700_000_000_000;
    let last_update_ms: i64 = 1_700_000_005_000;
    let expected_accepted_ns = UnixNanos::from((creation_ms as u64) * 1_000_000);
    let expected_terminal_ns = UnixNanos::from((last_update_ms as u64) * 1_000_000);

    let channel = format!("{TEST_SUBACCOUNT}.orders");
    let frame = json!([order_json_with(
        "ord-terminal-1",
        client_order_id.as_str(),
        "buy",
        "ETH-PERP",
        last_update_ms,
        status,
    )]);
    ws_state.push_notification(make_subscription_frame(&channel, &frame));

    let accepted_event = drain_until(
        &mut tc.rx,
        |e| matches!(e, ExecutionEvent::Order(OrderEventAny::Accepted(_))),
        "synthesized OrderAccepted",
    )
    .await;

    if let ExecutionEvent::Order(OrderEventAny::Accepted(accepted)) = accepted_event {
        assert_eq!(accepted.client_order_id, client_order_id);
        assert_eq!(accepted.venue_order_id.as_str(), "ord-terminal-1");
        assert_eq!(
            accepted.ts_event, expected_accepted_ns,
            "synthesized Accepted must carry ts_accepted (creation_timestamp)",
        );
    } else {
        unreachable!();
    }

    match expected {
        "canceled" => {
            let event = drain_until(
                &mut tc.rx,
                |e| matches!(e, ExecutionEvent::Order(OrderEventAny::Canceled(_))),
                "OrderCanceled",
            )
            .await;

            if let ExecutionEvent::Order(OrderEventAny::Canceled(canceled)) = event {
                assert_eq!(canceled.client_order_id, client_order_id);
                assert_eq!(canceled.venue_order_id.unwrap().as_str(), "ord-terminal-1");
                assert_eq!(canceled.ts_event, expected_terminal_ns);
            } else {
                unreachable!();
            }
        }
        "expired" => {
            let event = drain_until(
                &mut tc.rx,
                |e| matches!(e, ExecutionEvent::Order(OrderEventAny::Expired(_))),
                "OrderExpired",
            )
            .await;

            if let ExecutionEvent::Order(OrderEventAny::Expired(expired)) = event {
                assert_eq!(expired.client_order_id, client_order_id);
                assert_eq!(expired.venue_order_id.unwrap().as_str(), "ord-terminal-1");
                assert_eq!(expired.ts_event, expected_terminal_ns);
            } else {
                unreachable!();
            }
        }
        _ => unreachable!("unexpected variant marker {expected}"),
    }

    ws_state.push_notification(make_subscription_frame(&channel, &frame));
    let replay = tokio::time::timeout(Duration::from_millis(200), tc.rx.recv()).await;
    assert!(
        replay.is_err(),
        "terminal replay emits no status or lifecycle event: {replay:?}"
    );

    tc.client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test]
async fn test_ws_dispatch_tracked_rejected_emits_rejected_without_synthesized_accepted() {
    // Rejected is deliberately asymmetric with Canceled/Expired: a venue-side
    // rejection can precede any Accepted notice, so the dispatch must NOT
    // synthesize an Accepted before the Rejected event.
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    let mut tc = build_client(rest_state.clone(), ws_state.clone()).await;
    tc.client.connect().await.expect("connect");

    wait_until(
        || {
            let state = ws_state.clone();
            async move { !state.subscribe_frames.lock().await.is_empty() }
        },
        "subscribe acknowledged",
    )
    .await;

    let _ = drain_until(
        &mut tc.rx,
        |e| matches!(e, ExecutionEvent::Account(_)),
        "initial AccountState",
    )
    .await;

    let instrument_id = InstrumentId::from("ETH-PERP.DERIVE");
    let client_order_id = ClientOrderId::from("STRAT-REJECTED-WS");
    let order = build_limit_order(
        instrument_id,
        client_order_id,
        OrderSide::Buy,
        Price::from("3500.00"),
        Quantity::from("1.000"),
    );
    tc.cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .expect("cache insert");
    tc.client
        .submit_order(submit_cmd(&order))
        .expect("submit Ok");

    let _ = drain_until(
        &mut tc.rx,
        |e| matches!(e, ExecutionEvent::Order(OrderEventAny::Submitted(_))),
        "OrderSubmitted",
    )
    .await;

    let channel = format!("{TEST_SUBACCOUNT}.orders");
    let frame = json!([order_json_with(
        "ord-rej-1",
        client_order_id.as_str(),
        "buy",
        "ETH-PERP",
        1_700_000_002_000_i64,
        "rejected",
    )]);
    ws_state.push_notification(make_subscription_frame(&channel, &frame));

    // The first lifecycle event that lands must be the Rejected itself, with
    // no synthesized Accepted preceding it.
    let event = drain_until(
        &mut tc.rx,
        |e| {
            matches!(
                e,
                ExecutionEvent::Order(OrderEventAny::Accepted(_) | OrderEventAny::Rejected(_),)
            )
        },
        "Rejected (without prior Accepted)",
    )
    .await;

    match event {
        ExecutionEvent::Order(OrderEventAny::Rejected(rejected)) => {
            assert_eq!(rejected.client_order_id, client_order_id);
            assert_eq!(rejected.reason, "Order rejected by Derive");
            assert!(!rejected.due_post_only);
        }
        ExecutionEvent::Order(OrderEventAny::Accepted(_)) => {
            panic!("Rejected path must not synthesize OrderAccepted");
        }
        _ => unreachable!(),
    }

    tc.client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test]
async fn test_ws_dispatch_post_only_cross_rejected_sets_due_post_only() {
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    let mut tc = build_client(rest_state.clone(), ws_state.clone()).await;
    tc.client.connect().await.expect("connect");

    wait_until(
        || {
            let state = ws_state.clone();
            async move { !state.subscribe_frames.lock().await.is_empty() }
        },
        "subscribe acknowledged",
    )
    .await;

    let _ = drain_until(
        &mut tc.rx,
        |e| matches!(e, ExecutionEvent::Account(_)),
        "initial AccountState",
    )
    .await;

    let instrument_id = InstrumentId::from("ETH-PERP.DERIVE");
    let client_order_id = ClientOrderId::from("STRAT-POST-ONLY-WS");
    let order = build_limit_order_with_time_in_force(
        instrument_id,
        client_order_id,
        OrderSide::Buy,
        Price::from("3500.00"),
        Quantity::from("1.000"),
        TimeInForce::Gtc,
        true,
    );
    tc.cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .expect("cache insert");
    tc.client
        .submit_order(submit_cmd(&order))
        .expect("submit Ok");

    let _ = drain_until(
        &mut tc.rx,
        |e| matches!(e, ExecutionEvent::Order(OrderEventAny::Submitted(_))),
        "OrderSubmitted",
    )
    .await;

    let channel = format!("{TEST_SUBACCOUNT}.orders");
    let mut order_update = order_json_with(
        "ord-post-only-rej-1",
        client_order_id.as_str(),
        "buy",
        "ETH-PERP",
        1_700_000_002_000_i64,
        "rejected",
    );
    order_update["cancel_reason"] = json!("Post only order cannot cross the market");
    order_update["time_in_force"] = json!("post_only");
    let frame = json!([order_update]);
    ws_state.push_notification(make_subscription_frame(&channel, &frame));

    let event = drain_until(
        &mut tc.rx,
        |e| {
            matches!(
                e,
                ExecutionEvent::Order(OrderEventAny::Accepted(_) | OrderEventAny::Rejected(_),)
            )
        },
        "post-only Rejected",
    )
    .await;

    match event {
        ExecutionEvent::Order(OrderEventAny::Rejected(rejected)) => {
            assert_eq!(rejected.client_order_id, client_order_id);
            assert_eq!(rejected.reason, "Post only order cannot cross the market");
            assert!(rejected.due_post_only);
        }
        ExecutionEvent::Order(OrderEventAny::Accepted(_)) => {
            panic!("Rejected path must not synthesize OrderAccepted");
        }
        _ => unreachable!(),
    }

    tc.client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test]
async fn test_submit_order_jsonrpc_rejection_suppresses_stale_status() {
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    *ws_state.order_reply.lock().await = Some(json!({
        "error": {"code": -32602, "message": "Invalid params"}
    }));
    let mut tc = build_client(rest_state, ws_state.clone()).await;
    tc.client.connect().await.expect("connect succeeds");

    wait_until(
        || {
            let state = ws_state.clone();
            async move { !state.subscribe_frames.lock().await.is_empty() }
        },
        "subscribe acknowledged",
    )
    .await;

    let _ = drain_until(
        &mut tc.rx,
        |e| matches!(e, ExecutionEvent::Account(_)),
        "initial AccountState",
    )
    .await;

    let instrument_id = InstrumentId::from("ETH-PERP.DERIVE");
    let client_order_id = ClientOrderId::from("STRAT-FORGET-1");
    let order = build_limit_order(
        instrument_id,
        client_order_id,
        OrderSide::Buy,
        Price::from("3500.00"),
        Quantity::from("1.000"),
    );
    tc.cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .expect("cache insert");
    tc.client
        .submit_order(submit_cmd(&order))
        .expect("submit Ok");

    let _ = drain_until(
        &mut tc.rx,
        |e| matches!(e, ExecutionEvent::Order(OrderEventAny::Rejected(_))),
        "OrderRejected after JSON-RPC error",
    )
    .await;

    let channel = format!("{TEST_SUBACCOUNT}.orders");
    let frame = json!([order_json_with(
        "ord-stale-after-reject",
        client_order_id.as_str(),
        "buy",
        "ETH-PERP",
        1_700_000_006_000_i64,
        "open",
    )]);
    ws_state.push_notification(make_subscription_frame(&channel, &frame));

    let replay = tokio::time::timeout(Duration::from_millis(200), tc.rx.recv()).await;
    assert!(
        replay.is_err(),
        "rejected order replay emits no status or acceptance: {replay:?}"
    );

    tc.client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test]
async fn test_ws_dispatch_filled_then_replayed_open_is_suppressed() {
    // After a tracked Filled, a replayed `.orders` Open (typical reconnect
    // replay window) must not re-emit OrderAccepted. The `contains_filled`
    // guard short-circuits the Accepted path.
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    let mut tc = build_client(rest_state.clone(), ws_state.clone()).await;
    tc.client.connect().await.expect("connect");

    wait_until(
        || {
            let state = ws_state.clone();
            async move { !state.subscribe_frames.lock().await.is_empty() }
        },
        "subscribe acknowledged",
    )
    .await;

    let _ = drain_until(
        &mut tc.rx,
        |e| matches!(e, ExecutionEvent::Account(_)),
        "initial AccountState",
    )
    .await;

    let instrument_id = InstrumentId::from("ETH-PERP.DERIVE");
    let client_order_id = ClientOrderId::from("STRAT-FILLED-REPLAY");
    let order = build_limit_order(
        instrument_id,
        client_order_id,
        OrderSide::Buy,
        Price::from("3500.00"),
        Quantity::from("1.000"),
    );
    tc.cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .expect("cache insert");
    tc.client
        .submit_order(submit_cmd(&order))
        .expect("submit Ok");

    let _ = drain_until(
        &mut tc.rx,
        |e| matches!(e, ExecutionEvent::Order(OrderEventAny::Submitted(_))),
        "OrderSubmitted",
    )
    .await;

    let channel = format!("{TEST_SUBACCOUNT}.orders");
    let filled_frame = json!([order_json_with(
        "ord-filled-replay",
        client_order_id.as_str(),
        "buy",
        "ETH-PERP",
        1_700_000_007_000_i64,
        "filled",
    )]);
    ws_state.push_notification(make_subscription_frame(&channel, &filled_frame));
    let _ = drain_until(
        &mut tc.rx,
        |e| matches!(e, ExecutionEvent::Order(OrderEventAny::Accepted(_))),
        "synthesized OrderAccepted from Filled",
    )
    .await;

    // Replay an Open frame for the same CID. Must not re-emit OrderAccepted.
    let open_frame = json!([order_json_with(
        "ord-filled-replay",
        client_order_id.as_str(),
        "buy",
        "ETH-PERP",
        1_700_000_008_000_i64,
        "open",
    )]);
    ws_state.push_notification(make_subscription_frame(&channel, &open_frame));

    let outcome = tokio::time::timeout(Duration::from_millis(200), async {
        loop {
            match tc.rx.recv().await {
                Some(ExecutionEvent::Order(OrderEventAny::Accepted(_))) => {
                    return Some("duplicate Accepted after Filled");
                }
                Some(_) => {}
                None => return None,
            }
        }
    })
    .await;

    assert!(
        outcome.is_err(),
        "replayed Open after Filled must be suppressed, was {outcome:?}",
    );

    tc.client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test]
async fn test_submit_market_buy_full_lifecycle_emits_accepted_then_filled() {
    // TC-E01: market BUY end-to-end. Walks the dispatch path from submit
    // (OrderSubmitted + REST POST), through `.orders` Open (OrderAccepted),
    // `.trades` (OrderFilled with venue trade fields), and a trailing
    // `.orders` Filled which must be a no-op (Accepted is already marked
    // and tracked Filled emits only from the trade path).
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    let mut tc = build_client(rest_state, ws_state.clone()).await;
    tc.client.connect().await.expect("connect");

    wait_until(
        || {
            let state = ws_state.clone();
            async move { !state.subscribe_frames.lock().await.is_empty() }
        },
        "subscribe acknowledged",
    )
    .await;

    let _ = drain_until(
        &mut tc.rx,
        |e| matches!(e, ExecutionEvent::Account(_)),
        "initial AccountState",
    )
    .await;

    let instrument_id = InstrumentId::from("ETH-PERP.DERIVE");
    let client_order_id = ClientOrderId::from("STRAT-MKT-BUY-E01");

    // Market orders need top-of-book to compute the slippage bound; see
    // `test_submit_order_market_with_quote_uses_rounded_slippage_bound`.
    let quote = QuoteTick::new(
        instrument_id,
        Price::from("3500.00"),
        Price::from("3501.00"),
        Quantity::from("1.000"),
        Quantity::from("1.000"),
        UnixNanos::default(),
        UnixNanos::default(),
    );
    tc.cache
        .borrow_mut()
        .add_quote(quote)
        .expect("quote insert");

    let order = build_market_order(
        instrument_id,
        client_order_id,
        OrderSide::Buy,
        Quantity::from("1.000"),
    );
    tc.cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .expect("cache insert");

    tc.client
        .submit_order(submit_cmd(&order))
        .expect("submit Ok");

    let _ = drain_until(
        &mut tc.rx,
        |e| matches!(e, ExecutionEvent::Order(OrderEventAny::Submitted(_))),
        "OrderSubmitted",
    )
    .await;
    wait_until(
        || {
            let state = ws_state.clone();
            async move { !state.submitted_orders.lock().await.is_empty() }
        },
        "private/order posted",
    )
    .await;

    let orders_channel = format!("{TEST_SUBACCOUNT}.orders");
    let open_frame = json!([order_json_with(
        "ord-mkt-buy-1",
        client_order_id.as_str(),
        "buy",
        "ETH-PERP",
        1_700_000_001_000_i64,
        "open",
    )]);
    ws_state.push_notification(make_subscription_frame(&orders_channel, &open_frame));

    let accepted = drain_until(
        &mut tc.rx,
        |e| matches!(e, ExecutionEvent::Order(OrderEventAny::Accepted(_))),
        "OrderAccepted on .orders Open",
    )
    .await;

    if let ExecutionEvent::Order(OrderEventAny::Accepted(accepted)) = accepted {
        assert_eq!(accepted.client_order_id, client_order_id);
        assert_eq!(accepted.venue_order_id.as_str(), "ord-mkt-buy-1");
        assert_eq!(accepted.instrument_id, instrument_id);
    } else {
        unreachable!();
    }

    let trades_channel = format!("{TEST_SUBACCOUNT}.trades");
    let trade_frame = json!([trade_json_with_label(
        "trade-mkt-buy-1",
        "ord-mkt-buy-1",
        "ETH-PERP",
        client_order_id.as_str(),
    )]);
    ws_state.push_notification(make_subscription_frame(&trades_channel, &trade_frame));

    let filled = drain_until(
        &mut tc.rx,
        |e| matches!(e, ExecutionEvent::Order(OrderEventAny::Filled(_))),
        "OrderFilled on .trades",
    )
    .await;

    if let ExecutionEvent::Order(OrderEventAny::Filled(filled)) = filled {
        assert_eq!(filled.client_order_id, client_order_id);
        assert_eq!(filled.venue_order_id.as_str(), "ord-mkt-buy-1");
        assert_eq!(filled.trade_id.as_str(), "trade-mkt-buy-1");
        assert_eq!(filled.order_side, OrderSide::Buy);
        assert_eq!(filled.last_qty.as_decimal(), dec!(1));
        assert_eq!(filled.last_px.as_decimal(), dec!(3505));
    } else {
        unreachable!();
    }

    let filled_frame = json!([order_json_with(
        "ord-mkt-buy-1",
        client_order_id.as_str(),
        "buy",
        "ETH-PERP",
        1_700_000_003_000_i64,
        "filled",
    )]);
    ws_state.push_notification(make_subscription_frame(&orders_channel, &filled_frame));

    let outcome = tokio::time::timeout(Duration::from_millis(200), async {
        loop {
            match tc.rx.recv().await {
                Some(ExecutionEvent::Order(OrderEventAny::Accepted(_))) => {
                    return Some("duplicate Accepted after fill");
                }
                Some(ExecutionEvent::Order(OrderEventAny::Filled(_))) => {
                    return Some("duplicate Filled after fill");
                }
                Some(ExecutionEvent::Report(_)) => {
                    return Some("fallback report after tracked fill");
                }
                Some(_) => {}
                None => return None,
            }
        }
    })
    .await;

    assert!(
        outcome.is_err(),
        "trailing .orders Filled must be a no-op, was {outcome:?}",
    );

    tc.client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test]
async fn test_cancel_order_jsonrpc_definitive_rejection_emits_order_cancel_rejected() {
    // TC-E40: a venue cancel for a definitive rejection (invalid params,
    // unknown order) must translate to OrderCancelRejected so the engine
    // clears the PendingCancel state. Uses `-32602` (invalid params), a
    // non-retryable JSON-RPC code per `is_retryable_jsonrpc_code`.
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    *ws_state.cancel_reply.lock().await = Some(json!({
        "error": {"code": -32602, "message": "Order already canceled"}
    }));
    let mut tc = build_client(rest_state, ws_state).await;
    tc.client.connect().await.expect("connect succeeds");

    let cancel = CancelOrder::new(
        TraderId::from("TRADER-001"),
        Some(ClientId::from("DERIVE")),
        StrategyId::from("S-1"),
        InstrumentId::from("ETH-PERP.DERIVE"),
        ClientOrderId::from("STRAT-CXL-E40"),
        Some(VenueOrderId::from("ord-already-canceled")),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );
    tc.client.cancel_order(cancel).expect("cancel_order Ok");

    let event = drain_until(
        &mut tc.rx,
        |e| matches!(e, ExecutionEvent::Order(OrderEventAny::CancelRejected(_))),
        "OrderCancelRejected on definitive JSON-RPC error",
    )
    .await;

    if let ExecutionEvent::Order(OrderEventAny::CancelRejected(rejected)) = event {
        assert_eq!(
            rejected.client_order_id,
            ClientOrderId::from("STRAT-CXL-E40")
        );
        assert_eq!(
            rejected.venue_order_id.map(|v| v.as_str().to_string()),
            Some("ord-already-canceled".to_string()),
        );
        let reason = rejected.reason.as_str();
        assert!(
            reason.contains("-32602") && reason.contains("already canceled"),
            "unexpected cancel-reject reason: {reason}",
        );
    } else {
        unreachable!();
    }

    tc.client.disconnect().await.expect("disconnect");
}

#[rstest]
#[case::internal_error(-32603)]
#[case::order_confirmation_timeout(9000)]
#[case::engine_confirmation_timeout(9001)]
#[tokio::test]
async fn test_cancel_order_jsonrpc_ambiguous_does_not_emit_cancel_rejected(#[case] code: i64) {
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    *ws_state.cancel_reply.lock().await = Some(json!({
        "error": {"code": code, "message": "Internal venue error"}
    }));
    let mut tc = build_client(rest_state, ws_state.clone()).await;
    tc.client.connect().await.expect("connect succeeds");

    let cancel = CancelOrder::new(
        TraderId::from("TRADER-001"),
        Some(ClientId::from("DERIVE")),
        StrategyId::from("S-1"),
        InstrumentId::from("ETH-PERP.DERIVE"),
        ClientOrderId::from("STRAT-CXL-RETRY"),
        Some(VenueOrderId::from("ord-retry-1")),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );
    tc.client.cancel_order(cancel).expect("cancel_order Ok");
    wait_until(
        || {
            let state = ws_state.clone();
            async move { !state.cancelled_orders.lock().await.is_empty() }
        },
        "cancel posted",
    )
    .await;

    let outcome = tokio::time::timeout(Duration::from_millis(200), async {
        loop {
            match tc.rx.recv().await {
                Some(ExecutionEvent::Order(OrderEventAny::CancelRejected(_))) => {
                    return Some("unexpected OrderCancelRejected on retryable code");
                }
                Some(_) => {}
                None => return None,
            }
        }
    })
    .await;

    assert!(
        outcome.is_err(),
        "retryable JSON-RPC code must not emit OrderCancelRejected, was {outcome:?}",
    );

    assert_eq!(ws_state.cancelled_orders.lock().await.len(), 1);

    tc.client.disconnect().await.expect("disconnect");
}

#[rstest]
#[case(-32602, "Invalid params")]
#[case(-32603, "Internal venue error")]
#[tokio::test]
async fn test_cancel_all_orders_bulk_failures_emit_no_order_events(
    #[case] code: i64,
    #[case] message: &str,
) {
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    *ws_state.cancel_by_instrument_reply.lock().await = Some(json!({
        "error": {"code": code, "message": message}
    }));
    let mut tc = build_client(rest_state, ws_state.clone()).await;
    tc.client.connect().await.expect("connect succeeds");

    let cmd = CancelAllOrders::new(
        TraderId::from("TRADER-001"),
        Some(ClientId::from("DERIVE")),
        StrategyId::from("S-1"),
        InstrumentId::from("ETH-PERP.DERIVE"),
        None,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );
    tc.client.cancel_all_orders(cmd).expect("cancel_all Ok");

    wait_until(
        || {
            let state = ws_state.clone();
            async move { !state.cancel_by_instrument_calls.lock().await.is_empty() }
        },
        "cancel_by_instrument posted",
    )
    .await;

    let outcome = tokio::time::timeout(Duration::from_millis(200), async {
        loop {
            match tc.rx.recv().await {
                Some(ExecutionEvent::Order(event)) => return Some(event),
                Some(_) => {}
                None => return None,
            }
        }
    })
    .await;

    assert!(
        outcome.is_err(),
        "bulk failure has no per-order outcome to emit, was {outcome:?}",
    );
    assert!(ws_state.cancel_all_calls.lock().await.is_empty());

    tc.client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test]
async fn test_cancel_all_orders_buy_side_with_no_open_orders_is_noop() {
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    let mut tc = build_client(rest_state.clone(), ws_state.clone()).await;
    tc.client.connect().await.expect("connect succeeds");

    let cmd = CancelAllOrders::new(
        TraderId::from("TRADER-001"),
        Some(ClientId::from("DERIVE")),
        StrategyId::from("S-1"),
        InstrumentId::from("ETH-PERP.DERIVE"),
        Some(OrderSide::Buy),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );
    tc.client.cancel_all_orders(cmd).expect("cancel_all Ok");

    // Intentional quiet window: `wait_until_async` cannot prove absence of a future cancel.
    tokio::time::sleep(Duration::from_millis(100)).await;
    let cancels = ws_state.cancelled_orders.lock().await;
    assert!(
        cancels.is_empty(),
        "no cancels should be sent when open_orders is empty, saw {}",
        cancels.len(),
    );
    assert!(ws_state.cancelled_trigger_orders.lock().await.is_empty());
    assert!(ws_state.cancel_by_instrument_calls.lock().await.is_empty(),);
    assert!(rest_state.open_orders_calls.lock().await.is_empty());
    assert_eq!(rest_state.trigger_orders_calls.lock().await.len(), 2);
    let cancel_all = ws_state.cancel_all_calls.lock().await;
    assert!(
        cancel_all.is_empty(),
        "private/cancel_all must not be invoked for side-filtered command, saw {}",
        cancel_all.len(),
    );

    tc.client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test]
async fn test_query_order_unparsable_response_does_not_emit_report() {
    // query_order swallows deserialize failures so callers do not get a
    // partial / invalid OrderStatusReport. Use a response that the
    // DeriveOrder serde shape cannot parse.
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    *rest_state.get_order_response.lock().await = json!({});
    let mut tc = build_report_client(rest_state, ws_state).await;
    tc.client.connect().await.expect("connect succeeds");

    let cmd = QueryOrder::new(
        TraderId::from("TRADER-001"),
        Some(ClientId::from("DERIVE")),
        StrategyId::from("S-1"),
        InstrumentId::from("ETH-PERP.DERIVE"),
        ClientOrderId::from("STRAT-Q-UNK"),
        Some(VenueOrderId::from("ord-unknown-1")),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );
    tc.client.query_order(cmd).expect("query_order Ok");

    let outcome = tokio::time::timeout(Duration::from_millis(200), async {
        loop {
            match tc.rx.recv().await {
                Some(ExecutionEvent::Report(ExecutionReport::Order(_))) => {
                    return Some("unexpected OrderStatusReport");
                }
                Some(_) => {}
                None => return None,
            }
        }
    })
    .await;

    assert!(
        outcome.is_err(),
        "unparsable get_order response must not emit a report, was {outcome:?}",
    );

    tc.client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test]
async fn test_generate_order_status_reports_open_no_filter_returns_all_instruments() {
    // TC-E84: with `open_only=true` and no instrument filter, the
    // reconciler must see every open order the venue returns, regardless of
    // instrument. Caller-side filtering is the only knob for narrowing.
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    *rest_state.open_orders_response.lock().await = json!({
        "orders": [
            order_json_with("ord-eth-1", "L-ETH-1", "buy", "ETH-PERP", 100, "open"),
            order_json_with("ord-eth-2", "L-ETH-2", "sell", "ETH-PERP", 101, "open"),
            order_json_with("ord-btc-1", "L-BTC-1", "buy", "BTC-PERP", 102, "open"),
        ],
        "subaccount_id": TEST_SUBACCOUNT,
    });
    let mut tc = build_report_client(rest_state.clone(), ws_state).await;
    tc.client.connect().await.expect("connect succeeds");

    let cmd = GenerateOrderStatusReports::new(
        UUID4::new(),
        UnixNanos::default(),
        true,
        None,
        None,
        None,
        None,
        None,
    );
    let reports = tc
        .client
        .generate_order_status_reports(&cmd)
        .await
        .expect("reports");
    assert_eq!(reports.len(), 3);

    let mut by_voi: std::collections::HashMap<&str, &OrderStatusReport> =
        std::collections::HashMap::new();

    for r in &reports {
        by_voi.insert(r.venue_order_id.as_str(), r);
    }

    let eth1 = by_voi.get("ord-eth-1").expect("ord-eth-1 present");
    assert_eq!(
        eth1.client_order_id.map(|c| c.as_str().to_string()),
        Some("L-ETH-1".to_string()),
    );
    assert_eq!(eth1.instrument_id.symbol.as_str(), "ETH-PERP");
    assert_eq!(eth1.order_side, Some(OrderSide::Buy));

    let eth2 = by_voi.get("ord-eth-2").expect("ord-eth-2 present");
    assert_eq!(eth2.order_side, Some(OrderSide::Sell));

    let btc1 = by_voi.get("ord-btc-1").expect("ord-btc-1 present");
    assert_eq!(btc1.instrument_id.symbol.as_str(), "BTC-PERP");

    // History endpoint must NOT be touched: open_only routes via get_open_orders.
    assert!(rest_state.order_history_calls.lock().await.is_empty());

    tc.client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test]
async fn test_generate_position_status_reports_returns_long_short_and_flat() {
    // TC-E85: positions with mixed signs must round-trip with the correct
    // `position_side`. Flats are preserved (the reconciler decides how to
    // treat them; the adapter does not pre-filter).
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    *rest_state.positions_response.lock().await = json!({
        "positions": [
            sample_position_json("ETH-PERP", "3"),
            sample_position_json("BTC-PERP", "-1.5"),
            sample_position_json("SOL-PERP", "0"),
        ],
        "subaccount_id": TEST_SUBACCOUNT,
    });
    let mut tc = build_report_client(rest_state, ws_state).await;
    tc.client.connect().await.expect("connect succeeds");

    let cmd = GeneratePositionStatusReports::new(
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
        None,
        None,
        None,
    );
    let reports = tc
        .client
        .generate_position_status_reports(&cmd)
        .await
        .expect("positions");
    assert_eq!(reports.len(), 3);

    let by_symbol: std::collections::HashMap<&str, &PositionStatusReport> = reports
        .iter()
        .map(|r| (r.instrument_id.symbol.as_str(), r))
        .collect();

    let eth = by_symbol.get("ETH-PERP").expect("ETH-PERP present");
    assert_eq!(eth.position_side, PositionSide::Long);
    assert_eq!(eth.signed_decimal_qty, dec!(3));

    let btc = by_symbol.get("BTC-PERP").expect("BTC-PERP present");
    assert_eq!(btc.position_side, PositionSide::Short);
    assert_eq!(btc.signed_decimal_qty, dec!(-1.5));

    let sol = by_symbol.get("SOL-PERP").expect("SOL-PERP present");
    assert_eq!(sol.position_side, PositionSide::Flat);
    assert_eq!(sol.signed_decimal_qty, dec!(0));

    tc.client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test]
async fn test_generate_fill_reports_paginates_across_multiple_pages() {
    // TC-E86: the adapter walks `pagination.num_pages` and merges trades
    // across calls. Two pages with one trade each must produce two reports
    // and two GET calls.
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    *rest_state.trade_history_pages.lock().await = vec![
        json!({
            "trades": [sample_trade_json("trade-page-1", "ord-A", "ETH-PERP")],
            "pagination": {"count": 2, "num_pages": 2},
            "subaccount_id": TEST_SUBACCOUNT,
        }),
        json!({
            "trades": [sample_trade_json("trade-page-2", "ord-B", "ETH-PERP")],
            "pagination": {"count": 2, "num_pages": 2},
            "subaccount_id": TEST_SUBACCOUNT,
        }),
    ];
    let mut tc = build_report_client(rest_state.clone(), ws_state).await;
    tc.client.connect().await.expect("connect succeeds");

    let cmd = GenerateFillReports::new(
        UUID4::new(),
        UnixNanos::default(),
        Some(InstrumentId::from("ETH-PERP.DERIVE")),
        None,
        None,
        None,
        None,
        None,
    );
    let reports = tc.client.generate_fill_reports(cmd).await.expect("fills");

    let mut trade_ids: Vec<&str> = reports.iter().map(|r| r.trade_id.as_str()).collect();
    trade_ids.sort_unstable();
    assert_eq!(trade_ids, vec!["trade-page-1", "trade-page-2"]);

    let calls = rest_state.trade_history_calls.lock().await;
    assert_eq!(calls.len(), 2, "must request both pages");
    assert_eq!(calls[0]["page"].as_u64(), Some(1));
    assert_eq!(calls[0]["page_size"].as_u64(), Some(500));
    assert_eq!(calls[1]["page"].as_u64(), Some(2));
    assert_eq!(calls[1]["page_size"].as_u64(), Some(500));

    tc.client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test]
async fn test_submit_option_call_buy_limit_full_lifecycle() {
    // Group 10 / Option call buy: exercises the adapter against an
    // `instrument_type=option` instrument shape. The dispatch must accept
    // option instrument_id strings (`ETH-20260626-3500-C.DERIVE`) and walk
    // the same lifecycle as perps.
    let (mut tc, ws_state) =
        build_connected_option_client("ETH-20260626-3500-C", "C", "3500").await;
    drain_initial_account_state(&mut tc).await;

    let instrument_id = InstrumentId::from("ETH-20260626-3500-C.DERIVE");
    let client_order_id = ClientOrderId::from("STRAT-OPT-CALL-BUY");
    let order = build_limit_order(
        instrument_id,
        client_order_id,
        OrderSide::Buy,
        Price::from("100"),
        Quantity::from("1.00"),
    );
    submit_cached_order(&tc, &order);

    drain_order_submitted(&mut tc).await;
    wait_for_private_order_post(&ws_state).await;
    {
        let posts = ws_state.submitted_orders.lock().await;
        assert_eq!(
            posts[0]["instrument_name"].as_str(),
            Some("ETH-20260626-3500-C"),
        );
        assert_eq!(posts[0]["direction"].as_str(), Some("buy"));
        assert_eq!(posts[0]["order_type"].as_str(), Some("limit"));
    }

    let open_frame = json!([order_json_with(
        "ord-opt-call-1",
        client_order_id.as_str(),
        "buy",
        "ETH-20260626-3500-C",
        1_700_000_001_000_i64,
        "open",
    )]);
    push_orders_update(&ws_state, &open_frame);

    let accepted = drain_until(
        &mut tc.rx,
        |e| matches!(e, ExecutionEvent::Order(OrderEventAny::Accepted(_))),
        "OrderAccepted on option Open",
    )
    .await;

    if let ExecutionEvent::Order(OrderEventAny::Accepted(accepted)) = accepted {
        assert_eq!(accepted.client_order_id, client_order_id);
        assert_eq!(accepted.instrument_id, instrument_id);
    } else {
        unreachable!();
    }

    let trade_frame = json!([trade_json_with_label(
        "trade-opt-call-1",
        "ord-opt-call-1",
        "ETH-20260626-3500-C",
        client_order_id.as_str(),
    )]);
    push_trades_update(&ws_state, &trade_frame);

    let filled = drain_until(
        &mut tc.rx,
        |e| matches!(e, ExecutionEvent::Order(OrderEventAny::Filled(_))),
        "OrderFilled on option .trades",
    )
    .await;

    if let ExecutionEvent::Order(OrderEventAny::Filled(filled)) = filled {
        assert_eq!(filled.client_order_id, client_order_id);
        assert_eq!(filled.trade_id.as_str(), "trade-opt-call-1");
        assert_eq!(filled.order_side, OrderSide::Buy);
    } else {
        unreachable!();
    }

    tc.client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test]
async fn test_submit_option_put_sell_limit_full_lifecycle() {
    // Group 10 / Option put sell: mirror of the call-buy test on a put
    // instrument (`-P` suffix, `option_type=P`).
    let (mut tc, ws_state) =
        build_connected_option_client("ETH-20260626-3500-P", "P", "3500").await;
    drain_initial_account_state(&mut tc).await;

    let instrument_id = InstrumentId::from("ETH-20260626-3500-P.DERIVE");
    let client_order_id = ClientOrderId::from("STRAT-OPT-PUT-SELL");
    let order = build_limit_order(
        instrument_id,
        client_order_id,
        OrderSide::Sell,
        Price::from("80"),
        Quantity::from("0.50"),
    );
    submit_cached_order(&tc, &order);

    drain_order_submitted(&mut tc).await;
    wait_for_private_order_post(&ws_state).await;
    {
        let posts = ws_state.submitted_orders.lock().await;
        assert_eq!(
            posts[0]["instrument_name"].as_str(),
            Some("ETH-20260626-3500-P"),
        );
        assert_eq!(posts[0]["direction"].as_str(), Some("sell"));
    }

    let open_frame = json!([order_json_with(
        "ord-opt-put-1",
        client_order_id.as_str(),
        "sell",
        "ETH-20260626-3500-P",
        1_700_000_001_000_i64,
        "open",
    )]);
    push_orders_update(&ws_state, &open_frame);

    let _ = drain_until(
        &mut tc.rx,
        |e| matches!(e, ExecutionEvent::Order(OrderEventAny::Accepted(_))),
        "OrderAccepted on option put Open",
    )
    .await;

    let trade_frame = json!([{
        "direction": "sell",
        "index_price": "3500",
        "instrument_name": "ETH-20260626-3500-P",
        "is_transfer": false,
        "label": client_order_id.as_str(),
        "liquidity_role": "taker",
        "mark_price": "80",
        "order_id": "ord-opt-put-1",
        "quote_id": null,
        "realized_pnl": "0",
        "subaccount_id": TEST_SUBACCOUNT,
        "timestamp": 1_700_000_002_000_i64,
        "trade_amount": "0.5",
        "trade_fee": "0.1",
        "trade_id": "trade-opt-put-1",
        "trade_price": "80",
        "tx_hash": "0xabc",
        "batch_status": "Settled",
        "wallet": "0xwallet",
    }]);
    push_trades_update(&ws_state, &trade_frame);

    let filled = drain_until(
        &mut tc.rx,
        |e| matches!(e, ExecutionEvent::Order(OrderEventAny::Filled(_))),
        "OrderFilled on option put .trades",
    )
    .await;

    if let ExecutionEvent::Order(OrderEventAny::Filled(filled)) = filled {
        assert_eq!(filled.client_order_id, client_order_id);
        assert_eq!(filled.trade_id.as_str(), "trade-opt-put-1");
        assert_eq!(filled.order_side, OrderSide::Sell);
    } else {
        unreachable!();
    }

    tc.client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test]
async fn test_submit_option_order_resolves_option_instrument_for_signing() {
    // Group 10 / signing: the adapter resolves the option-specific
    // instrument record (option_details, base_asset_sub_id, tick_size=1)
    // when submitting an option order. We assert the get_instrument
    // request carries the option name (so the right asset is used for the
    // EIP-712 trade-module signing payload) and that the POST body uses
    // the same name.
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    *rest_state.get_instrument_response.lock().await =
        option_instrument_json("ETH-20260626-3500-C", "C", "3500");
    let mut tc = build_client(rest_state.clone(), ws_state.clone()).await;
    tc.client.connect().await.expect("connect");
    wait_for_private_subscription(&ws_state).await;

    let instrument_id = InstrumentId::from("ETH-20260626-3500-C.DERIVE");
    let client_order_id = ClientOrderId::from("STRAT-OPT-SIGN");
    let order = build_limit_order(
        instrument_id,
        client_order_id,
        OrderSide::Buy,
        Price::from("100"),
        Quantity::from("1.00"),
    );
    submit_cached_order(&tc, &order);

    wait_for_private_order_post(&ws_state).await;

    let instrument_calls = rest_state.get_instrument_calls.lock().await;
    assert_eq!(instrument_calls.len(), 1);
    assert_eq!(
        instrument_calls[0]["instrument_name"].as_str(),
        Some("ETH-20260626-3500-C"),
    );
    drop(instrument_calls);

    let posts = ws_state.submitted_orders.lock().await;
    let body = &posts[0];
    assert_eq!(
        body["instrument_name"].as_str(),
        Some("ETH-20260626-3500-C"),
    );
    assert_eq!(body["direction"].as_str(), Some("buy"));
    assert_eq!(body["order_type"].as_str(), Some("limit"));
    assert_eq!(body["label"].as_str(), Some("STRAT-OPT-SIGN"));
    // The signed payload carries a non-empty signature and a nonce; the
    // venue-side verification would fail if asset_address / sub_id from
    // the option record were not used.
    assert!(body["signature"].as_str().unwrap().starts_with("0x"));
    assert!(body["nonce"].as_str().unwrap().parse::<u64>().unwrap() > 1_700_000_000_000_000_000);

    tc.client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test]
async fn test_submit_option_fok_limit_posts_fok_and_emits_filled() {
    // TC-E99: option FOK limit orders use the ordinary option signing path
    // but must preserve the FOK instruction sent to the venue.
    let (mut tc, ws_state) =
        build_connected_option_client("ETH-20260626-3500-C", "C", "3500").await;
    drain_initial_account_state(&mut tc).await;

    let instrument_id = InstrumentId::from("ETH-20260626-3500-C.DERIVE");
    let client_order_id = ClientOrderId::from("STRAT-OPT-FOK");
    let order = build_limit_order_with_time_in_force(
        instrument_id,
        client_order_id,
        OrderSide::Buy,
        Price::from("100"),
        Quantity::from("1.00"),
        TimeInForce::Fok,
        false,
    );
    submit_cached_order(&tc, &order);

    drain_order_submitted(&mut tc).await;
    wait_for_private_order_post(&ws_state).await;
    {
        let posts = ws_state.submitted_orders.lock().await;
        assert_eq!(
            posts[0]["instrument_name"].as_str(),
            Some("ETH-20260626-3500-C"),
        );
        assert_eq!(posts[0]["time_in_force"].as_str(), Some("fok"));
        assert_eq!(posts[0]["order_type"].as_str(), Some("limit"));
    }

    let open_frame = json!([order_json_with(
        "ord-opt-fok-1",
        client_order_id.as_str(),
        "buy",
        "ETH-20260626-3500-C",
        1_700_000_001_000_i64,
        "open",
    )]);
    push_orders_update(&ws_state, &open_frame);

    let _ = drain_until(
        &mut tc.rx,
        |e| matches!(e, ExecutionEvent::Order(OrderEventAny::Accepted(_))),
        "OrderAccepted on option FOK Open",
    )
    .await;

    let trade_frame = json!([trade_json_with_label(
        "trade-opt-fok-1",
        "ord-opt-fok-1",
        "ETH-20260626-3500-C",
        client_order_id.as_str(),
    )]);
    push_trades_update(&ws_state, &trade_frame);

    let filled = drain_until(
        &mut tc.rx,
        |e| matches!(e, ExecutionEvent::Order(OrderEventAny::Filled(_))),
        "OrderFilled on option FOK .trades",
    )
    .await;

    if let ExecutionEvent::Order(OrderEventAny::Filled(filled)) = filled {
        assert_eq!(filled.client_order_id, client_order_id);
        assert_eq!(filled.venue_order_id.as_str(), "ord-opt-fok-1");
        assert_eq!(filled.trade_id.as_str(), "trade-opt-fok-1");
        assert_eq!(filled.order_side, OrderSide::Buy);
        assert_eq!(filled.last_qty.as_decimal(), dec!(1));
    } else {
        unreachable!();
    }

    tc.client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test]
async fn test_cancel_option_order_posts_private_cancel_and_emits_canceled() {
    // TC-E100: option cancels must keep the option symbol on private/cancel
    // and route the terminal `.orders` update back to the tracked order.
    let (mut tc, ws_state) =
        build_connected_option_client("ETH-20260626-3500-C", "C", "3500").await;
    drain_initial_account_state(&mut tc).await;

    let instrument_id = InstrumentId::from("ETH-20260626-3500-C.DERIVE");
    let client_order_id = ClientOrderId::from("STRAT-OPT-CANCEL");
    let venue_order_id = VenueOrderId::from("ord-opt-cancel-1");
    let order = build_limit_order(
        instrument_id,
        client_order_id,
        OrderSide::Buy,
        Price::from("100"),
        Quantity::from("1.00"),
    );
    submit_cached_order(&tc, &order);

    drain_order_submitted(&mut tc).await;
    wait_for_private_order_post(&ws_state).await;

    let open_frame = json!([order_json_with(
        venue_order_id.as_str(),
        client_order_id.as_str(),
        "buy",
        "ETH-20260626-3500-C",
        1_700_000_001_000_i64,
        "open",
    )]);
    push_orders_update(&ws_state, &open_frame);

    let _ = drain_until(
        &mut tc.rx,
        |e| matches!(e, ExecutionEvent::Order(OrderEventAny::Accepted(_))),
        "OrderAccepted on option Open",
    )
    .await;

    let cancel = CancelOrder::new(
        TraderId::from("TRADER-001"),
        Some(ClientId::from("DERIVE")),
        StrategyId::from("S-1"),
        instrument_id,
        client_order_id,
        Some(venue_order_id),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );
    tc.client.cancel_order(cancel).expect("cancel_order Ok");

    wait_until(
        || {
            let state = ws_state.clone();
            async move { !state.cancelled_orders.lock().await.is_empty() }
        },
        "private/cancel posted",
    )
    .await;

    {
        let posts = ws_state.cancelled_orders.lock().await;
        assert_eq!(
            posts[0]["instrument_name"].as_str(),
            Some("ETH-20260626-3500-C"),
        );
        assert_eq!(posts[0]["order_id"].as_str(), Some("ord-opt-cancel-1"));
        assert!(ws_state.cancelled_trigger_orders.lock().await.is_empty());
    }

    let cancel_frame = json!([order_json_with(
        "ord-opt-cancel-1",
        client_order_id.as_str(),
        "buy",
        "ETH-20260626-3500-C",
        1_700_000_002_000_i64,
        "cancelled",
    )]);
    push_orders_update(&ws_state, &cancel_frame);

    let event = drain_until(
        &mut tc.rx,
        |e| matches!(e, ExecutionEvent::Order(OrderEventAny::Canceled(_))),
        "OrderCanceled on option cancel",
    )
    .await;

    if let ExecutionEvent::Order(OrderEventAny::Canceled(canceled)) = event {
        assert_eq!(canceled.client_order_id, client_order_id);
        assert_eq!(
            canceled.venue_order_id.map(|v| v.as_str().to_string()),
            Some("ord-opt-cancel-1".to_string()),
        );
    } else {
        unreachable!();
    }

    tc.client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test]
async fn test_generate_position_status_reports_option_position() {
    // TC-E101: option positions use the same reconciliation report path, but
    // the instrument id must preserve the full option symbol.
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    let mut option_position = sample_position_json("ETH-20260626-3500-C", "-2");
    option_position["instrument_type"] = json!("option");
    option_position["average_price"] = json!("80");
    *rest_state.positions_response.lock().await = json!({
        "positions": [option_position],
        "subaccount_id": TEST_SUBACCOUNT,
    });
    let mut tc = build_report_client(rest_state, ws_state).await;
    tc.client.connect().await.expect("connect succeeds");

    let cmd = GeneratePositionStatusReports::new(
        UUID4::new(),
        UnixNanos::default(),
        Some(InstrumentId::from("ETH-20260626-3500-C.DERIVE")),
        None,
        None,
        None,
        None,
    );
    let reports = tc
        .client
        .generate_position_status_reports(&cmd)
        .await
        .expect("positions");

    assert_eq!(reports.len(), 1);
    assert_eq!(
        reports[0].instrument_id.symbol.as_str(),
        "ETH-20260626-3500-C"
    );
    assert_eq!(reports[0].position_side, PositionSide::Short);
    assert_eq!(reports[0].signed_decimal_qty, dec!(-2));
    assert_eq!(reports[0].avg_px_open, Some(dec!(80)));

    tc.client.disconnect().await.expect("disconnect");
}

async fn build_connected_option_client(
    instrument_name: &str,
    option_type: &str,
    strike: &str,
) -> (TestClient, WsState) {
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    *rest_state.get_instrument_response.lock().await =
        option_instrument_json(instrument_name, option_type, strike);

    let mut tc = build_client(rest_state, ws_state.clone()).await;
    tc.client.connect().await.expect("connect");
    wait_for_private_subscription(&ws_state).await;

    (tc, ws_state)
}

async fn wait_for_private_subscription(ws_state: &WsState) {
    wait_until(
        || {
            let state = (*ws_state).clone();
            async move { !state.subscribe_frames.lock().await.is_empty() }
        },
        "subscribe acknowledged",
    )
    .await;
}

async fn drain_initial_account_state(tc: &mut TestClient) {
    for _ in 0..2 {
        let _ = drain_until(
            &mut tc.rx,
            |e| matches!(e, ExecutionEvent::Account(_)),
            "initial AccountState",
        )
        .await;
    }
}

fn submit_cached_order(tc: &TestClient, order: &OrderAny) {
    tc.cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .expect("cache insert");
    tc.client
        .submit_order(submit_cmd(order))
        .expect("submit Ok");
}

async fn drain_order_submitted(tc: &mut TestClient) {
    let _ = drain_until(
        &mut tc.rx,
        |e| matches!(e, ExecutionEvent::Order(OrderEventAny::Submitted(_))),
        "OrderSubmitted",
    )
    .await;
}

async fn wait_for_private_order_post(ws_state: &WsState) {
    wait_until(
        || {
            let state = (*ws_state).clone();
            async move { !state.submitted_orders.lock().await.is_empty() }
        },
        "private/order posted",
    )
    .await;
}

fn push_orders_update(ws_state: &WsState, data: &Value) {
    let channel = format!("{TEST_SUBACCOUNT}.orders");
    ws_state.push_notification(make_subscription_frame(&channel, data));
}

fn push_trades_update(ws_state: &WsState, data: &Value) {
    let channel = format!("{TEST_SUBACCOUNT}.trades");
    ws_state.push_notification(make_subscription_frame(&channel, data));
}

#[rstest]
#[tokio::test]
async fn test_second_submit_resolves_instrument_from_cache_without_refetch() {
    // The instrument cache is keyed by InstrumentId over an AtomicMap: once an
    // order resolves an instrument via public/get_instrument, a later order for
    // the same instrument must be served from the cache rather than re-fetched.
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    let mut tc = build_client(rest_state.clone(), ws_state.clone()).await;
    tc.client.connect().await.expect("connect");

    let instrument_id = InstrumentId::from("ETH-PERP.DERIVE");

    let first = build_limit_order(
        instrument_id,
        ClientOrderId::from("STRAT-CACHE-1"),
        OrderSide::Buy,
        Price::from("100"),
        Quantity::from("1.0"),
    );
    tc.cache
        .borrow_mut()
        .add_order(first.clone(), None, None, false)
        .expect("cache insert");
    tc.client
        .submit_order(submit_cmd(&first))
        .expect("submit Ok");

    // Insert precedes the signed POST, so once the first order is on the wire
    // the cache is populated and the second submit cannot race the fetch.
    wait_until(
        || {
            let state = ws_state.clone();
            async move { state.submitted_orders.lock().await.len() == 1 }
        },
        "first private/order posted",
    )
    .await;

    let second = build_limit_order(
        instrument_id,
        ClientOrderId::from("STRAT-CACHE-2"),
        OrderSide::Buy,
        Price::from("100"),
        Quantity::from("1.0"),
    );
    tc.cache
        .borrow_mut()
        .add_order(second.clone(), None, None, false)
        .expect("cache insert");
    tc.client
        .submit_order(submit_cmd(&second))
        .expect("submit Ok");

    wait_until(
        || {
            let state = ws_state.clone();
            async move { state.submitted_orders.lock().await.len() == 2 }
        },
        "second private/order posted",
    )
    .await;

    assert_eq!(rest_state.get_instrument_calls.lock().await.len(), 1);

    tc.client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test]
async fn test_submit_market_sell_full_lifecycle_emits_accepted_then_filled() {
    // TC-E02: market SELL mirror of TC-E01. Verifies the dispatch path is
    // side-agnostic and that `OrderFilled.order_side` is taken from the
    // tracked identity (Sell) rather than the trade frame.
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    let mut tc = build_client(rest_state, ws_state.clone()).await;
    tc.client.connect().await.expect("connect");

    wait_until(
        || {
            let state = ws_state.clone();
            async move { !state.subscribe_frames.lock().await.is_empty() }
        },
        "subscribe acknowledged",
    )
    .await;

    let _ = drain_until(
        &mut tc.rx,
        |e| matches!(e, ExecutionEvent::Account(_)),
        "initial AccountState",
    )
    .await;

    let instrument_id = InstrumentId::from("ETH-PERP.DERIVE");
    let client_order_id = ClientOrderId::from("STRAT-MKT-SELL-E02");

    let quote = QuoteTick::new(
        instrument_id,
        Price::from("3500.00"),
        Price::from("3501.00"),
        Quantity::from("1.000"),
        Quantity::from("1.000"),
        UnixNanos::default(),
        UnixNanos::default(),
    );
    tc.cache
        .borrow_mut()
        .add_quote(quote)
        .expect("quote insert");

    let order = build_market_order(
        instrument_id,
        client_order_id,
        OrderSide::Sell,
        Quantity::from("1.000"),
    );
    tc.cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .expect("cache insert");

    tc.client
        .submit_order(submit_cmd(&order))
        .expect("submit Ok");

    let _ = drain_until(
        &mut tc.rx,
        |e| matches!(e, ExecutionEvent::Order(OrderEventAny::Submitted(_))),
        "OrderSubmitted",
    )
    .await;
    wait_until(
        || {
            let state = ws_state.clone();
            async move { !state.submitted_orders.lock().await.is_empty() }
        },
        "private/order posted",
    )
    .await;

    {
        let posts = ws_state.submitted_orders.lock().await;
        assert_eq!(posts[0]["direction"].as_str(), Some("sell"));
        assert_eq!(posts[0]["order_type"].as_str(), Some("market"));
    }

    let orders_channel = format!("{TEST_SUBACCOUNT}.orders");
    let open_frame = json!([order_json_with(
        "ord-mkt-sell-1",
        client_order_id.as_str(),
        "sell",
        "ETH-PERP",
        1_700_000_001_000_i64,
        "open",
    )]);
    ws_state.push_notification(make_subscription_frame(&orders_channel, &open_frame));

    let accepted = drain_until(
        &mut tc.rx,
        |e| matches!(e, ExecutionEvent::Order(OrderEventAny::Accepted(_))),
        "OrderAccepted on .orders Open",
    )
    .await;

    if let ExecutionEvent::Order(OrderEventAny::Accepted(accepted)) = accepted {
        assert_eq!(accepted.client_order_id, client_order_id);
        assert_eq!(accepted.venue_order_id.as_str(), "ord-mkt-sell-1");
        assert_eq!(accepted.instrument_id, instrument_id);
    } else {
        unreachable!();
    }

    // Inline rather than `trade_json_with_label` so the frame's direction
    // matches the order side. Identity drives OrderFilled.order_side, but
    // keeping the frame realistic avoids confusion in regression diffs.
    let trades_channel = format!("{TEST_SUBACCOUNT}.trades");
    let trade_frame = json!([{
        "direction": "sell",
        "index_price": "3500",
        "instrument_name": "ETH-PERP",
        "is_transfer": false,
        "label": client_order_id.as_str(),
        "liquidity_role": "taker",
        "mark_price": "3500",
        "order_id": "ord-mkt-sell-1",
        "quote_id": null,
        "realized_pnl": "0",
        "subaccount_id": TEST_SUBACCOUNT,
        "timestamp": 1_700_000_002_000_i64,
        "trade_amount": "1",
        "trade_fee": "0.5",
        "trade_id": "trade-mkt-sell-1",
        "trade_price": "3495",
        "tx_hash": "0xabc",
        "batch_status": "Settled",
        "wallet": "0xwallet",
    }]);
    ws_state.push_notification(make_subscription_frame(&trades_channel, &trade_frame));

    let filled = drain_until(
        &mut tc.rx,
        |e| matches!(e, ExecutionEvent::Order(OrderEventAny::Filled(_))),
        "OrderFilled on .trades",
    )
    .await;

    if let ExecutionEvent::Order(OrderEventAny::Filled(filled)) = filled {
        assert_eq!(filled.client_order_id, client_order_id);
        assert_eq!(filled.venue_order_id.as_str(), "ord-mkt-sell-1");
        assert_eq!(filled.trade_id.as_str(), "trade-mkt-sell-1");
        assert_eq!(filled.order_side, OrderSide::Sell);
        assert_eq!(filled.last_qty.as_decimal(), dec!(1));
        assert_eq!(filled.last_px.as_decimal(), dec!(3495));
    } else {
        unreachable!();
    }

    let filled_frame = json!([order_json_with(
        "ord-mkt-sell-1",
        client_order_id.as_str(),
        "sell",
        "ETH-PERP",
        1_700_000_003_000_i64,
        "filled",
    )]);
    ws_state.push_notification(make_subscription_frame(&orders_channel, &filled_frame));

    let outcome = tokio::time::timeout(Duration::from_millis(200), async {
        loop {
            match tc.rx.recv().await {
                Some(ExecutionEvent::Order(OrderEventAny::Accepted(_))) => {
                    return Some("duplicate Accepted after fill");
                }
                Some(ExecutionEvent::Order(OrderEventAny::Filled(_))) => {
                    return Some("duplicate Filled after fill");
                }
                Some(ExecutionEvent::Report(_)) => {
                    return Some("fallback report after tracked fill");
                }
                Some(_) => {}
                None => return None,
            }
        }
    })
    .await;

    assert!(
        outcome.is_err(),
        "trailing .orders Filled must be a no-op, was {outcome:?}",
    );

    tc.client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test]
async fn test_submit_spot_buy_limit_full_lifecycle() {
    // Spot (ERC-20) buy: exercises the adapter against an
    // `instrument_type=erc20` instrument (`ETH-USDC`). Spot reuses the Trade
    // module signing path, so submit/open/fill must walk the same lifecycle
    // as perps and options with no execution-side branch.
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    *rest_state.get_instrument_response.lock().await = spot_instrument_json("ETH-USDC");
    let mut tc = build_client(rest_state, ws_state.clone()).await;
    tc.client.connect().await.expect("connect");

    wait_until(
        || {
            let state = ws_state.clone();
            async move { !state.subscribe_frames.lock().await.is_empty() }
        },
        "subscribe acknowledged",
    )
    .await;

    let _ = drain_until(
        &mut tc.rx,
        |e| matches!(e, ExecutionEvent::Account(_)),
        "initial AccountState",
    )
    .await;

    let instrument_id = InstrumentId::from("ETH-USDC.DERIVE");
    let client_order_id = ClientOrderId::from("STRAT-SPOT-BUY");
    let order = build_limit_order(
        instrument_id,
        client_order_id,
        OrderSide::Buy,
        Price::from("2000.0"),
        Quantity::from("0.10"),
    );
    tc.cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .expect("cache insert");
    tc.client
        .submit_order(submit_cmd(&order))
        .expect("submit Ok");

    let _ = drain_until(
        &mut tc.rx,
        |e| matches!(e, ExecutionEvent::Order(OrderEventAny::Submitted(_))),
        "OrderSubmitted",
    )
    .await;
    wait_until(
        || {
            let state = ws_state.clone();
            async move { !state.submitted_orders.lock().await.is_empty() }
        },
        "private/order posted",
    )
    .await;

    {
        let posts = ws_state.submitted_orders.lock().await;
        assert_eq!(posts[0]["instrument_name"].as_str(), Some("ETH-USDC"));
        assert_eq!(posts[0]["direction"].as_str(), Some("buy"));
        assert_eq!(posts[0]["order_type"].as_str(), Some("limit"));
        // reduce_only must be absent: this is a plain spot open.
        assert!(posts[0].get("reduce_only").is_none());
    }

    let orders_channel = format!("{TEST_SUBACCOUNT}.orders");
    let open_frame = json!([order_json_with(
        "ord-spot-1",
        client_order_id.as_str(),
        "buy",
        "ETH-USDC",
        1_700_000_001_000_i64,
        "open",
    )]);
    ws_state.push_notification(make_subscription_frame(&orders_channel, &open_frame));

    let accepted = drain_until(
        &mut tc.rx,
        |e| matches!(e, ExecutionEvent::Order(OrderEventAny::Accepted(_))),
        "OrderAccepted on spot Open",
    )
    .await;

    if let ExecutionEvent::Order(OrderEventAny::Accepted(accepted)) = accepted {
        assert_eq!(accepted.client_order_id, client_order_id);
        assert_eq!(accepted.instrument_id, instrument_id);
        assert_eq!(accepted.venue_order_id.as_str(), "ord-spot-1");
    } else {
        unreachable!();
    }

    let trades_channel = format!("{TEST_SUBACCOUNT}.trades");
    let trade_frame = json!([{
        "direction": "buy",
        "index_price": "2000",
        "instrument_name": "ETH-USDC",
        "is_transfer": false,
        "label": client_order_id.as_str(),
        "liquidity_role": "taker",
        "mark_price": "2000",
        "order_id": "ord-spot-1",
        "quote_id": null,
        "realized_pnl": "0",
        "subaccount_id": TEST_SUBACCOUNT,
        "timestamp": 1_700_000_002_000_i64,
        "trade_amount": "0.1",
        "trade_fee": "0",
        "trade_id": "trade-spot-1",
        "trade_price": "2000",
        "tx_hash": "0xabc",
        "batch_status": "Settled",
        "wallet": "0xwallet",
    }]);
    ws_state.push_notification(make_subscription_frame(&trades_channel, &trade_frame));

    let filled = drain_until(
        &mut tc.rx,
        |e| matches!(e, ExecutionEvent::Order(OrderEventAny::Filled(_))),
        "OrderFilled on spot .trades",
    )
    .await;

    if let ExecutionEvent::Order(OrderEventAny::Filled(filled)) = filled {
        assert_eq!(filled.client_order_id, client_order_id);
        assert_eq!(filled.trade_id.as_str(), "trade-spot-1");
        assert_eq!(filled.order_side, OrderSide::Buy);
        assert_eq!(filled.last_qty.as_decimal(), dec!(0.1));
        assert_eq!(filled.last_px.as_decimal(), dec!(2000));
    } else {
        unreachable!();
    }

    tc.client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test]
async fn test_submit_spot_reduce_only_is_denied_locally() {
    // Derive spot has no position concept, so reduce-only can never reduce
    // anything; the venue rejects it unconditionally (11025). The adapter
    // short-circuits that deterministic outcome with a local OrderDenied and
    // never posts to the venue. Perp/option reduce-only is untouched.
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    let mut tc = build_client(rest_state, ws_state.clone()).await;
    tc.client.connect().await.expect("connect");

    let _ = drain_until(
        &mut tc.rx,
        |e| matches!(e, ExecutionEvent::Account(_)),
        "initial AccountState",
    )
    .await;

    // The guard reads the engine cache to classify the instrument as spot
    // (CurrencyPair), so register the parsed ETH-USDC record first.
    let derive_instrument: DeriveInstrument =
        serde_json::from_value(spot_instrument_json("ETH-USDC")).expect("spot instrument parses");
    let instrument = parse_derive_instrument_any(&derive_instrument, UnixNanos::default())
        .expect("parse succeeds")
        .expect("spot instrument produced");
    tc.cache
        .borrow_mut()
        .add_instrument(instrument)
        .expect("instrument insert");

    let instrument_id = InstrumentId::from("ETH-USDC.DERIVE");
    let client_order_id = ClientOrderId::from("STRAT-SPOT-RO");
    let order = build_reduce_only_limit_order(
        instrument_id,
        client_order_id,
        OrderSide::Sell,
        Price::from("2000.0"),
        Quantity::from("0.10"),
    );
    tc.cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .expect("cache insert");
    tc.client
        .submit_order(submit_cmd(&order))
        .expect("submit Ok");

    let event = drain_until(
        &mut tc.rx,
        |e| matches!(e, ExecutionEvent::Order(OrderEventAny::Denied(_))),
        "OrderDenied for spot reduce-only",
    )
    .await;

    if let ExecutionEvent::Order(OrderEventAny::Denied(denied)) = event {
        assert_eq!(denied.client_order_id, client_order_id);
        assert_eq!(denied.reason.as_str(), "UNSUPPORTED_REDUCE_ONLY");
    } else {
        unreachable!();
    }

    // The order must never reach the venue.
    assert!(ws_state.submitted_orders.lock().await.is_empty());

    tc.client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test]
async fn test_submit_perp_reduce_only_reaches_venue() {
    // The other half of the spot guard's invariant: reduce-only on a
    // derivative (perp) must NOT be blocked locally. The venue's perp
    // reduce-only rejection is conditional on position state, so the order
    // must reach `/private/order` with `reduce_only: true` and emit no local
    // Denied/Rejected. Guards against the guard over-matching all instruments.
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    // Default get_instrument returns the ETH-PERP perp record.
    let mut tc = build_client(rest_state, ws_state.clone()).await;
    tc.client.connect().await.expect("connect");

    let _ = drain_until(
        &mut tc.rx,
        |e| matches!(e, ExecutionEvent::Account(_)),
        "initial AccountState",
    )
    .await;

    let instrument_id = InstrumentId::from("ETH-PERP.DERIVE");
    let client_order_id = ClientOrderId::from("STRAT-PERP-RO");
    let order = build_reduce_only_limit_order(
        instrument_id,
        client_order_id,
        OrderSide::Sell,
        Price::from("3500.00"),
        Quantity::from("1.000"),
    );
    tc.cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .expect("cache insert");
    tc.client
        .submit_order(submit_cmd(&order))
        .expect("submit Ok");

    wait_until(
        || {
            let state = ws_state.clone();
            async move { !state.submitted_orders.lock().await.is_empty() }
        },
        "private/order posted for reduce-only perp",
    )
    .await;

    {
        let posts = ws_state.submitted_orders.lock().await;
        assert_eq!(posts[0]["instrument_name"].as_str(), Some("ETH-PERP"));
        assert_eq!(posts[0]["reduce_only"].as_bool(), Some(true));
    }

    // No local Denied/Rejected should have been emitted for the perp.
    let blocked = tokio::time::timeout(Duration::from_millis(200), async {
        loop {
            match tc.rx.recv().await {
                Some(ExecutionEvent::Order(OrderEventAny::Denied(_))) => return Some("denied"),
                Some(ExecutionEvent::Order(OrderEventAny::Rejected(_))) => return Some("rejected"),
                Some(_) => {}
                None => return None,
            }
        }
    })
    .await;

    assert!(
        blocked.is_err(),
        "reduce-only perp must not be blocked locally, was {blocked:?}",
    );

    tc.client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test]
async fn test_submit_spot_reduce_only_lazy_resolution_is_rejected() {
    // Same invariant as the deny test, but via the lazy instrument path: the
    // core cache is empty at submit time, so the synchronous deny is skipped
    // and the order resolves through `public/get_instrument`. The in-task net
    // must still keep the reduce-only spot order off the venue. OrderSubmitted
    // has already fired, so this surfaces as OrderRejected rather than Denied.
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    *rest_state.get_instrument_response.lock().await = spot_instrument_json("ETH-USDC");
    let mut tc = build_client(rest_state, ws_state.clone()).await;
    tc.client.connect().await.expect("connect");

    let _ = drain_until(
        &mut tc.rx,
        |e| matches!(e, ExecutionEvent::Account(_)),
        "initial AccountState",
    )
    .await;

    let instrument_id = InstrumentId::from("ETH-USDC.DERIVE");
    let client_order_id = ClientOrderId::from("STRAT-SPOT-RO-LAZY");
    let order = build_reduce_only_limit_order(
        instrument_id,
        client_order_id,
        OrderSide::Sell,
        Price::from("2000.0"),
        Quantity::from("0.10"),
    );
    tc.cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .expect("cache insert");
    tc.client
        .submit_order(submit_cmd(&order))
        .expect("submit Ok");

    let event = drain_until(
        &mut tc.rx,
        |e| matches!(e, ExecutionEvent::Order(OrderEventAny::Rejected(_))),
        "OrderRejected for lazily-resolved spot reduce-only",
    )
    .await;

    if let ExecutionEvent::Order(OrderEventAny::Rejected(rejected)) = event {
        assert_eq!(rejected.client_order_id, client_order_id);
        assert!(
            rejected.reason.contains("reduce-only"),
            "unexpected reject reason: {}",
            rejected.reason,
        );
    } else {
        unreachable!();
    }

    // The order must never reach the venue.
    assert!(ws_state.submitted_orders.lock().await.is_empty());

    tc.client.disconnect().await.expect("disconnect");
}

async fn query_order_report(
    tc: &mut TestClient,
    cmd: &GenerateOrderStatusReport,
) -> OrderStatusReport {
    tc.client
        .query_order(QueryOrder::new(
            TraderId::from("TRADER-001"),
            Some(ClientId::from("DERIVE")),
            StrategyId::from("S-1"),
            cmd.instrument_id.unwrap(),
            cmd.client_order_id.unwrap(),
            None,
            cmd.command_id,
            cmd.ts_init,
            None,
            None,
        ))
        .expect("query_order succeeds");

    let event = drain_until(
        &mut tc.rx,
        |event| matches!(event, ExecutionEvent::Report(ExecutionReport::Order(_))),
        "OrderStatusReport event",
    )
    .await;

    let ExecutionEvent::Report(ExecutionReport::Order(report)) = event else {
        unreachable!();
    };

    *report
}

#[rstest]
#[case("0", None, "1.5", false)]
#[case("0.3", None, "1.2", false)]
#[case("0.3", Some("0.2"), "1.0", false)]
#[case::restored_alias_without_rest_link("0.3", Some("0.2"), "1.0", true)]
#[case("0.300000000000000000", None, "1.2", false)]
#[tokio::test]
async fn test_modify_order_signs_native_remaining_amount_with_fresh_filled_guard(
    #[case] current_filled: &str,
    #[case] ancestor_filled: Option<&str>,
    #[case] expected_child_amount: &str,
    #[case] restored_alias: bool,
) {
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    let instrument_id = InstrumentId::from("ETH-PERP.DERIVE");
    let cid = ClientOrderId::from("NATIVE-REPLACE-AMOUNTS");
    let old_id = VenueOrderId::from("native-current-leg");
    let mut current = order_json_with(
        old_id.as_str(),
        cid.as_str(),
        "buy",
        "ETH-PERP",
        1_700_000_001_000,
        "open",
    );
    current["last_update_timestamp"] = json!(1_700_000_002_000_i64);
    current["amount"] = json!("1.000");
    current["filled_amount"] = json!(current_filled);

    if let Some(filled) = ancestor_filled {
        if !restored_alias {
            current["replaced_order_id"] = json!("native-ancestor-leg");
        }

        let mut ancestor = order_json_with(
            "native-ancestor-leg",
            cid.as_str(),
            "buy",
            "ETH-PERP",
            1_700_000_000_000,
            "cancelled",
        );
        ancestor["filled_amount"] = json!(filled);
        rest_state
            .get_order_responses
            .lock()
            .await
            .insert("native-ancestor-leg".to_string(), ancestor);
    }

    rest_state
        .get_order_responses
        .lock()
        .await
        .insert(old_id.to_string(), current.clone());
    let mut accepted = current.clone();
    accepted["filled_amount"] = json!("0");
    accepted["last_update_timestamp"] = json!(1_700_000_001_000_i64);
    *ws_state.order_reply.lock().await = Some(json!({"result": {"order": accepted, "trades": []}}));
    let mut replacement = order_json_with(
        "native-next-leg",
        cid.as_str(),
        "buy",
        "ETH-PERP",
        1_700_000_002_000,
        "open",
    );
    replacement["amount"] = json!(expected_child_amount);
    replacement["filled_amount"] = json!("0");
    replacement["replaced_order_id"] = json!(old_id.as_str());
    let mut cancelled = current;
    cancelled["order_status"] = json!("cancelled");
    *ws_state.replace_reply.lock().await =
        Some(json!({"result": {"order": replacement, "cancelled_order": cancelled, "trades": []}}));
    let mut tc = build_report_client(rest_state.clone(), ws_state.clone()).await;
    let order = build_limit_order(
        instrument_id,
        cid,
        OrderSide::Buy,
        Price::from("3500.00"),
        Quantity::from("1.000"),
    );

    if restored_alias {
        let mut restored = accepted_order(
            order.clone(),
            VenueOrderId::from("native-ancestor-leg"),
            AccountId::from("DERIVE-001"),
        );
        restored
            .apply(OrderEventAny::Updated(OrderUpdated::new(
                restored.trader_id(),
                restored.strategy_id(),
                instrument_id,
                cid,
                Quantity::from("1.000"),
                UUID4::new(),
                UnixNanos::from(2),
                UnixNanos::from(2),
                false,
                Some(old_id),
                Some(restored.account_id().unwrap()),
                Some(Price::from("3500.00")),
                None,
                None,
                false,
            )))
            .unwrap();
        add_order_to_cache(&tc.cache, restored, Some(ClientId::from("DERIVE")));
    }

    tc.client.connect().await.unwrap();
    drain_initial_account_state(&mut tc).await;
    if !restored_alias {
        submit_cached_order(&tc, &order);
        drain_until(
            &mut tc.rx,
            |e| matches!(e, ExecutionEvent::Order(OrderEventAny::Accepted(_))),
            "original acceptance",
        )
        .await;
    }

    tc.client
        .modify_order(ModifyOrder::new(
            TraderId::from("TRADER-001"),
            Some(ClientId::from("DERIVE")),
            StrategyId::from("S-1"),
            instrument_id,
            cid,
            Some(old_id),
            Some(Quantity::from("1.500")),
            Some(Price::from("3505.00")),
            None,
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ))
        .unwrap();

    let ExecutionEvent::Order(OrderEventAny::Updated(updated)) = drain_until(
        &mut tc.rx,
        |e| matches!(e, ExecutionEvent::Order(OrderEventAny::Updated(_))),
        "replacement target",
    )
    .await
    else {
        panic!("expected Updated");
    };

    let writes = ws_state.replace_orders.lock().await;
    assert_eq!(writes.len(), 1);
    assert_eq!(
        writes[0]["amount"]
            .as_str()
            .unwrap()
            .parse::<rust_decimal::Decimal>()
            .unwrap(),
        expected_child_amount
            .parse::<rust_decimal::Decimal>()
            .unwrap()
    );
    assert_eq!(
        writes[0]["expected_filled_amount"]
            .as_str()
            .expect("exact native guard is mandatory")
            .parse::<rust_decimal::Decimal>()
            .unwrap(),
        current_filled.parse::<rust_decimal::Decimal>().unwrap()
    );
    assert_eq!(writes[0]["order_id_to_cancel"], json!(old_id.as_str()));
    assert_eq!(updated.client_order_id, cid);
    assert_eq!(updated.instrument_id, instrument_id);
    assert_eq!(updated.quantity, Quantity::from("1.500"));
    assert_eq!(updated.price, Some(Price::from("3505.00")));
    assert_eq!(
        updated.venue_order_id,
        Some(VenueOrderId::from("native-next-leg"))
    );
    assert_eq!(
        rest_state.get_order_calls.lock().await.len(),
        1 + usize::from(ancestor_filled.is_some())
    );
    drop(writes);
    tc.client.disconnect().await.unwrap();
}

#[rstest]
#[case::stable("stable", None)]
#[case::short("short", Some("incomplete Derive"))]
#[case::repeated("repeated", Some("Derive pagination made no progress"))]
#[case::empty("empty", Some("Derive pagination made no progress"))]
#[case::negative("negative", Some("Invalid Derive page count"))]
#[case::overflow("overflow", Some("Invalid Derive page count"))]
#[case::limit("limit", Some("Derive page count exceeds collection limit"))]
#[case::negative_count("negative_count", Some("Invalid Derive record count"))]
#[case::changing_count("changing_count", Some("Derive page count changed during collection"))]
#[case::expanding("expanding", Some("Derive page count changed during collection"))]
#[case::shrinking("shrinking", Some("Derive page count changed during collection"))]
#[tokio::test]
async fn test_private_report_pagination_rejects_incomplete_pages(
    #[case] scenario: &str,
    #[case] expected_error: Option<&str>,
    #[values(false, true)] fills: bool,
) {
    let state = RestState::default();
    configure_private_pagination(&state, scenario, fills).await;
    let mut tc = build_report_client(state.clone(), WsState::default()).await;
    tc.client.connect().await.unwrap();
    let result = if fills {
        tc.client
            .generate_fill_reports(GenerateFillReports::new(
                UUID4::new(),
                UnixNanos::default(),
                Some(InstrumentId::from("ETH-PERP.DERIVE")),
                None,
                None,
                None,
                None,
                None,
            ))
            .await
            .map(|reports| {
                reports
                    .into_iter()
                    .map(|report| report.venue_order_id)
                    .collect::<Vec<_>>()
            })
    } else {
        tc.client
            .generate_order_status_reports(&GenerateOrderStatusReports::new(
                UUID4::new(),
                UnixNanos::default(),
                false,
                Some(InstrumentId::from("ETH-PERP.DERIVE")),
                None,
                None,
                None,
                None,
            ))
            .await
            .map(|reports| {
                reports
                    .into_iter()
                    .map(|report| report.venue_order_id)
                    .collect::<Vec<_>>()
            })
    };

    if let Some(expected) = expected_error {
        assert!(format!("{:#}", result.unwrap_err()).contains(expected));
    } else {
        let mut expected = vec![
            VenueOrderId::from("page-order-1"),
            VenueOrderId::from("page-order-2"),
        ];

        if !fills {
            expected.push(VenueOrderId::from("ord-mock-1"));
        }

        assert_eq!(result.unwrap(), expected);
    }

    let requests = if fills {
        state.trade_history_calls.lock().await.clone()
    } else {
        state.order_history_calls.lock().await.clone()
    };

    assert_eq!(
        requests.len(),
        match scenario {
            "negative" | "overflow" | "empty" | "limit" | "negative_count" | "short" => 1,
            "changing_count" | "expanding" | "shrinking" => 6,
            _ => 2,
        }
    );
    assert_eq!(requests[0]["page"], json!(1));
    assert_eq!(requests[0]["page_size"], json!(500));

    if requests.len() == 2 {
        assert_eq!(requests[1]["page"], json!(2));
    }

    tc.client.disconnect().await.unwrap();
}

async fn configure_private_pagination(state: &RestState, scenario: &str, fills: bool) {
    let first = pagination_record(fills, 1);
    let second = if scenario == "repeated" {
        first.clone()
    } else {
        pagination_record(fills, 2)
    };

    let first_count: i64 = match scenario {
        "short" => 1,
        "negative" => -1,
        "overflow" => 4_294_967_296,
        "limit" => 1_001,
        "shrinking" => 3,
        _ => 2,
    };

    let second_count: i64 = match scenario {
        "expanding" => 3,
        "shrinking" => 1,
        _ => first_count,
    };

    let field = if fills { "trades" } else { "orders" };
    let mut first_page = json!({"pagination": {"count": 2, "num_pages": first_count}, "subaccount_id": TEST_SUBACCOUNT});
    first_page[field] = json!(if scenario == "empty" {
        vec![]
    } else {
        vec![first]
    });

    if scenario == "negative_count" {
        first_page["pagination"]["count"] = json!(-1);
    }

    let mut second_page = json!({"pagination": {"count": 2, "num_pages": second_count}, "subaccount_id": TEST_SUBACCOUNT});
    second_page[field] = json!([second]);
    if scenario == "changing_count" {
        second_page["pagination"]["count"] = json!(3);
    }

    let mut pages = vec![first_page.clone(), second_page.clone()];
    if matches!(scenario, "changing_count" | "expanding" | "shrinking") {
        pages.extend([
            first_page.clone(),
            second_page.clone(),
            first_page,
            second_page,
        ]);
    }

    if fills {
        *state.trade_history_pages.lock().await = pages;
    } else {
        *state.order_history_pages.lock().await = pages;
    }
}

fn pagination_record(fills: bool, page: u8) -> Value {
    let order_id = format!("page-order-{page}");
    let label = format!("PAGE-{page}");
    if fills {
        trade_json_with_label(&format!("page-trade-{page}"), &order_id, "ETH-PERP", &label)
    } else {
        order_json_with(
            &order_id,
            &label,
            "buy",
            "ETH-PERP",
            1_700_000_000_000 + i64::from(page) * 1000,
            "cancelled",
        )
    }
}

#[rstest]
#[case::direct_current(0)]
#[case::direct_parent(1)]
#[case::bulk(2)]
#[case::mass_engine(3)]
#[case::mass_manager(4)]
#[case::bounded_engine(5)]
#[case::bounded_manager(6)]
#[case::reversed_engine(7)]
#[case::reversed_manager(8)]
#[case::duplicate_engine(9)]
#[case::duplicate_manager(10)]
#[tokio::test]
async fn test_replacement_reports_project_logical_quantity_and_average(
    #[case] route: u8,
    #[values(false, true)] terminal: bool,
    #[values(false, true)] closed_cache: bool,
    #[values(false, true)] ancestor_history_only: bool,
    #[values("cancelled", "expired", "filled")] ancestor_status: &str,
) {
    let state = RestState::default();
    let cid = ClientOrderId::from("LOGICAL-REPLACEMENT");
    let instrument_id = InstrumentId::from("ETH-PERP.DERIVE");
    let parent_id = VenueOrderId::from("logical-parent");
    let current_id = VenueOrderId::from("logical-current");

    let bounded = matches!(route, 5 | 6);

    let epoch_ms = if bounded {
        get_atomic_clock_realtime().get_time_ns().as_u64() / 1_000_000 - 2_000
    } else {
        1_700_000_000_000
    };

    let parent_created_ms = epoch_ms - if bounded { 120_000 } else { 0 };

    let (child_filled, cumulative_filled, average, native_status, report_status) = if terminal {
        ("1.2", "1.500", dec!(3504), "filled", OrderStatus::Filled)
    } else {
        (
            "0.2",
            "0.500",
            dec!(3502),
            "open",
            OrderStatus::PartiallyFilled,
        )
    };

    let mut parent = order_json_with(
        parent_id.as_str(),
        cid.as_str(),
        "buy",
        "ETH-PERP",
        i64::try_from(epoch_ms + 1_000).unwrap(),
        ancestor_status,
    );
    parent["creation_timestamp"] = json!(parent_created_ms);
    parent["amount"] = json!(if ancestor_status == "filled" {
        "0.3"
    } else {
        "1.0"
    });
    parent["filled_amount"] = json!("0.3");
    parent["average_price"] = json!("3500");
    let mut current = order_json_with(
        current_id.as_str(),
        cid.as_str(),
        "buy",
        "ETH-PERP",
        i64::try_from(epoch_ms + 2_000).unwrap(),
        native_status,
    );
    current["creation_timestamp"] = json!(epoch_ms + 1_500);
    current["amount"] = json!("1.2");
    current["filled_amount"] = json!(child_filled);
    current["average_price"] = json!("3505");
    current["limit_price"] = json!("3505.00");
    current["replaced_order_id"] = Value::Null;

    configure_replacement_order_reports(
        &state,
        parent,
        current,
        route,
        terminal,
        ancestor_history_only,
    )
    .await;
    let mut parent_fill = trade_json_with_label(
        "logical-parent-fill",
        parent_id.as_str(),
        "ETH-PERP",
        cid.as_str(),
    );
    parent_fill["timestamp"] = json!(epoch_ms + 1_000);
    parent_fill["trade_amount"] = json!("0.3");
    parent_fill["trade_price"] = json!("3500");
    parent_fill["trade_fee"] = json!("0.03");
    let mut child_fill = trade_json_with_label(
        "logical-child-fill",
        current_id.as_str(),
        "ETH-PERP",
        cid.as_str(),
    );
    child_fill["timestamp"] = json!(epoch_ms + 2_000);
    child_fill["trade_amount"] = json!(child_filled);
    child_fill["trade_price"] = json!("3505");
    child_fill["trade_fee"] = json!("0.05");

    let trades = if matches!(route, 9 | 10) {
        vec![child_fill, parent_fill.clone(), parent_fill]
    } else {
        vec![child_fill, parent_fill]
    };

    *state.trade_history_response.lock().await = json!({"trades": trades, "pagination": {"count": 2, "num_pages": 1}, "subaccount_id": TEST_SUBACCOUNT});
    let mut position = sample_position_json("ETH-PERP", cumulative_filled);
    position["average_price"] = json!(average.to_string());
    *state.positions_response.lock().await =
        json!({"positions": [position], "subaccount_id": TEST_SUBACCOUNT});
    let mut tc = build_report_client(state, WsState::default()).await;
    let mut restored = accepted_order(
        build_limit_order(
            instrument_id,
            cid,
            OrderSide::Buy,
            Price::from("3500.00"),
            Quantity::from("1.000"),
        ),
        parent_id,
        AccountId::from("DERIVE-001"),
    );
    restored
        .apply(OrderEventAny::Updated(OrderUpdated::new(
            restored.trader_id(),
            restored.strategy_id(),
            instrument_id,
            cid,
            Quantity::from("1.500"),
            UUID4::new(),
            UnixNanos::from(2),
            UnixNanos::from(2),
            false,
            Some(current_id),
            restored.account_id(),
            Some(Price::from("3505.00")),
            None,
            None,
            false,
        )))
        .unwrap();

    if closed_cache {
        restored
            .apply(OrderEventAny::Canceled(OrderCanceled::new(
                restored.trader_id(),
                restored.strategy_id(),
                instrument_id,
                cid,
                UUID4::new(),
                UnixNanos::from(3),
                UnixNanos::from(3),
                false,
                Some(current_id),
                Some(AccountId::from("DERIVE-001")),
                None,
            )))
            .unwrap();
    }

    add_order_to_cache(&tc.cache, restored, Some(ClientId::from("DERIVE")));
    tc.client.connect().await.unwrap();
    let mut mass = None;

    let reports = match route {
        0 | 1 => vec![
            tc.client
                .generate_order_status_report(&GenerateOrderStatusReport::new(
                    UUID4::new(),
                    UnixNanos::default(),
                    Some(instrument_id),
                    Some(cid),
                    Some(if route == 1 { parent_id } else { current_id }),
                    None,
                    None,
                ))
                .await
                .unwrap()
                .unwrap(),
        ],
        2 => tc
            .client
            .generate_order_status_reports(&GenerateOrderStatusReports::new(
                UUID4::new(),
                UnixNanos::default(),
                !terminal,
                Some(instrument_id),
                None,
                None,
                None,
                None,
            ))
            .await
            .unwrap(),
        3..=10 => {
            let value = tc
                .client
                .generate_mass_status(bounded.then_some(1))
                .await
                .unwrap()
                .unwrap();
            let reports = value.order_reports().into_values().collect();
            mass = Some(value);
            reports
        }
        _ => unreachable!(),
    };

    let owned: Vec<_> = reports
        .iter()
        .filter(|report| report.client_order_id == Some(cid) && report.venue_order_id == current_id)
        .collect();
    assert_eq!(owned.len(), 1);
    let report = owned[0];
    assert_eq!(report.account_id, AccountId::from("DERIVE-001"));
    assert_eq!(report.instrument_id, instrument_id);
    assert_eq!(report.venue_order_id, current_id);
    assert_eq!(report.order_side, Some(OrderSide::Buy));
    assert_eq!(report.order_type, OrderType::Limit);
    assert_eq!(report.quantity, Quantity::from("1.500"));
    assert_eq!(report.filled_qty, Quantity::from(cumulative_filled));
    assert_eq!(report.avg_px, Some(average));
    assert_eq!(report.price, Some(Price::from("3505.00")));
    assert_eq!(
        report.ts_accepted,
        UnixNanos::from(parent_created_ms * 1_000_000)
    );
    assert_eq!(
        report.ts_last,
        UnixNanos::from((epoch_ms + 2_000) * 1_000_000)
    );
    assert_eq!(report.order_status, report_status);
    assert_eq!(
        reports
            .iter()
            .filter(|report| report.venue_order_id == parent_id)
            .count(),
        usize::from(route >= 3 || route == 2 && terminal),
    );

    tc.client.disconnect().await.unwrap();

    if let Some(mass) = mass {
        assert!(mass.reports_complete());
        let fills = mass.fill_reports();
        assert_eq!(fills.len(), 2);
        let parent = &fills[&parent_id];
        let child = &fills[&current_id];
        assert_eq!(parent.len(), 1);
        assert_eq!(child.len(), 1);
        assert_eq!(parent[0].venue_order_id, parent_id);
        assert_eq!(parent[0].client_order_id, Some(cid));
        assert_eq!(parent[0].trade_id, TradeId::from("logical-parent-fill"));
        assert_eq!(parent[0].last_qty, Quantity::from("0.300"));
        assert_eq!(parent[0].last_px, Price::from("3500.00"));
        assert_eq!(parent[0].commission, Money::from("0.03 USDC"));
        assert_eq!(child[0].venue_order_id, current_id);
        assert_eq!(child[0].client_order_id, Some(cid));
        assert_eq!(child[0].trade_id, TradeId::from("logical-child-fill"));
        assert_eq!(
            child[0].last_qty,
            Quantity::from_decimal_dp(child_filled.parse().unwrap(), 3).unwrap()
        );
        assert_eq!(child[0].last_px, Price::from("3505.00"));
        assert_eq!(child[0].commission, Money::from("0.05 USDC"));

        reconcile_replacement_reports(
            tc,
            &mass,
            report,
            parent_id,
            child_filled,
            closed_cache,
            route.is_multiple_of(2),
        );
    }
}

async fn configure_replacement_order_reports(
    state: &RestState,
    parent: Value,
    current: Value,
    route: u8,
    terminal: bool,
    ancestor_history_only: bool,
) {
    let parent_lookup = if ancestor_history_only {
        json!({"id": 1, "error": {"code": 11006, "message": "Does not exist", "data": null}})
    } else {
        parent.clone()
    };

    state.get_order_responses.lock().await.insert(
        parent["order_id"].as_str().unwrap().to_string(),
        parent_lookup,
    );
    state.get_order_responses.lock().await.insert(
        current["order_id"].as_str().unwrap().to_string(),
        current.clone(),
    );
    *state.open_orders_response.lock().await = json!({"orders": if terminal { vec![] } else { vec![current.clone()] }, "subaccount_id": TEST_SUBACCOUNT});

    let mut history = if terminal {
        vec![parent.clone(), current]
    } else {
        vec![parent.clone()]
    };

    if matches!(route, 5 | 6) && !ancestor_history_only {
        history.retain(|order| order["order_id"] != parent["order_id"].as_str().unwrap());
    }

    if matches!(route, 7 | 8) {
        history.reverse();
    }

    *state.order_history_response.lock().await = json!({"orders": history, "pagination": {"count": history.len(), "num_pages": 1}, "subaccount_id": TEST_SUBACCOUNT});
}

fn reconcile_replacement_reports(
    tc: TestClient,
    mass: &ExecutionMassStatus,
    expected: &OrderStatusReport,
    parent_id: VenueOrderId,
    child_filled: &str,
    closed_cache: bool,
    use_manager: bool,
) {
    let cid = expected.client_order_id.unwrap();
    let current_id = expected.venue_order_id;
    let instrument_id = expected.instrument_id;
    let clock = Rc::new(RefCell::new(VirtualClock::new()));
    let mut engine = ExecutionEngine::new(clock.clone(), tc.cache.clone(), None);
    engine.register_client(Box::new(tc.client)).unwrap();
    let engine = RefCell::new(engine);
    let mut manager =
        ExecutionManager::new(clock, tc.cache.clone(), ExecutionManagerConfig::default()).unwrap();

    let expected_status = if closed_cache && expected.order_status != OrderStatus::Filled {
        OrderStatus::Canceled
    } else {
        expected.order_status
    };

    for _ in 0..2 {
        if use_manager {
            manager.reconcile_execution_mass_status(mass, &engine);
        } else {
            engine.borrow_mut().reconcile_execution_mass_status(mass);
        }

        let cache = tc.cache.borrow();
        let order = cache.order(&cid).unwrap();

        let events: Vec<_> = order
            .events()
            .into_iter()
            .filter_map(|event| match event {
                OrderEventAny::Filled(fill) => Some(fill),
                _ => None,
            })
            .collect();

        assert_eq!(order.quantity(), Quantity::from("1.500"));
        assert_eq!(order.filled_qty(), expected.filled_qty);
        assert_eq!(order.venue_order_id(), Some(current_id));
        assert_eq!(order.avg_px(), expected.avg_px);
        assert_eq!(
            order.commissions().get(&Currency::USDC()),
            Some(&Money::from("0.08 USDC"))
        );
        assert_eq!(
            order
                .events()
                .iter()
                .filter(|event| matches!(event, OrderEventAny::Updated(_)))
                .count(),
            1
        );
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].venue_order_id, parent_id);
        assert_eq!(events[0].trade_id, TradeId::from("logical-parent-fill"));
        assert_eq!(events[0].last_qty, Quantity::from("0.300"));
        assert_eq!(events[0].last_px, Price::from("3500.00"));
        assert_eq!(events[0].commission, Some(Money::from("0.03 USDC")));
        assert_eq!(events[1].venue_order_id, current_id);
        assert_eq!(events[1].trade_id, TradeId::from("logical-child-fill"));
        assert_eq!(events[1].last_qty, Quantity::from(child_filled));
        assert_eq!(events[1].last_px, Price::from("3505.00"));
        assert_eq!(events[1].commission, Some(Money::from("0.05 USDC")));
        assert_eq!(order.status(), expected_status);
        assert_eq!(order.trade_ids().len(), 2);
        let positions = cache.positions_open(
            None,
            Some(&instrument_id),
            None,
            Some(&AccountId::from("DERIVE-001")),
            None,
        );
        assert_eq!(positions.len(), 1);
        assert_eq!(positions[0].quantity, order.filled_qty());
        assert_eq!(positions[0].commissions(), vec![Money::from("0.08 USDC")]);
    }
}

#[rstest]
#[case::second_page("second_page", None, 2)]
#[case::different_id("different_id", Some("Does not exist"), 1)]
#[case::different_error("different_error", Some("Permission error"), 0)]
#[case::foreign_account("foreign_account", Some("subaccount"), 1)]
#[case::foreign_instrument("foreign_instrument", None, 1)]
#[case::bad_pagination("bad_pagination", Some("Invalid Derive page count"), 1)]
#[tokio::test]
async fn test_order_lookup_falls_back_to_exact_scoped_history(
    #[case] scenario: &str,
    #[case] expected_error: Option<&str>,
    #[case] history_calls: usize,
) {
    let state = RestState::default();
    let cid = ClientOrderId::from("HISTORY-LOOKUP");
    let native_id = VenueOrderId::from("history-exact-order");
    let instrument_id = InstrumentId::from("ETH-PERP.DERIVE");
    let mut native = order_json_with(
        native_id.as_str(),
        cid.as_str(),
        "buy",
        "ETH-PERP",
        1_700_000_002_000,
        "cancelled",
    );

    let error_code = if scenario == "different_error" {
        11000
    } else {
        11006
    };

    let message = if scenario == "different_error" {
        "Permission error"
    } else {
        "Does not exist"
    };

    state.get_order_responses.lock().await.insert(
        native_id.to_string(),
        json!({"id": 1, "error": {"code": error_code, "message": message, "data": null}}),
    );

    match scenario {
        "different_id" => native["order_id"] = json!("same-label-other-order"),
        "foreign_account" => native["subaccount_id"] = json!(TEST_SUBACCOUNT + 1),
        "foreign_instrument" => native["instrument_name"] = json!("BTC-PERP"),
        _ => {}
    }

    let mut page = json!({"orders": [native.clone()], "pagination": {"count": 1, "num_pages": 1}, "subaccount_id": TEST_SUBACCOUNT});
    if scenario == "bad_pagination" {
        page["pagination"]["num_pages"] = json!(-1);
    }

    if scenario == "second_page" {
        let other = order_json_with(
            "history-first-order",
            cid.as_str(),
            "buy",
            "ETH-PERP",
            1_700_000_001_000,
            "cancelled",
        );
        *state.order_history_pages.lock().await = vec![
            json!({"orders": [other], "pagination": {"count": 2, "num_pages": 2}, "subaccount_id": TEST_SUBACCOUNT}),
            json!({"orders": [native], "pagination": {"count": 2, "num_pages": 2}, "subaccount_id": TEST_SUBACCOUNT}),
        ];
    } else {
        *state.order_history_response.lock().await = page;
    }

    let mut tc = build_report_client(state.clone(), WsState::default()).await;
    tc.client.connect().await.unwrap();
    let result = tc
        .client
        .generate_order_status_report(&GenerateOrderStatusReport::new(
            UUID4::new(),
            UnixNanos::default(),
            Some(instrument_id),
            Some(cid),
            Some(native_id),
            None,
            None,
        ))
        .await;

    if let Some(expected) = expected_error {
        assert!(format!("{:#}", result.unwrap_err()).contains(expected));
    } else if scenario == "foreign_instrument" {
        assert!(result.unwrap().is_none());
    } else {
        let report = result.unwrap().unwrap();
        assert_eq!(report.venue_order_id, native_id);
        assert_eq!(report.client_order_id, Some(cid));
        assert_eq!(report.instrument_id, instrument_id);
        assert_eq!(report.account_id, AccountId::from("DERIVE-001"));
        assert_eq!(report.order_status, OrderStatus::Canceled);
        assert_eq!(report.quantity, Quantity::from("1.000"));
        assert_eq!(report.filled_qty, Quantity::from("0.000"));
    }

    let calls = state.order_history_calls.lock().await;
    assert_eq!(calls.len(), history_calls);

    for (index, call) in calls.iter().enumerate() {
        assert_eq!(call["page"], json!(index + 1));
        assert_eq!(call["page_size"], json!(500));
        assert_eq!(call["subaccount_id"], json!(TEST_SUBACCOUNT));
        assert_eq!(call["instrument_name"], json!("ETH-PERP"));
    }

    drop(calls);
    tc.client.disconnect().await.unwrap();
}

#[rstest]
#[case::open("open", false)]
#[case::open_pending("open", true)]
#[case::terminal("terminal", false)]
#[case::terminal_pending("terminal", true)]
#[case::malformed("malformed", false)]
#[case::malformed_pending("malformed", true)]
#[case::empty_pending("empty", true)]
#[tokio::test]
async fn test_startup_refuses_unproved_replacement_before_private_connection(
    #[case] scenario: &str,
    #[case] pending: bool,
) {
    let state = RestState::default();
    let ws = WsState::default();
    let cid = ClientOrderId::from("RESTART-UNPROVED");
    let instrument_id = InstrumentId::from("ETH-PERP.DERIVE");
    let old_id = VenueOrderId::from("restart-old-leg");
    let mut child = order_json_with(
        "restart-unknown-child",
        cid.as_str(),
        "buy",
        "ETH-PERP",
        1_700_000_002_000,
        if scenario == "open" { "open" } else { "filled" },
    );
    child["replaced_order_id"] = Value::Null;
    if scenario == "malformed" {
        child["order_status"] = json!("unknown-native-state");
    }

    let children = if scenario == "empty" {
        vec![]
    } else {
        vec![child]
    };

    *state.open_orders_response.lock().await = json!({"orders": if scenario == "open" { children.clone() } else { vec![] }, "subaccount_id": TEST_SUBACCOUNT});
    *state.order_history_response.lock().await = json!({"orders": if scenario == "open" { vec![] } else { children.clone() }, "pagination": {"count": if scenario == "open" { 0 } else { children.len() }, "num_pages": i32::from(scenario != "open" && !children.is_empty())}, "subaccount_id": TEST_SUBACCOUNT});
    let mut tc = build_report_client(state.clone(), ws.clone()).await;
    let mut restored = accepted_order(
        build_limit_order(
            instrument_id,
            cid,
            OrderSide::Buy,
            Price::from("3500.00"),
            Quantity::from("1.000"),
        ),
        old_id,
        AccountId::from("DERIVE-001"),
    );

    if pending {
        restored
            .apply(OrderEventAny::PendingUpdate(OrderPendingUpdate::new(
                restored.trader_id(),
                restored.strategy_id(),
                instrument_id,
                cid,
                restored.account_id(),
                UUID4::new(),
                UnixNanos::from(2),
                UnixNanos::from(2),
                false,
                Some(old_id),
            )))
            .unwrap();
    }

    add_order_to_cache(&tc.cache, restored, Some(ClientId::from("DERIVE")));
    let error = tc.client.connect().await.unwrap_err();
    assert_eq!(
        error.to_string(),
        "unresolved Derive venue binding for RESTART-UNPROVED"
    );
    assert!(!tc.client.is_connected());
    assert_eq!(ws.connection_count.load(Ordering::SeqCst), 0);
    assert!(ws.login_frames.lock().await.is_empty());
    assert!(ws.subscribe_frames.lock().await.is_empty());
    assert!(ws.replace_orders.lock().await.is_empty());
    assert!(ws.cancelled_orders.lock().await.is_empty());
    assert!(ws.cancelled_labels.lock().await.is_empty());
    {
        let cache = tc.cache.borrow();
        let cached = cache.order(&cid).unwrap();
        assert_eq!(cached.venue_order_id(), Some(old_id));
        assert_eq!(
            cached.status(),
            if pending {
                OrderStatus::PendingUpdate
            } else {
                OrderStatus::Accepted
            }
        );
        assert_eq!(cached.quantity(), Quantity::from("1.000"));
    }

    while let Ok(event) = tc.rx.try_recv() {
        assert!(!matches!(event, ExecutionEvent::Order(_)));
    }

    tc.client.disconnect().await.unwrap();
}

#[rstest]
#[case("zero_amount", "Invalid native replacement amount or filled amount")]
#[case(
    "negative_filled",
    "Invalid native replacement amount or filled amount"
)]
#[case("excess_filled", "Invalid native replacement amount or filled amount")]
#[case("filled_precision", "financial input exceeds 12 fractional digits")]
#[case(
    "closed_current",
    "Native replacement target is not open; current binding requires reconciliation"
)]
#[case("open_ancestor", "Native replacement ancestor is not closed")]
#[case("cycle", "Cyclic native replacement ancestry")]
#[case(
    "ancestor_negative",
    "Invalid native replacement amount or filled amount"
)]
#[case("side", "Native replacement order side does not match")]
#[case("order_type", "Native replacement order type does not match")]
#[case(
    "target_below_filled",
    "requested quantity is below native cumulative fills"
)]
#[tokio::test]
async fn test_modify_order_rejects_invalid_native_replacement_chain_before_write(
    #[case] invalid_field: &str,
    #[case] expected_reason: &str,
) {
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    let instrument_id = InstrumentId::from("ETH-PERP.DERIVE");
    let cid = ClientOrderId::from("NATIVE-REPLACE-BOUNDS");
    let old_id = VenueOrderId::from("native-current-leg");
    let accepted = order_json_with(
        old_id.as_str(),
        cid.as_str(),
        "buy",
        "ETH-PERP",
        1_700_000_001_000,
        "open",
    );
    let mut current = accepted.clone();
    current["last_update_timestamp"] = json!(1_700_000_002_000_i64);
    current["filled_amount"] = json!("0");
    match invalid_field {
        "zero_amount" => current["amount"] = json!("0"),
        "negative_filled" => current["filled_amount"] = json!("-0.1"),
        "excess_filled" => current["filled_amount"] = json!("1.1"),
        "filled_precision" => current["filled_amount"] = json!("0.0000000000001"),
        "closed_current" => current["order_status"] = json!("cancelled"),
        "cycle" => current["replaced_order_id"] = json!(old_id.as_str()),
        "side" => current["direction"] = json!("sell"),
        "order_type" => current["order_type"] = json!("market"),
        "target_below_filled" => {
            current["amount"] = json!("2");
            current["filled_amount"] = json!("1.6");
        }
        "open_ancestor" | "ancestor_negative" => {
            current["replaced_order_id"] = json!("native-ancestor-leg");
            let mut ancestor = order_json_with(
                "native-ancestor-leg",
                cid.as_str(),
                "buy",
                "ETH-PERP",
                1_700_000_000_000,
                "cancelled",
            );

            if invalid_field == "open_ancestor" {
                ancestor["order_status"] = json!("open");
            } else {
                ancestor["filled_amount"] = json!("-0.1");
            }

            rest_state
                .get_order_responses
                .lock()
                .await
                .insert("native-ancestor-leg".to_string(), ancestor);
        }
        _ => unreachable!(),
    }

    rest_state
        .get_order_responses
        .lock()
        .await
        .insert(old_id.to_string(), current);
    *ws_state.order_reply.lock().await = Some(json!({"result": {"order": accepted, "trades": []}}));
    let mut tc = build_client(rest_state.clone(), ws_state.clone()).await;
    tc.client.connect().await.unwrap();
    drain_initial_account_state(&mut tc).await;
    let order = build_limit_order(
        instrument_id,
        cid,
        OrderSide::Buy,
        Price::from("3500.00"),
        Quantity::from("1.000"),
    );
    submit_cached_order(&tc, &order);
    drain_until(
        &mut tc.rx,
        |event| matches!(event, ExecutionEvent::Order(OrderEventAny::Accepted(_))),
        "original acceptance",
    )
    .await;
    tc.client
        .modify_order(ModifyOrder::new(
            TraderId::from("TRADER-001"),
            Some(ClientId::from("DERIVE")),
            StrategyId::from("S-1"),
            instrument_id,
            cid,
            Some(old_id),
            Some(Quantity::from("1.500")),
            Some(Price::from("3505.00")),
            None,
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ))
        .unwrap();

    let ExecutionEvent::Order(OrderEventAny::ModifyRejected(rejected)) = drain_until(
        &mut tc.rx,
        |event| matches!(event, ExecutionEvent::Order(_)),
        "native chain rejection",
    )
    .await
    else {
        panic!("expected matching command rejection");
    };

    assert_eq!(rejected.client_order_id, cid);
    assert_eq!(rejected.instrument_id, instrument_id);
    assert_eq!(rejected.venue_order_id, Some(old_id));
    assert_eq!(rejected.reason, expected_reason);
    assert_eq!(ws_state.replace_orders.lock().await.len(), 0);
    assert_eq!(
        rest_state.get_order_calls.lock().await.len(),
        if invalid_field.starts_with("ancestor_") || invalid_field == "open_ancestor" {
            2
        } else {
            1
        }
    );
    assert!(tc.rx.try_recv().is_err());
    tc.client.disconnect().await.unwrap();
}

#[rstest]
#[case("none", "1.500", OrderStatus::PartiallyFilled)]
#[case("none", "2.000", OrderStatus::Filled)]
#[case("trade", "1.500", OrderStatus::PartiallyFilled)]
#[case("trade", "2.000", OrderStatus::Filled)]
#[case("order", "1.500", OrderStatus::PartiallyFilled)]
#[case("order", "2.000", OrderStatus::Filled)]
#[tokio::test]
async fn test_modify_order_applies_target_before_synchronous_or_early_fill(
    #[case] notification_first: &str,
    #[case] filled_amount: &str,
    #[case] expected_status: OrderStatus,
) {
    let ws_state = WsState::default();
    let client_order_id = ClientOrderId::from("STRAT-REPLACE-FILL");
    let instrument_id = InstrumentId::from("ETH-PERP.DERIVE");
    let original = order_json_with(
        "ord-original",
        client_order_id.as_str(),
        "buy",
        "ETH-PERP",
        1_700_000_001_000,
        "open",
    );
    *ws_state.order_reply.lock().await = Some(json!({"result": {"order": original, "trades": []}}));

    let mut replacement = order_json_with(
        "ord-replacement",
        client_order_id.as_str(),
        "buy",
        "ETH-PERP",
        1_700_000_002_000,
        if expected_status == OrderStatus::Filled {
            "filled"
        } else {
            "open"
        },
    );

    replacement["amount"] = json!("2.000");
    replacement["filled_amount"] = json!(filled_amount);
    replacement["limit_price"] = json!("3505.00");
    replacement["signed_limit_price"] = Value::Null;
    replacement["replaced_order_id"] = json!("ord-original");
    let mut trade = trade_json_with_label(
        "replace-fill-1",
        "ord-replacement",
        "ETH-PERP",
        client_order_id.as_str(),
    );
    trade["trade_amount"] = json!(filled_amount);
    *ws_state.replace_reply.lock().await = Some(json!({"result": {
        "order": replacement.clone(), "cancelled_order": order_json_with("ord-original", client_order_id.as_str(), "buy", "ETH-PERP", 1_700_000_002_000, "cancelled"), "trades": [trade.clone()],
    }}));
    let mut tc = build_client(RestState::default(), ws_state.clone()).await;
    tc.client.connect().await.unwrap();
    let mut order = build_limit_order(
        instrument_id,
        client_order_id,
        OrderSide::Buy,
        Price::from("3500.00"),
        Quantity::from("1.000"),
    );
    tc.cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .unwrap();
    tc.client.submit_order(submit_cmd(&order)).unwrap();

    for expected in ["submitted", "accepted"] {
        let event = drain_until(
            &mut tc.rx,
            |e| matches!(e, ExecutionEvent::Order(_)),
            expected,
        )
        .await;

        let ExecutionEvent::Order(event) = event else {
            unreachable!()
        };

        assert_eq!(
            event.event_type(),
            if expected == "submitted" {
                OrderEventType::Submitted
            } else {
                OrderEventType::Accepted
            }
        );
        order.apply(event).unwrap();
    }

    if notification_first == "trade" {
        *ws_state.replace_notification_before_reply.lock().await = Some(make_subscription_frame(
            &format!("{TEST_SUBACCOUNT}.trades"),
            &json!([trade.clone()]),
        ));
    }

    if notification_first == "order" {
        *ws_state.replace_notification_before_reply.lock().await = Some(make_subscription_frame(
            &format!("{TEST_SUBACCOUNT}.orders"),
            &json!([replacement]),
        ));
    }

    tc.client
        .modify_order(ModifyOrder::new(
            TraderId::from("TRADER-001"),
            Some(ClientId::from("DERIVE")),
            StrategyId::from("S-1"),
            instrument_id,
            client_order_id,
            Some(VenueOrderId::from("ord-original")),
            Some(Quantity::from("2.000")),
            Some(Price::from("3505.00")),
            None,
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ))
        .unwrap();
    let event = drain_until(
        &mut tc.rx,
        |e| matches!(e, ExecutionEvent::Order(_)),
        "replacement update before fill",
    )
    .await;

    let ExecutionEvent::Order(OrderEventAny::Updated(updated)) = event else {
        panic!("expected update before fill: {event:?}")
    };

    assert_eq!(updated.client_order_id, client_order_id);
    assert_eq!(
        updated.venue_order_id,
        Some(VenueOrderId::from("ord-replacement"))
    );
    assert_eq!(updated.quantity, Quantity::from("2.000"));
    assert_eq!(updated.price, Some(Price::from("3505.00")));
    order.apply(OrderEventAny::Updated(updated)).unwrap();
    let event = drain_until(
        &mut tc.rx,
        |e| matches!(e, ExecutionEvent::Order(_)),
        "replacement fill",
    )
    .await;

    let ExecutionEvent::Order(OrderEventAny::Filled(filled)) = event else {
        panic!("expected fill: {event:?}")
    };

    assert_eq!(filled.client_order_id, client_order_id);
    assert_eq!(filled.venue_order_id, VenueOrderId::from("ord-replacement"));
    assert_eq!(filled.trade_id, TradeId::from("replace-fill-1"));
    assert_eq!(filled.last_qty, Quantity::from(filled_amount));
    assert_eq!(filled.last_px, Price::from("3505.00"));
    assert_eq!(filled.commission, Some(Money::from("0.50 USDC")));
    order.apply(OrderEventAny::Filled(filled)).unwrap();
    ws_state.push_notification(make_subscription_frame(
        &format!("{TEST_SUBACCOUNT}.trades"),
        &json!([trade]),
    ));
    ws_state.push_notification(make_subscription_frame(
        &format!("{TEST_SUBACCOUNT}.orders"),
        &json!([order_json_with(
            "ord-original",
            client_order_id.as_str(),
            "buy",
            "ETH-PERP",
            1_700_000_003_000,
            "cancelled"
        )]),
    ));
    assert_eq!(order.status(), expected_status);
    assert_eq!(order.quantity(), Quantity::from("2.000"));
    assert_eq!(order.filled_qty(), Quantity::from(filled_amount));
    assert_eq!(
        order.leaves_qty().as_decimal(),
        dec!(2) - Quantity::from(filled_amount).as_decimal()
    );
    assert_eq!(
        order.venue_order_id(),
        Some(VenueOrderId::from("ord-replacement"))
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(250), tc.rx.recv())
            .await
            .is_err()
    );
    assert_eq!(ws_state.replace_orders.lock().await.len(), 1);
    tc.client.disconnect().await.unwrap();
}

#[rstest]
#[case(false, false)]
#[case(true, false)]
#[case(false, true)]
#[case(true, true)]
#[tokio::test]
async fn test_submit_order_dedupes_response_fills_and_private_updates(
    #[case] notification_first: bool,
    #[case] partial_cancel: bool,
) {
    let ws_state = WsState::default();
    let client_order_id = ClientOrderId::from("STRAT-RESPONSE-FILL");
    let instrument_id = InstrumentId::from("ETH-PERP.DERIVE");

    let mut response_order = order_json_with(
        "ord-response",
        client_order_id.as_str(),
        "buy",
        "ETH-PERP",
        1_700_000_002_000,
        if partial_cancel {
            "cancelled"
        } else {
            "filled"
        },
    );

    response_order["filled_amount"] = json!(if partial_cancel { "0.500" } else { "1.000" });
    response_order["signed_limit_price"] = Value::Null;
    response_order["nonce"] = json!("18446744073709551615");
    let mut trade = trade_json_with_label(
        "response-fill-1",
        "ord-response",
        "ETH-PERP",
        client_order_id.as_str(),
    );
    trade["trade_amount"] = json!(if partial_cancel { "0.500" } else { "1.000" });
    *ws_state.order_reply.lock().await =
        Some(json!({"result": {"order": response_order, "trades": [trade.clone()]}}));

    if notification_first {
        *ws_state.order_notification_before_reply.lock().await = Some(make_subscription_frame(
            &format!("{TEST_SUBACCOUNT}.trades"),
            &json!([trade.clone()]),
        ));
    }

    let mut tc = build_client(RestState::default(), ws_state.clone()).await;
    tc.client.connect().await.unwrap();
    let mut order = build_limit_order(
        instrument_id,
        client_order_id,
        OrderSide::Buy,
        Price::from("3510.00"),
        Quantity::from("1.000"),
    );
    tc.cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .unwrap();
    tc.client.submit_order(submit_cmd(&order)).unwrap();
    let mut kinds = Vec::new();

    for _ in 0..if partial_cancel { 4 } else { 3 } {
        let event = drain_until(
            &mut tc.rx,
            |e| matches!(e, ExecutionEvent::Order(_)),
            "submit lifecycle",
        )
        .await;

        let ExecutionEvent::Order(event) = event else {
            unreachable!()
        };

        kinds.push(event.event_type());
        order.apply(event).unwrap();
    }

    ws_state.push_notification(make_subscription_frame(
        &format!("{TEST_SUBACCOUNT}.trades"),
        &json!([trade]),
    ));
    let mut expected = vec![
        OrderEventType::Submitted,
        OrderEventType::Accepted,
        OrderEventType::Filled,
    ];

    if partial_cancel {
        expected.push(OrderEventType::Canceled);
    }

    assert_eq!(kinds, expected);
    assert_eq!(
        order.status(),
        if partial_cancel {
            OrderStatus::Canceled
        } else {
            OrderStatus::Filled
        }
    );
    assert_eq!(
        order.filled_qty(),
        Quantity::from(if partial_cancel { "0.500" } else { "1.000" })
    );
    assert_eq!(
        order.venue_order_id(),
        Some(VenueOrderId::from("ord-response"))
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(250), tc.rx.recv())
            .await
            .is_err()
    );
    assert_eq!(ws_state.submitted_orders.lock().await.len(), 1);
    tc.client.disconnect().await.unwrap();
}

#[rstest]
#[case(OrderType::Limit, false)]
#[case(OrderType::StopMarket, false)]
#[case(OrderType::Limit, true)]
#[tokio::test]
async fn test_write_timeout_retains_identity_for_late_private_fill(
    #[case] order_type: OrderType,
    #[case] replace: bool,
) {
    let client_order_id = ClientOrderId::from("STRAT-TIMEOUT");
    let instrument_id = InstrumentId::from("ETH-PERP.DERIVE");
    let ws_state = WsState::default();
    *ws_state.order_reply.lock().await = Some(
        json!({"result": {"order": order_json_with("ord-original", client_order_id.as_str(), "buy", "ETH-PERP", 1_700_000_001_000, "open"), "trades": []}}),
    );

    let mut tc = build_client_with_config(
        RestState::default(),
        ws_state.clone(),
        None,
        |mut config| {
            config.ws_timeout_secs = Some(1);
            config
        },
    )
    .await;

    tc.client.connect().await.unwrap();
    let _ = drain_until(
        &mut tc.rx,
        |e| matches!(e, ExecutionEvent::Account(_)),
        "initial account",
    )
    .await;

    let order = if order_type == OrderType::StopMarket {
        build_stop_market_order(
            instrument_id,
            client_order_id,
            OrderSide::Buy,
            Price::from("3600.00"),
            Quantity::from("1.000"),
        )
    } else {
        build_limit_order(
            instrument_id,
            client_order_id,
            OrderSide::Buy,
            Price::from("3500.00"),
            Quantity::from("1.000"),
        )
    };

    tc.cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .unwrap();

    if replace {
        tc.client.submit_order(submit_cmd(&order)).unwrap();
        let _ = drain_until(
            &mut tc.rx,
            |e| matches!(e, ExecutionEvent::Order(OrderEventAny::Accepted(_))),
            "original accepted",
        )
        .await;
    }

    *ws_state.write_reply_delay.lock().await = Duration::from_millis(1600);

    if replace {
        tc.client
            .modify_order(ModifyOrder::new(
                TraderId::from("TRADER-001"),
                Some(ClientId::from("DERIVE")),
                StrategyId::from("S-1"),
                instrument_id,
                client_order_id,
                Some(VenueOrderId::from("ord-original")),
                Some(Quantity::from("2.000")),
                Some(Price::from("3505.00")),
                None,
                UUID4::new(),
                UnixNanos::default(),
                None,
                None,
            ))
            .unwrap();
    } else {
        tc.client.submit_order(submit_cmd(&order)).unwrap();
        let _ = drain_until(
            &mut tc.rx,
            |e| matches!(e, ExecutionEvent::Order(OrderEventAny::Submitted(_))),
            "submitted",
        )
        .await;
    }

    let writes = match (replace, order_type) {
        (true, _) => ws_state.replace_orders.clone(),
        (false, OrderType::StopMarket) => ws_state.submitted_trigger_orders.clone(),
        _ => ws_state.submitted_orders.clone(),
    };

    wait_until(
        || {
            let writes = writes.clone();
            async move { !writes.lock().await.is_empty() }
        },
        "write posted",
    )
    .await;

    assert!(
        tokio::time::timeout(Duration::from_millis(1250), tc.rx.recv())
            .await
            .is_err()
    );

    if replace {
        ws_state.push_notification(make_subscription_frame(
            &format!("{TEST_SUBACCOUNT}.orders"),
            &json!([order_json_with(
                "ord-original",
                client_order_id.as_str(),
                "buy",
                "ETH-PERP",
                1_700_000_003_000,
                "cancelled"
            )]),
        ));
    }

    let venue_order_id = if replace {
        "ord-replacement-late"
    } else {
        "ord-submit-late"
    };

    let trade = trade_json_with_label(
        "timeout-fill-1",
        venue_order_id,
        "ETH-PERP",
        client_order_id.as_str(),
    );
    ws_state.push_notification(make_subscription_frame(
        &format!("{TEST_SUBACCOUNT}.trades"),
        &json!([trade]),
    ));

    if replace {
        let orders_channel = format!("{TEST_SUBACCOUNT}.orders");
        ws_state.push_notification(make_subscription_frame(
            &orders_channel,
            &json!([order_json_with(
                "late-authority-barrier",
                "EXTERNAL-LATE",
                "buy",
                "ETH-PERP",
                1_700_000_004_000,
                "open"
            ),]),
        ));

        let barrier = drain_until(
            &mut tc.rx,
            |event| {
                matches!(
                    event,
                    ExecutionEvent::Order(_) | ExecutionEvent::Report(ExecutionReport::Order(_))
                )
            },
            "trade remains deferred before native successor authority",
        )
        .await;

        let ExecutionEvent::Report(ExecutionReport::Order(barrier)) = barrier else {
            panic!("trade alone cannot authorize a replacement binding: {barrier:?}");
        };

        assert_eq!(
            barrier.venue_order_id,
            VenueOrderId::from("late-authority-barrier")
        );
        let mut successor = order_json_with(
            venue_order_id,
            client_order_id.as_str(),
            "buy",
            "ETH-PERP",
            1_700_000_004_001,
            "open",
        );
        successor["amount"] = json!("2.000");
        successor["replaced_order_id"] = json!("ord-original");
        successor["nonce"] = ws_state.replace_orders.lock().await[0]["nonce"].clone();
        ws_state.push_notification(make_subscription_frame(
            &orders_channel,
            &json!([successor]),
        ));
    }

    let first = drain_until(
        &mut tc.rx,
        |e| matches!(e, ExecutionEvent::Order(_)),
        "late tracked update",
    )
    .await;

    match (replace, first) {
        (true, ExecutionEvent::Order(OrderEventAny::Updated(updated))) => {
            assert_eq!(
                updated.venue_order_id,
                Some(VenueOrderId::from(venue_order_id))
            );
            assert_eq!(updated.quantity, Quantity::from("2.000"));
            assert_eq!(updated.price, Some(Price::from("3505.00")));
        }
        (false, ExecutionEvent::Order(OrderEventAny::Accepted(accepted))) => {
            assert_eq!(accepted.venue_order_id, VenueOrderId::from(venue_order_id));
            assert_eq!(accepted.client_order_id, client_order_id);
        }
        (replace, event) => {
            panic!("expected matching late write acknowledgement (replace={replace}): {event:?}")
        }
    }

    let fill = drain_until(
        &mut tc.rx,
        |e| matches!(e, ExecutionEvent::Order(_)),
        "late tracked fill",
    )
    .await;

    let ExecutionEvent::Order(OrderEventAny::Filled(fill)) = fill else {
        panic!("expected fill: {fill:?}")
    };

    assert_eq!(fill.client_order_id, client_order_id);
    assert_eq!(fill.venue_order_id, VenueOrderId::from(venue_order_id));
    assert_eq!(fill.order_type, order_type);
    assert_eq!(fill.trade_id, TradeId::from("timeout-fill-1"));
    assert_eq!(fill.last_qty, Quantity::from("1.000"));
    assert_eq!(fill.last_px, Price::from("3505.00"));
    assert_eq!(
        ws_state.replace_orders.lock().await.len(),
        usize::from(replace)
    );
    assert_eq!(
        ws_state.submitted_trigger_orders.lock().await.len(),
        usize::from(order_type == OrderType::StopMarket)
    );
    assert_eq!(
        ws_state.submitted_orders.lock().await.len(),
        usize::from(order_type == OrderType::Limit)
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(150), tc.rx.recv())
            .await
            .is_err()
    );
    tc.client.disconnect().await.unwrap();
}

#[rstest]
#[case(OrderType::Limit)]
#[case(OrderType::StopMarket)]
#[tokio::test]
async fn test_submit_private_rejection_before_response_emits_once(
    #[case] order_type: OrderType,
    #[values(false, true)] formatted_reason: bool,
) {
    let client_order_id = ClientOrderId::from("STRAT-EARLY-REJECT");
    let instrument_id = InstrumentId::from("ETH-PERP.DERIVE");
    let ws_state = WsState::default();
    let error = json!({"error": {"code": 11040, "message": "post only rejected"}});
    *ws_state.order_reply.lock().await = Some(error.clone());
    *ws_state.trigger_order_reply.lock().await = Some(error);
    let mut notification = order_json_with(
        "ord-rejected",
        client_order_id.as_str(),
        "buy",
        "ETH-PERP",
        1_700_000_001_000,
        "rejected",
    );

    if formatted_reason {
        notification["trigger_reject_message"] =
            json!(format!("  <p>risk\t limit</p>\0 \n{}", "x".repeat(400)));
    }

    *ws_state.order_notification_before_reply.lock().await = Some(make_subscription_frame(
        &format!("{TEST_SUBACCOUNT}.orders"),
        &json!([notification]),
    ));
    let mut tc = build_client(RestState::default(), ws_state.clone()).await;
    tc.client.connect().await.unwrap();

    let order = if order_type == OrderType::StopMarket {
        build_stop_market_order(
            instrument_id,
            client_order_id,
            OrderSide::Buy,
            Price::from("3600.00"),
            Quantity::from("1.000"),
        )
    } else {
        build_limit_order(
            instrument_id,
            client_order_id,
            OrderSide::Buy,
            Price::from("3500.00"),
            Quantity::from("1.000"),
        )
    };

    tc.cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .unwrap();
    tc.client.submit_order(submit_cmd(&order)).unwrap();
    let _ = drain_until(
        &mut tc.rx,
        |e| matches!(e, ExecutionEvent::Order(OrderEventAny::Submitted(_))),
        "submitted",
    )
    .await;
    let event = drain_until(
        &mut tc.rx,
        |e| matches!(e, ExecutionEvent::Order(_)),
        "single rejection",
    )
    .await;

    let ExecutionEvent::Order(OrderEventAny::Rejected(rejected)) = event else {
        panic!("expected rejection: {event:?}")
    };

    assert_eq!(rejected.client_order_id, client_order_id);
    assert_eq!(rejected.instrument_id, instrument_id);
    assert_eq!(rejected.strategy_id, StrategyId::from("S-1"));

    let expected = if formatted_reason {
        format!("risk limit {}", "x".repeat(245))
    } else {
        "Order rejected by Derive".to_string()
    };

    assert_eq!(rejected.reason, Ustr::from(&expected));
    assert_eq!(rejected.trader_id, TraderId::from("TRADER-001"));
    assert_eq!(rejected.account_id, AccountId::from("DERIVE-001"));
    assert_eq!(
        rejected.ts_event,
        UnixNanos::from(1_700_000_001_000_000_000)
    );
    assert!(!rejected.reconciliation);
    assert!(!rejected.due_post_only);
    assert_eq!(rejected.causation_id, None);
    assert!(
        tokio::time::timeout(Duration::from_millis(250), tc.rx.recv())
            .await
            .is_err()
    );
    tc.client.disconnect().await.unwrap();
}

#[rstest]
#[case(OrderType::Limit, TimeInForce::Gtc)]
#[case(OrderType::StopLimit, TimeInForce::Gtc)]
#[case(OrderType::LimitIfTouched, TimeInForce::Gtc)]
#[tokio::test]
async fn test_submit_resting_reduce_only_limit_is_denied_locally(
    #[case] order_type: OrderType,
    #[case] time_in_force: TimeInForce,
) {
    let ws_state = WsState::default();
    let mut tc = build_client(RestState::default(), ws_state.clone()).await;
    tc.client.connect().await.unwrap();
    let mut builder = OrderTestBuilder::new(order_type);
    builder
        .trader_id(TraderId::from("TRADER-001"))
        .strategy_id(StrategyId::from("S-1"))
        .instrument_id(InstrumentId::from("ETH-PERP.DERIVE"))
        .client_order_id(ClientOrderId::from("STRAT-RESTING-REDUCE"))
        .side(OrderSide::Sell)
        .quantity(Quantity::from("1.000"))
        .price(Price::from("3500.00"))
        .time_in_force(time_in_force)
        .reduce_only(true);

    if order_type != OrderType::Limit {
        builder
            .trigger_price(Price::from("3600.00"))
            .trigger_type(TriggerType::MarkPrice);
    }

    let order = builder.build();
    tc.cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .unwrap();
    tc.client.submit_order(submit_cmd(&order)).unwrap();
    let event = drain_until(
        &mut tc.rx,
        |e| matches!(e, ExecutionEvent::Order(_)),
        "local denial",
    )
    .await;

    let ExecutionEvent::Order(OrderEventAny::Denied(denied)) = event else {
        panic!("expected denial: {event:?}")
    };

    assert_eq!(denied.client_order_id, order.client_order_id());
    assert_eq!(
        denied.reason,
        Ustr::from(
            "VALIDATION_FAILED: reduce-only Derive limit orders require IOC or FOK time-in-force"
        )
    );
    assert_eq!(ws_state.submitted_orders.lock().await.len(), 0);
    assert_eq!(ws_state.submitted_trigger_orders.lock().await.len(), 0);
    tc.client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_ambiguous_replace_does_not_hide_original_expiry() {
    let ws_state = WsState::default();
    let client_order_id = ClientOrderId::from("STRAT-REPLACE-EXPIRE");
    let instrument_id = InstrumentId::from("ETH-PERP.DERIVE");
    let original = order_json_with(
        "ord-expiring",
        client_order_id.as_str(),
        "buy",
        "ETH-PERP",
        1_700_000_001_000,
        "open",
    );
    *ws_state.order_reply.lock().await =
        Some(json!({"result": {"order": original.clone(), "trades": []}}));
    *ws_state.replace_reply.lock().await =
        Some(json!({"error": {"code": -32603, "message": "Internal venue error"}}));
    let mut tc = build_client(RestState::default(), ws_state.clone()).await;
    tc.client.connect().await.unwrap();
    let mut order = build_limit_order(
        instrument_id,
        client_order_id,
        OrderSide::Buy,
        Price::from("3500.00"),
        Quantity::from("1.000"),
    );
    tc.cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .unwrap();
    tc.client.submit_order(submit_cmd(&order)).unwrap();

    for expected in [OrderEventType::Submitted, OrderEventType::Accepted] {
        let event = drain_until(
            &mut tc.rx,
            |e| matches!(e, ExecutionEvent::Order(_)),
            "original lifecycle",
        )
        .await;

        let ExecutionEvent::Order(event) = event else {
            unreachable!()
        };

        assert_eq!(event.event_type(), expected);
        order.apply(event).unwrap();
    }

    tc.client
        .modify_order(ModifyOrder::new(
            TraderId::from("TRADER-001"),
            Some(ClientId::from("DERIVE")),
            StrategyId::from("S-1"),
            instrument_id,
            client_order_id,
            Some(VenueOrderId::from("ord-expiring")),
            Some(Quantity::from("2.000")),
            Some(Price::from("3505.00")),
            None,
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ))
        .unwrap();
    wait_until(
        || {
            let state = ws_state.clone();
            async move { !state.replace_orders.lock().await.is_empty() }
        },
        "replace posted",
    )
    .await;

    assert!(
        tokio::time::timeout(Duration::from_millis(200), tc.rx.recv())
            .await
            .is_err()
    );
    ws_state.push_notification(make_subscription_frame(
        &format!("{TEST_SUBACCOUNT}.orders"),
        &json!([original]),
    ));
    assert!(
        tokio::time::timeout(Duration::from_millis(200), tc.rx.recv())
            .await
            .is_err()
    );
    let expired = order_json_with(
        "ord-expiring",
        client_order_id.as_str(),
        "buy",
        "ETH-PERP",
        1_700_000_003_000,
        "expired",
    );
    ws_state.push_notification(make_subscription_frame(
        &format!("{TEST_SUBACCOUNT}.orders"),
        &json!([expired]),
    ));
    let event = drain_until(
        &mut tc.rx,
        |e| matches!(e, ExecutionEvent::Order(_)),
        "original expiry",
    )
    .await;

    let ExecutionEvent::Order(OrderEventAny::Expired(expired)) = event else {
        panic!("expected expiry: {event:?}")
    };

    assert_eq!(expired.client_order_id, client_order_id);
    assert_eq!(
        expired.venue_order_id,
        Some(VenueOrderId::from("ord-expiring"))
    );
    assert_eq!(expired.instrument_id, instrument_id);
    assert_eq!(expired.strategy_id, StrategyId::from("S-1"));
    assert_eq!(expired.account_id, Some(AccountId::from("DERIVE-001")));
    assert_eq!(expired.ts_event, UnixNanos::from(1_700_000_003_000_000_000));
    order.apply(OrderEventAny::Expired(expired)).unwrap();
    assert_eq!(order.status(), OrderStatus::Expired);
    assert_eq!(order.quantity(), Quantity::from("1.000"));
    assert_eq!(order.price(), Some(Price::from("3500.00")));
    assert_eq!(ws_state.replace_orders.lock().await.len(), 1);
    assert!(
        tokio::time::timeout(Duration::from_millis(100), tc.rx.recv())
            .await
            .is_err()
    );
    tc.client.disconnect().await.unwrap();
}

#[rstest]
#[case(" ")]
#[case("external-\u{03bb}")]
#[tokio::test]
async fn test_ws_identity_errors_preserve_later_order_and_fill_reports(#[case] label: &str) {
    let ws_state = WsState::default();
    let mut tc = build_client(RestState::default(), ws_state.clone()).await;
    tc.client.connect().await.expect("connect");
    drain_initial_account_state(&mut tc).await;
    wait_until(
        || {
            let state = ws_state.clone();
            async move { !state.subscribe_frames.lock().await.is_empty() }
        },
        "subscribe acknowledged",
    )
    .await;

    let orders = json!([
        order_json_with(
            " ",
            "BAD-ORDER",
            "buy",
            "ETH-PERP",
            1_700_000_001_000,
            "open"
        ),
        order_json_with(
            "ord-label-omitted",
            label,
            "sell",
            "ETH-PERP",
            1_700_000_002_000,
            "open"
        ),
        order_json_with(
            "ord-valid-after",
            "VALID-AFTER",
            "buy",
            "ETH-PERP",
            1_700_000_003_000,
            "open"
        ),
    ]);
    ws_state.push_notification(make_subscription_frame(
        &format!("{TEST_SUBACCOUNT}.orders"),
        &orders,
    ));

    for (order_id, expected_label, side) in [
        ("ord-label-omitted", None, OrderSide::Sell),
        (
            "ord-valid-after",
            Some(ClientOrderId::from("VALID-AFTER")),
            OrderSide::Buy,
        ),
    ] {
        let event = drain_until(
            &mut tc.rx,
            |e| matches!(e, ExecutionEvent::Report(_)),
            "order report",
        )
        .await;

        let ExecutionEvent::Report(ExecutionReport::Order(report)) = event else {
            panic!("expected order report, was {event:?}");
        };

        assert_eq!(report.account_id, AccountId::from("DERIVE-001"));
        assert_eq!(report.instrument_id, InstrumentId::from("ETH-PERP.DERIVE"));
        assert_eq!(report.venue_order_id, VenueOrderId::from(order_id));
        assert_eq!(report.client_order_id, expected_label);
        assert_eq!(report.order_side, Some(side));
        assert_eq!(report.order_status, OrderStatus::Accepted);
        assert_eq!(report.quantity.as_decimal(), dec!(1));
        assert_eq!(report.filled_qty.as_decimal(), dec!(0));
        assert_eq!(report.price.unwrap().as_decimal(), dec!(3500));
    }

    let trades = json!([
        trade_json_with_label(" ", "ord-bad-trade", "ETH-PERP", "BAD-TRADE"),
        trade_json_with_label(
            "trade-bad-order",
            "external-\u{03bb}",
            "ETH-PERP",
            "BAD-TRADE"
        ),
        trade_json_with_label(
            "trade-label-omitted",
            "ord-label-omitted",
            "ETH-PERP",
            label
        ),
        trade_json_with_label(
            "trade-valid-after",
            "ord-valid-after",
            "ETH-PERP",
            "VALID-AFTER"
        ),
    ]);
    let channel = format!("{TEST_SUBACCOUNT}.trades");
    ws_state.push_notification(make_subscription_frame(&channel, &trades));

    for (trade_id, order_id, expected_label) in [
        ("trade-label-omitted", "ord-label-omitted", None),
        (
            "trade-valid-after",
            "ord-valid-after",
            Some(ClientOrderId::from("VALID-AFTER")),
        ),
    ] {
        let event = drain_until(
            &mut tc.rx,
            |e| matches!(e, ExecutionEvent::Report(_)),
            "fill report",
        )
        .await;

        let ExecutionEvent::Report(ExecutionReport::Fill(report)) = event else {
            panic!("expected fill report, was {event:?}");
        };

        assert_eq!(report.account_id, AccountId::from("DERIVE-001"));
        assert_eq!(report.instrument_id, InstrumentId::from("ETH-PERP.DERIVE"));
        assert_eq!(report.trade_id, TradeId::from(trade_id));
        assert_eq!(report.venue_order_id, VenueOrderId::from(order_id));
        assert_eq!(report.client_order_id, expected_label);
        assert_eq!(report.order_side, OrderSide::Buy);
        assert_eq!(report.last_qty.as_decimal(), dec!(1));
        assert_eq!(report.last_px.as_decimal(), dec!(3505));
        assert_eq!(report.commission.as_decimal(), dec!(0.5));
        assert_eq!(report.commission.currency, Currency::USDC());
    }

    ws_state.push_notification(make_subscription_frame(&channel, &trades));
    ws_state.push_notification(make_subscription_frame(
        &channel,
        &json!([trade_json_with_label(
            "trade-later-frame",
            "ord-later-frame",
            "ETH-PERP",
            "LATER-FRAME"
        )]),
    ));
    let event = drain_until(
        &mut tc.rx,
        |e| matches!(e, ExecutionEvent::Report(_)),
        "later frame fill",
    )
    .await;

    let ExecutionEvent::Report(ExecutionReport::Fill(report)) = event else {
        panic!("expected fill report, was {event:?}");
    };

    assert_eq!(report.trade_id, TradeId::from("trade-later-frame"));
    assert_eq!(report.venue_order_id, VenueOrderId::from("ord-later-frame"));
    assert_eq!(
        report.client_order_id,
        Some(ClientOrderId::from("LATER-FRAME"))
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(250), tc.rx.recv())
            .await
            .is_err()
    );
    tc.client
        .disconnect()
        .await
        .expect("no task or dispatcher panic");
}

#[rstest]
#[tokio::test]
async fn test_private_history_identity_errors_preserve_valid_rows_and_pages() {
    let rest_state = RestState::default();
    *rest_state.open_orders_response.lock().await =
        json!({"orders": [], "subaccount_id": TEST_SUBACCOUNT});
    *rest_state.order_history_pages.lock().await = vec![
        json!({
            "orders": [
                order_json_with("external-\u{03bb}", "BAD-ORDER", "buy", "ETH-PERP", 1_700_000_001_000, "open"),
                order_json_with("ord-history-1", " ", "sell", "ETH-PERP", 1_700_000_002_000, "open"),
            ],
            "pagination": {"count": 3, "num_pages": 2}, "subaccount_id": TEST_SUBACCOUNT,
        }),
        json!({
            "orders": [order_json_with("ord-history-2", "HISTORY-2", "buy", "ETH-PERP", 1_700_000_003_000, "open")],
            "pagination": {"count": 3, "num_pages": 2}, "subaccount_id": TEST_SUBACCOUNT,
        }),
    ];
    *rest_state.trade_history_pages.lock().await = vec![
        json!({
            "trades": [
                trade_json_with_label("external-\u{03bb}", "ord-bad", "ETH-PERP", "BAD"),
                trade_json_with_label("trade-bad-order", " ", "ETH-PERP", "BAD"),
                trade_json_with_label("trade-history-1", "ord-history-1", "ETH-PERP", "external-\u{03bb}"),
            ],
            "pagination": {"count": 4, "num_pages": 2}, "subaccount_id": TEST_SUBACCOUNT,
        }),
        json!({
            "trades": [trade_json_with_label("trade-history-2", "ord-history-2", "ETH-PERP", "HISTORY-2")],
            "pagination": {"count": 4, "num_pages": 2}, "subaccount_id": TEST_SUBACCOUNT,
        }),
    ];
    let mut tc = build_report_client(rest_state.clone(), WsState::default()).await;
    tc.client.connect().await.expect("connect");
    let mass = tc
        .client
        .generate_mass_status(Some(10))
        .await
        .expect("mass status preserves valid rows")
        .unwrap();
    assert!(!mass.reports_complete());
    let orders: Vec<_> = mass.order_reports().values().cloned().collect();
    let fills: Vec<_> = mass.fill_reports().values().flatten().cloned().collect();
    assert_eq!(orders.len(), 2);
    assert_eq!(fills.len(), 2);

    for (index, label) in [None, Some(ClientOrderId::from("HISTORY-2"))]
        .into_iter()
        .enumerate()
    {
        let expected_order_id = VenueOrderId::from(format!("ord-history-{}", index + 1));
        assert_eq!(orders[index].venue_order_id, expected_order_id);
        assert_eq!(orders[index].client_order_id, label);
        assert_eq!(orders[index].quantity.as_decimal(), dec!(1));
        assert_eq!(orders[index].filled_qty.as_decimal(), dec!(0));
        assert_eq!(fills[index].venue_order_id, expected_order_id);
        assert_eq!(
            fills[index].trade_id,
            TradeId::from(format!("trade-history-{}", index + 1))
        );
        assert_eq!(fills[index].client_order_id, label);
        assert_eq!(fills[index].last_qty.as_decimal(), dec!(1));
        assert_eq!(fills[index].last_px.as_decimal(), dec!(3505));
        assert_eq!(fills[index].commission.as_decimal(), dec!(0.5));
        assert_eq!(fills[index].commission.currency, Currency::USDC());
    }

    assert_eq!(rest_state.order_history_calls.lock().await.len(), 2);
    assert_eq!(rest_state.trade_history_calls.lock().await.len(), 2);
    tc.client.disconnect().await.expect("disconnect");
}

#[rstest]
#[case("blank_id")]
#[case("unicode_id")]
#[case("account")]
#[case("instrument")]
#[case("label")]
#[case("different_id")]
#[tokio::test]
async fn test_cancel_trigger_invalid_response_identity_retains_identity_for_private_cancel(
    #[case] invalid_field: &str,
) {
    let ws_state = WsState::default();
    let instrument_id = InstrumentId::from("ETH-PERP.DERIVE");
    let client_order_id = ClientOrderId::from("STRAT-CANCEL-BAD-ID");
    let venue_order_id = VenueOrderId::from("trig-mock-1");
    let mut response = trigger_order_json_with(
        venue_order_id.as_str(),
        client_order_id.as_str(),
        "buy",
        "ETH-PERP",
        1_700_000_002_000,
        "market",
        "cancelled",
        "3500",
        "3400",
        "mark",
        "stoploss",
    );

    match invalid_field {
        "blank_id" => response["order_id"] = json!(" "),
        "unicode_id" => response["order_id"] = json!("external-\u{03bb}"),
        "account" => response["subaccount_id"] = json!(TEST_SUBACCOUNT + 1),
        "instrument" => response["instrument_name"] = json!("BTC-PERP"),
        "label" => response["label"] = json!("FOREIGN-LABEL"),
        "different_id" => response["order_id"] = json!("different-native-order"),
        _ => unreachable!(),
    }

    *ws_state.cancel_trigger_reply.lock().await = Some(json!({"result": response}));
    let mut tc = build_client(RestState::default(), ws_state.clone()).await;
    tc.client.connect().await.expect("connect");
    wait_for_private_subscription(&ws_state).await;
    drain_initial_account_state(&mut tc).await;
    let order = build_stop_market_order(
        instrument_id,
        client_order_id,
        OrderSide::Buy,
        Price::from("3400.00"),
        Quantity::from("1.000"),
    );
    submit_cached_order(&tc, &order);
    let event = drain_until(
        &mut tc.rx,
        |e| matches!(e, ExecutionEvent::Order(OrderEventAny::Accepted(_))),
        "trigger accepted",
    )
    .await;

    let ExecutionEvent::Order(OrderEventAny::Accepted(accepted)) = event else {
        panic!("expected acceptance, was {event:?}");
    };

    assert_eq!(accepted.client_order_id, client_order_id);
    assert_eq!(accepted.venue_order_id, venue_order_id);
    tc.client
        .cancel_order(CancelOrder::new(
            TraderId::from("TRADER-001"),
            Some(ClientId::from("DERIVE")),
            StrategyId::from("S-1"),
            instrument_id,
            client_order_id,
            Some(venue_order_id),
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ))
        .expect("cancel admitted");
    wait_until(
        || {
            let state = ws_state.clone();
            async move { !state.cancelled_trigger_orders.lock().await.is_empty() }
        },
        "trigger cancel posted",
    )
    .await;

    assert!(
        tokio::time::timeout(Duration::from_millis(300), tc.rx.recv())
            .await
            .is_err(),
        "invalid cancellation identity must not emit a completion or rejection"
    );
    ws_state.push_notification(make_subscription_frame(
        &format!("{TEST_SUBACCOUNT}.orders"),
        &json!([trigger_order_json_with(
            venue_order_id.as_str(),
            client_order_id.as_str(),
            "buy",
            "ETH-PERP",
            1_700_000_003_000,
            "market",
            "cancelled",
            "3500",
            "3400",
            "mark",
            "stoploss"
        )]),
    ));
    let event = drain_until(
        &mut tc.rx,
        |e| matches!(e, ExecutionEvent::Order(_)),
        "native cancellation",
    )
    .await;

    let ExecutionEvent::Order(OrderEventAny::Canceled(canceled)) = event else {
        panic!("expected tracked cancellation, was {event:?}");
    };

    assert_eq!(canceled.client_order_id, client_order_id);
    assert_eq!(canceled.instrument_id, instrument_id);
    assert_eq!(canceled.venue_order_id, Some(venue_order_id));
    assert!(
        tokio::time::timeout(Duration::from_millis(250), tc.rx.recv())
            .await
            .is_err()
    );
    tc.client
        .disconnect()
        .await
        .expect("malformed cancellation must not panic its task");
}

#[rstest]
#[case("nonce")]
#[case("nonce_missing")]
#[case("nonce_malformed")]
#[case("blank_id")]
#[case("unicode_id")]
#[case("account")]
#[case("instrument")]
#[case("cancelled_account")]
#[case("cancelled_instrument")]
#[case("cancelled_label")]
#[case("successful_cancelled_account")]
#[case("successful_cancelled_instrument")]
#[case("successful_cancelled_label")]
#[tokio::test]
async fn test_replace_invalid_response_identity_retains_pending_modify_for_private_update(
    #[case] invalid_field: &str,
) {
    let ws_state = WsState::default();
    let instrument_id = InstrumentId::from("ETH-PERP.DERIVE");
    let client_order_id = ClientOrderId::from("STRAT-REPLACE-BAD-ID");
    let old_venue_order_id = VenueOrderId::from("ord-before-invalid");
    let new_venue_order_id = VenueOrderId::from("ord-after-invalid");
    *ws_state.order_reply.lock().await = Some(json!({"result": {"order": order_json_with(
        old_venue_order_id.as_str(), client_order_id.as_str(), "buy", "ETH-PERP", 1_700_000_001_000, "open"
    )}}));
    let mut replacement = order_json_with(
        new_venue_order_id.as_str(),
        client_order_id.as_str(),
        "buy",
        "ETH-PERP",
        1_700_000_002_000,
        "open",
    );
    let mut cancelled = order_json_with(
        old_venue_order_id.as_str(),
        client_order_id.as_str(),
        "buy",
        "ETH-PERP",
        1_700_000_002_000,
        "cancelled",
    );

    match invalid_field {
        "nonce" => replacement["nonce"] = json!("987"),
        "nonce_missing" => {
            replacement.as_object_mut().unwrap().remove("nonce");
        }
        "nonce_malformed" => replacement["nonce"] = json!("invalid-nonce"),
        "blank_id" => replacement["order_id"] = json!(" "),
        "unicode_id" => replacement["order_id"] = json!("external-\u{03bb}"),
        "account" => replacement["subaccount_id"] = json!(TEST_SUBACCOUNT + 1),
        "instrument" => replacement["instrument_name"] = json!("BTC-PERP"),
        "cancelled_account" | "successful_cancelled_account" => {
            cancelled["subaccount_id"] = json!(TEST_SUBACCOUNT + 1);
        }
        "cancelled_instrument" | "successful_cancelled_instrument" => {
            cancelled["instrument_name"] = json!("BTC-PERP");
        }
        "cancelled_label" | "successful_cancelled_label" => {
            cancelled["label"] = json!("FOREIGN-LABEL");
        }
        _ => unreachable!(),
    }

    *ws_state.replace_reply.lock().await = Some(if invalid_field.starts_with("cancelled_") {
        json!({"result": {"cancelled_order": cancelled, "create_order_error": {"code": 11008, "message": "Post only order would cross"}}})
    } else {
        json!({"result": {"order": replacement, "cancelled_order": cancelled}})
    });

    let mut tc = build_client(RestState::default(), ws_state.clone()).await;
    tc.client.connect().await.expect("connect");
    wait_for_private_subscription(&ws_state).await;
    drain_initial_account_state(&mut tc).await;
    let order = build_limit_order(
        instrument_id,
        client_order_id,
        OrderSide::Buy,
        Price::from("3500.00"),
        Quantity::from("1.000"),
    );
    submit_cached_order(&tc, &order);
    let event = drain_until(
        &mut tc.rx,
        |e| matches!(e, ExecutionEvent::Order(OrderEventAny::Accepted(_))),
        "original order accepted",
    )
    .await;

    let ExecutionEvent::Order(OrderEventAny::Accepted(accepted)) = event else {
        panic!("expected acceptance, was {event:?}");
    };

    assert_eq!(accepted.venue_order_id, old_venue_order_id);
    tc.client
        .modify_order(ModifyOrder::new(
            TraderId::from("TRADER-001"),
            Some(ClientId::from("DERIVE")),
            StrategyId::from("S-1"),
            instrument_id,
            client_order_id,
            Some(old_venue_order_id),
            Some(Quantity::from("2.000")),
            Some(Price::from("3505.00")),
            None,
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ))
        .expect("modify admitted");
    wait_until(
        || {
            let state = ws_state.clone();
            async move { !state.replace_orders.lock().await.is_empty() }
        },
        "replace posted",
    )
    .await;

    assert!(
        tokio::time::timeout(Duration::from_millis(300), tc.rx.recv())
            .await
            .is_err(),
        "invalid replacement identity must not emit a completion or rejection"
    );
    let mut replacement = order_json_with(
        new_venue_order_id.as_str(),
        client_order_id.as_str(),
        "buy",
        "ETH-PERP",
        1_700_000_003_000,
        "open",
    );
    replacement["amount"] = json!("2.000");
    replacement["limit_price"] = json!("3505.00");
    replacement["replaced_order_id"] = json!(old_venue_order_id.as_str());
    replacement["nonce"] = ws_state.replace_orders.lock().await[0]["nonce"].clone();
    ws_state.push_notification(make_subscription_frame(
        &format!("{TEST_SUBACCOUNT}.orders"),
        &json!([replacement]),
    ));
    let event = drain_until(
        &mut tc.rx,
        |e| matches!(e, ExecutionEvent::Order(_)),
        "native replacement",
    )
    .await;

    let ExecutionEvent::Order(OrderEventAny::Updated(updated)) = event else {
        panic!("expected tracked update, was {event:?}");
    };

    assert_eq!(updated.client_order_id, client_order_id);
    assert_eq!(updated.instrument_id, instrument_id);
    assert_eq!(updated.venue_order_id, Some(new_venue_order_id));
    assert_eq!(updated.quantity, Quantity::from("2.000"));
    assert_eq!(updated.price, Some(Price::from("3505.00")));
    assert!(
        tokio::time::timeout(Duration::from_millis(250), tc.rx.recv())
            .await
            .is_err()
    );
    tc.client
        .disconnect()
        .await
        .expect("malformed replacement must not panic its task");
}

#[rstest]
#[case::direct_label(0)]
#[case::direct_old_id(1)]
#[case::bulk_history(2)]
#[case::fill_history(3)]
#[case::mass_status(4)]
#[case::direct_missing_label(5)]
#[case::unparsable_history_child(6)]
#[case::cancel_unresolved(7)]
#[case::modify_unresolved(8)]
#[case::cancel_all_unresolved(9)]
#[case::batch_cancel_unresolved(10)]
#[case::restored_pending_without_child(11)]
#[case::restored_pending_before_old_expiry(12)]
#[tokio::test]
async fn test_reports_defer_unproved_tracked_replacement_binding(#[case] operation: u8) {
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    let cid = ClientOrderId::from("COLD-BINDING");
    let instrument_id = InstrumentId::from("ETH-PERP.DERIVE");
    let old_id = VenueOrderId::from("cold-old");
    let old = order_json_with(
        old_id.as_str(),
        cid.as_str(),
        "buy",
        "ETH-PERP",
        1_700_000_001_000,
        "cancelled",
    );
    let child = order_json_with(
        "cold-unproved-child",
        cid.as_str(),
        "buy",
        "ETH-PERP",
        1_700_000_002_000,
        "open",
    );
    *rest_state.open_orders_response.lock().await =
        json!({"orders": [child.clone()], "subaccount_id": TEST_SUBACCOUNT});
    let mut terminal_child = child.clone();
    terminal_child["order_status"] = json!("filled");
    terminal_child["filled_amount"] = json!("1");
    *rest_state.order_history_response.lock().await = json!({"orders": [old.clone(), terminal_child], "pagination": {"count": 2, "num_pages": 1}, "subaccount_id": TEST_SUBACCOUNT});
    rest_state
        .get_order_responses
        .lock()
        .await
        .insert(old_id.to_string(), old);
    *rest_state.trade_history_response.lock().await = json!({"trades": [trade_json_with_label("cold-child-trade", "cold-unproved-child", "ETH-PERP", cid.as_str())], "pagination": {"count": 1, "num_pages": 1}, "subaccount_id": TEST_SUBACCOUNT});

    if operation == 5 {
        let mut unlabeled = order_json_with(
            "cold-unproved-child",
            "",
            "buy",
            "ETH-PERP",
            1_700_000_002_000,
            "open",
        );
        unlabeled["label"] = json!("");
        rest_state
            .get_order_responses
            .lock()
            .await
            .insert("cold-unproved-child".to_string(), unlabeled);
    }

    if operation == 6 {
        *rest_state.open_orders_response.lock().await =
            json!({"orders": [], "subaccount_id": TEST_SUBACCOUNT});
        let mut history = rest_state.order_history_response.lock().await;
        history["orders"][1]["order_status"] = json!("future-unknown-status");
    }

    let mut tc = build_report_client(rest_state.clone(), ws_state.clone()).await;
    tc.client.connect().await.unwrap();
    let mut restored = accepted_order(
        build_limit_order(
            instrument_id,
            cid,
            OrderSide::Buy,
            Price::from("3500.00"),
            Quantity::from("1.000"),
        ),
        old_id,
        AccountId::from("DERIVE-001"),
    );

    if operation >= 11 {
        restored
            .apply(OrderEventAny::PendingUpdate(OrderPendingUpdate::new(
                restored.trader_id(),
                restored.strategy_id(),
                instrument_id,
                cid,
                restored.account_id(),
                UUID4::new(),
                UnixNanos::from(2),
                UnixNanos::from(2),
                false,
                Some(old_id),
            )))
            .unwrap();
        *rest_state.open_orders_response.lock().await =
            json!({"orders": [], "subaccount_id": TEST_SUBACCOUNT});
        *rest_state.order_history_response.lock().await = json!({"orders": [], "pagination": {"count": 0, "num_pages": 0}, "subaccount_id": TEST_SUBACCOUNT});
        *rest_state.trade_history_response.lock().await = json!({"trades": [], "pagination": {"count": 0, "num_pages": 0}, "subaccount_id": TEST_SUBACCOUNT});
    }

    add_order_to_cache(&tc.cache, restored, Some(ClientId::from("DERIVE")));
    tc.client.start().unwrap();
    let result = match operation {
        0 | 1 | 5 => tc
            .client
            .generate_order_status_report(&GenerateOrderStatusReport::new(
                UUID4::new(),
                UnixNanos::default(),
                Some(instrument_id),
                Some(cid),
                (operation == 1).then_some(old_id).or_else(|| {
                    (operation == 5).then_some(VenueOrderId::from("cold-unproved-child"))
                }),
                None,
                None,
            ))
            .await
            .map(|_| ()),
        2 | 6 => tc
            .client
            .generate_order_status_reports(&GenerateOrderStatusReports::new(
                UUID4::new(),
                UnixNanos::default(),
                false,
                Some(instrument_id),
                None,
                None,
                None,
                None,
            ))
            .await
            .map(|_| ()),
        3 => tc
            .client
            .generate_fill_reports(GenerateFillReports::new(
                UUID4::new(),
                UnixNanos::default(),
                Some(instrument_id),
                None,
                None,
                None,
                None,
                None,
            ))
            .await
            .map(|_| ()),
        4 | 7..=12 => tc.client.generate_mass_status(None).await.map(|_| ()),
        _ => unreachable!(),
    };

    assert_eq!(
        result.unwrap_err().to_string(),
        "unresolved Derive venue binding for COLD-BINDING"
    );
    *rest_state.open_orders_response.lock().await =
        json!({"orders": [], "subaccount_id": TEST_SUBACCOUNT});
    *rest_state.trigger_orders_response.lock().await =
        json!({"orders": [], "subaccount_id": TEST_SUBACCOUNT});
    *rest_state.order_history_response.lock().await = json!({"orders": [], "pagination": {"count": 0, "num_pages": 0}, "subaccount_id": TEST_SUBACCOUNT});
    *rest_state.trade_history_response.lock().await = json!({"trades": [], "pagination": {"count": 0, "num_pages": 0}, "subaccount_id": TEST_SUBACCOUNT});
    let error = tc
        .client
        .generate_order_status_report(&GenerateOrderStatusReport::new(
            UUID4::new(),
            UnixNanos::default(),
            Some(instrument_id),
            Some(cid),
            None,
            None,
            None,
        ))
        .await
        .unwrap_err();
    assert_eq!(
        error.to_string(),
        "unresolved Derive venue binding for COLD-BINDING"
    );
    let error = tc.client.generate_mass_status(None).await.unwrap_err();
    assert_eq!(
        error.to_string(),
        "unresolved Derive venue binding for COLD-BINDING"
    );

    if (7..=10).contains(&operation) {
        let cancel = CancelOrder::new(
            TraderId::from("TRADER-001"),
            Some(ClientId::from("DERIVE")),
            StrategyId::from("S-1"),
            instrument_id,
            cid,
            Some(old_id),
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        );

        match operation {
            7 => tc.client.cancel_order(cancel).unwrap(),
            8 => tc
                .client
                .modify_order(ModifyOrder::new(
                    TraderId::from("TRADER-001"),
                    Some(ClientId::from("DERIVE")),
                    StrategyId::from("S-1"),
                    instrument_id,
                    cid,
                    Some(old_id),
                    Some(Quantity::from("2.000")),
                    Some(Price::from("3506.00")),
                    None,
                    UUID4::new(),
                    UnixNanos::default(),
                    None,
                    None,
                ))
                .unwrap(),
            9 => assert_eq!(
                tc.client
                    .cancel_all_orders(CancelAllOrders::new(
                        TraderId::from("TRADER-001"),
                        Some(ClientId::from("DERIVE")),
                        StrategyId::from("S-1"),
                        instrument_id,
                        Some(OrderSide::Buy),
                        UUID4::new(),
                        UnixNanos::default(),
                        None,
                        None,
                    ))
                    .unwrap_err()
                    .to_string(),
                "unresolved Derive venue binding for COLD-BINDING"
            ),
            10 => assert_eq!(
                tc.client
                    .batch_cancel_orders(BatchCancelOrders::new(
                        TraderId::from("TRADER-001"),
                        Some(ClientId::from("DERIVE")),
                        StrategyId::from("S-1"),
                        instrument_id,
                        vec![cancel],
                        UUID4::new(),
                        UnixNanos::default(),
                        None,
                        None,
                    ))
                    .unwrap_err()
                    .to_string(),
                "unresolved Derive venue binding for COLD-BINDING"
            ),
            _ => unreachable!(),
        }

        if operation == 7 || operation == 8 {
            assert_unresolved_command_rejection(&mut tc, operation, cid, old_id).await;
        }
    }

    let channel = format!("{TEST_SUBACCOUNT}.orders");
    ws_state.push_notification(make_subscription_frame(
        &channel,
        &json!([
            order_json_with(
                old_id.as_str(),
                cid.as_str(),
                "buy",
                "ETH-PERP",
                1_700_000_003_000,
                if operation == 12 {
                    "expired"
                } else {
                    "cancelled"
                }
            ),
            order_json_with(
                "external-barrier",
                "EXTERNAL-BARRIER",
                "buy",
                "ETH-PERP",
                1_700_000_004_000,
                "open"
            ),
        ]),
    ));

    let event = drain_until(
        &mut tc.rx,
        |event| {
            matches!(
                event,
                ExecutionEvent::Order(OrderEventAny::Canceled(_) | OrderEventAny::Expired(_))
                    | ExecutionEvent::Report(ExecutionReport::Order(_))
            )
        },
        "deferred old cancellation or external barrier",
    )
    .await;

    let event = if operation == 12 {
        let ExecutionEvent::Order(OrderEventAny::Expired(expired)) = event else {
            panic!("definitive original expiry resolves restored pending request: {event:?}");
        };

        assert_eq!(expired.client_order_id, cid);
        assert_eq!(expired.venue_order_id, Some(old_id));
        assert_eq!(expired.instrument_id, instrument_id);
        assert_eq!(expired.strategy_id, StrategyId::from("S-1"));
        assert_eq!(expired.account_id, Some(AccountId::from("DERIVE-001")));
        assert_eq!(expired.ts_event, UnixNanos::from(1_700_000_003_000_000_000));
        tc.client.generate_mass_status(None).await.unwrap();
        drain_until(
            &mut tc.rx,
            |event| matches!(event, ExecutionEvent::Report(ExecutionReport::Order(_))),
            "external barrier after original expiry",
        )
        .await
    } else {
        event
    };

    let ExecutionEvent::Report(ExecutionReport::Order(report)) = event else {
        panic!("unresolved tracked binding must retain old lifecycle authority: {event:?}");
    };

    assert_eq!(
        report.venue_order_id,
        VenueOrderId::from("external-barrier")
    );
    assert_eq!(
        report.client_order_id,
        Some(ClientOrderId::from("EXTERNAL-BARRIER"))
    );
    tc.client.disconnect().await.unwrap();
    assert!(ws_state.cancelled_orders.lock().await.is_empty());
    assert!(ws_state.cancelled_labels.lock().await.is_empty());
    assert!(ws_state.replace_orders.lock().await.is_empty());
}

async fn assert_unresolved_command_rejection(
    tc: &mut TestClient,
    operation: u8,
    cid: ClientOrderId,
    old_id: VenueOrderId,
) {
    let event = drain_until(
        &mut tc.rx,
        |event| matches!(event, ExecutionEvent::Order(_)),
        "unresolved command rejection",
    )
    .await;

    match event {
        ExecutionEvent::Order(OrderEventAny::CancelRejected(event)) if operation == 7 => {
            assert_eq!(event.client_order_id, cid);
            assert_eq!(event.venue_order_id, Some(old_id));
            assert_eq!(
                event.reason,
                "unresolved Derive venue binding for COLD-BINDING"
            );
        }
        ExecutionEvent::Order(OrderEventAny::ModifyRejected(event)) if operation == 8 => {
            assert_eq!(event.client_order_id, cid);
            assert_eq!(event.venue_order_id, Some(old_id));
            assert_eq!(
                event.reason,
                "unresolved Derive venue binding for COLD-BINDING"
            );
        }
        other => panic!("expected matching local command rejection, received {other:?}"),
    }
}

#[rstest]
#[case::orders(false)]
#[case::fills(true)]
#[tokio::test]
async fn test_targeted_report_collection_rejects_incomplete_coverage(#[case] fills: bool) {
    let state = RestState::default();
    let mut bad_trade = sample_trade_json("bad-coverage-trade", "coverage-order", "ETH-PERP");
    bad_trade["direction"] = json!("unsupported-side");
    *state.trade_history_response.lock().await = json!({
        "trades": [sample_trade_json("good-coverage-trade", "coverage-order", "ETH-PERP"), bad_trade],
        "pagination": {"count": 2, "num_pages": 1}, "subaccount_id": TEST_SUBACCOUNT,
    });
    *state.order_history_response.lock().await = json!({
        "orders": [order_json_with("coverage-order", "COVERAGE", "buy", "ETH-PERP", 1_700_000_001_000, "filled"),
            order_json_with(" ", "INVALID", "buy", "ETH-PERP", 1_700_000_002_000, "filled")],
        "pagination": {"count": 2, "num_pages": 1}, "subaccount_id": TEST_SUBACCOUNT,
    });
    let mut tc = build_report_client(state, WsState::default()).await;
    tc.client.connect().await.unwrap();
    let error = if fills {
        tc.client
            .generate_fill_reports(GenerateFillReports::new(
                UUID4::new(),
                UnixNanos::default(),
                None,
                None,
                None,
                None,
                None,
                None,
            ))
            .await
            .unwrap_err()
    } else {
        tc.client
            .generate_order_status_reports(&GenerateOrderStatusReports::new(
                UUID4::new(),
                UnixNanos::default(),
                false,
                None,
                None,
                None,
                None,
                None,
            ))
            .await
            .unwrap_err()
    };

    assert_eq!(
        error.to_string(),
        if fills {
            "incomplete Derive fill report coverage"
        } else {
            "incomplete Derive order report coverage"
        }
    );
    tc.client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_startup_owned_order_ignores_unrelated_historical_metadata() {
    let state = RestState::default();
    let cid = ClientOrderId::from("RESTORE-SCOPED");
    let native_id = VenueOrderId::from("restore-scoped-native");
    *state.order_history_response.lock().await = json!({
        "orders": [order_json_with("expired-unrelated", "UNRELATED", "buy", "EXPIRED-PERP", 1_700_000_002_000, "filled")],
        "pagination": {"count": 1, "num_pages": 1}, "subaccount_id": TEST_SUBACCOUNT,
    });
    let ws = WsState::default();
    let mut tc = build_report_client(state.clone(), ws.clone()).await;
    let order = accepted_order(
        build_limit_order(
            InstrumentId::from("ETH-PERP.DERIVE"),
            cid,
            OrderSide::Buy,
            Price::from("3500.00"),
            Quantity::from("1.000"),
        ),
        native_id,
        AccountId::from("DERIVE-001"),
    );
    add_order_to_cache(&tc.cache, order, Some(ClientId::from("DERIVE")));

    tc.client.connect().await.unwrap();
    let connected = tc.client.is_connected();
    let connections = ws.connection_count.load(Ordering::SeqCst);
    let calls = state.order_history_calls.lock().await.clone();
    tc.client.disconnect().await.unwrap();

    assert!(connected);
    assert_eq!(connections, 1);
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0]["instrument_name"], json!("ETH-PERP"));
    assert_eq!(calls[0]["subaccount_id"], json!(TEST_SUBACCOUNT));
}

#[rstest]
#[case::history_failure(0)]
#[case::fill_failure(1)]
#[case::position_failure(2)]
#[case::salvaged_trade(3)]
#[case::open_failure(4)]
#[tokio::test]
async fn test_mass_status_partial_sources_obey_report_window(
    #[case] source: u8,
    #[values(None, Some(10_000_000))] lookback: Option<u64>,
) {
    let state = RestState::default();
    *state.open_orders_response.lock().await = json!({"orders": [order_json_with("partial-source-order", "PARTIAL-SOURCE", "buy", "ETH-PERP", 1_700_000_001_000, "open")], "subaccount_id": TEST_SUBACCOUNT});
    let error = json!({"id": 1, "error": {"code": -32602, "message": "source unavailable"}});
    match source {
        0 => *state.order_history_response.lock().await = error,
        1 => *state.trade_history_response.lock().await = error,
        2 => *state.positions_response.lock().await = error,
        3 => {
            let mut trade =
                sample_trade_json("partial-source-trade", "partial-source-order", "ETH-PERP");
            trade["direction"] = json!("future-direction");
            *state.trade_history_response.lock().await = json!({"trades": [sample_trade_json("valid-source-trade", "partial-source-order", "ETH-PERP"), trade], "pagination": {"count": 2, "num_pages": 1}, "subaccount_id": TEST_SUBACCOUNT});
        }
        4 => *state.open_orders_response.lock().await = error,
        _ => unreachable!(),
    }

    let mut tc = build_report_client(state, WsState::default()).await;
    tc.client.connect().await.unwrap();

    let result = tc.client.generate_mass_status(lookback).await;
    tc.client.disconnect().await.unwrap();

    if let Some(minutes) = lookback {
        let mass = result.unwrap().unwrap();
        assert!(!mass.reports_complete());
        assert_eq!(
            mass.lookback_start(),
            Some(
                mass.ts_init
                    .saturating_sub(DurationNanos::try_from_mins(minutes).unwrap())
            )
        );
        assert_eq!(mass.order_reports().len(), usize::from(source != 4));
        assert_eq!(mass.fill_reports().len(), usize::from(source == 3));
        assert_eq!(
            mass.position_reports().len(),
            usize::from(source != 2 && source != 4)
        );
        return;
    }

    let error = result.unwrap_err();

    if source == 3 {
        assert_eq!(
            error.to_string(),
            "incomplete unbounded Derive report coverage"
        );
    } else {
        assert!(
            matches!(error.downcast_ref::<DeriveHttpError>(), Some(DeriveHttpError::JsonRpc {code: -32602, message, ..}) if message == "source unavailable")
        );
    }
}

#[rstest]
#[case::matching("matching")]
#[case::nonce("nonce")]
#[case::parent("parent")]
#[case::side("side")]
#[tokio::test]
async fn test_rest_recovers_only_correlated_pending_replace(
    #[case] proof: &str,
    #[values(false, true)] historical: bool,
    #[values(0, 1, 2, 3)] request: u8,
) {
    let state = RestState::default();
    let ws = WsState::default();
    let cid = ClientOrderId::from("REST-REPLACE");
    let instrument_id = InstrumentId::from("ETH-PERP.DERIVE");
    let old_id = VenueOrderId::from("rest-parent");
    let new_id = VenueOrderId::from("rest-child");
    let mut parent = order_json_with(
        old_id.as_str(),
        cid.as_str(),
        "buy",
        "ETH-PERP",
        1_700_000_001_000,
        "open",
    );
    *ws.order_reply.lock().await = Some(json!({"result": {"order": parent.clone(), "trades": []}}));
    *ws.replace_reply.lock().await =
        Some(json!({"error": {"code": 9000, "message": "confirmation timeout"}}));
    let mut tc = build_report_client(state.clone(), ws.clone()).await;
    tc.client.connect().await.unwrap();
    wait_for_private_subscription(&ws).await;
    drain_initial_account_state(&mut tc).await;
    let order = build_limit_order(
        instrument_id,
        cid,
        OrderSide::Buy,
        Price::from("3500.00"),
        Quantity::from("1.000"),
    );
    submit_cached_order(&tc, &order);
    let _accepted = drain_until(
        &mut tc.rx,
        |e| matches!(e, ExecutionEvent::Order(OrderEventAny::Accepted(_))),
        "accepted parent",
    )
    .await;
    tc.client
        .modify_order(ModifyOrder::new(
            TraderId::from("TRADER-001"),
            Some(ClientId::from("DERIVE")),
            StrategyId::from("S-1"),
            instrument_id,
            cid,
            Some(old_id),
            Some(Quantity::from("2.000")),
            Some(Price::from("3505.00")),
            None,
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ))
        .unwrap();
    wait_until(
        || {
            let ws = ws.clone();
            async move { !ws.replace_orders.lock().await.is_empty() }
        },
        "replace transmitted",
    )
    .await;

    assert!(
        tokio::time::timeout(Duration::from_millis(150), tc.rx.recv())
            .await
            .is_err()
    );
    parent["order_status"] = json!("cancelled");
    let mut child = order_json_with(
        new_id.as_str(),
        cid.as_str(),
        "buy",
        "ETH-PERP",
        1_700_000_002_000,
        "open",
    );
    child["amount"] = json!("2.000");
    child["limit_price"] = json!("3505.00");
    child["replaced_order_id"] = json!(old_id.as_str());
    child["nonce"] = ws.replace_orders.lock().await[0]["nonce"].clone();
    let nonce = child["nonce"].as_str().unwrap().parse::<u64>().unwrap();
    let created_ms = nonce.saturating_sub(3_600_000_000_000) / 1_000_000;
    child["creation_timestamp"] = json!(created_ms);
    child["last_update_timestamp"] = json!(created_ms + 1);

    match proof {
        "nonce" => {
            child["nonce"] =
                json!((child["nonce"].as_str().unwrap().parse::<u64>().unwrap() + 1).to_string());
        }
        "parent" => child["replaced_order_id"] = json!("other-parent"),
        "side" => child["direction"] = json!("sell"),
        "matching" => (),
        _ => unreachable!(),
    }

    *state.open_orders_response.lock().await = json!({"orders": if historical {vec![]} else {vec![child.clone()]}, "subaccount_id": TEST_SUBACCOUNT});
    *state.order_history_response.lock().await = json!({"orders": if historical {vec![parent.clone(), child.clone()]} else {vec![parent.clone()]}, "pagination": {"count": if historical {2} else {1}, "num_pages": 1}, "subaccount_id": TEST_SUBACCOUNT});
    state
        .get_order_responses
        .lock()
        .await
        .insert(old_id.to_string(), parent);
    state
        .get_order_responses
        .lock()
        .await
        .insert(new_id.to_string(), child);

    let result = match request {
        0 => {
            tc.client
                .generate_order_status_report(&GenerateOrderStatusReport::new(
                    UUID4::new(),
                    UnixNanos::default(),
                    Some(instrument_id),
                    Some(cid),
                    None,
                    None,
                    None,
                ))
                .await
        }
        1 => tc
            .client
            .generate_order_status_reports(&GenerateOrderStatusReports::new(
                UUID4::new(),
                UnixNanos::default(),
                false,
                Some(instrument_id),
                None,
                None,
                None,
                None,
            ))
            .await
            .map(|reports| reports.into_iter().find(|r| r.venue_order_id == new_id)),
        2 => tc
            .client
            .generate_fill_reports(GenerateFillReports::new(
                UUID4::new(),
                UnixNanos::default(),
                Some(instrument_id),
                None,
                None,
                None,
                None,
                None,
            ))
            .await
            .map(|reports| {
                assert!(reports.is_empty());
                None
            }),
        3 => tc.client.generate_mass_status(None).await.map(|mass| {
            let mass = mass.unwrap();
            assert!(mass.reports_complete());
            mass.order_reports().get(&new_id).cloned()
        }),
        _ => unreachable!(),
    };

    if proof == "matching" {
        let report = result.unwrap();
        let event = drain_until(
            &mut tc.rx,
            |e| matches!(e, ExecutionEvent::Order(_)),
            "REST replacement update",
        )
        .await;

        let ExecutionEvent::Order(OrderEventAny::Updated(update)) = event else {
            panic!("expected update, received {event:?}")
        };

        assert_eq!(update.client_order_id, cid);
        assert_eq!(update.venue_order_id, Some(new_id));
        assert_eq!(update.quantity, Quantity::from("2.000"));
        assert_eq!(update.price, Some(Price::from("3505.00")));

        if let Some(report) = report {
            assert_eq!(report.client_order_id, Some(cid));
            assert_eq!(report.venue_order_id, new_id);
            assert_eq!(report.order_status, OrderStatus::Accepted);
            assert_eq!(report.quantity, Quantity::from("2.000"));
            assert_eq!(report.filled_qty, Quantity::from("0.000"));
            assert_eq!(report.price, Some(Price::from("3505.00")));
        } else {
            assert_eq!(request, 2);
        }

        assert!(tc.rx.try_recv().is_err());
    } else {
        assert_eq!(
            result.unwrap_err().to_string(),
            if proof == "side" {
                "Native replacement order side does not match"
            } else {
                "unresolved Derive venue binding for REST-REPLACE"
            }
        );
        assert!(
            tokio::time::timeout(Duration::from_millis(150), tc.rx.recv())
                .await
                .is_err()
        );
    }

    let history_calls = state.order_history_calls.lock().await;

    if historical || matches!(proof, "nonce" | "parent") {
        assert_eq!(history_calls[0]["from_timestamp"], json!(created_ms));
        assert!(history_calls[0].get("to_timestamp").is_none());
    } else if proof == "side" {
        assert_eq!(history_calls.len(), 0);
    }

    drop(history_calls);
    assert_eq!(ws.replace_orders.lock().await.len(), 1);
    assert!(ws.cancelled_orders.lock().await.is_empty());
    tc.client.disconnect().await.unwrap();
}

#[rstest]
#[case::settles(false)]
#[case::exhausts(true)]
#[tokio::test]
async fn test_private_pagination_restarts_complete_scan(
    #[case] exhausts: bool,
    #[values(false, true)] fills: bool,
) {
    let state = RestState::default();
    *state.open_orders_response.lock().await =
        json!({"orders": [], "subaccount_id": TEST_SUBACCOUNT});

    let field = if fills { "trades" } else { "orders" };
    let mut responses = Vec::new();

    for (count, records) in if exhausts {
        vec![
            (2, vec![1]),
            (3, vec![2]),
            (4, vec![1]),
            (5, vec![2]),
            (6, vec![1]),
            (7, vec![2]),
        ]
    } else {
        vec![(2, vec![9]), (3, vec![2]), (3, vec![1, 2]), (3, vec![3])]
    } {
        let mut response = json!({"pagination": {"count": count, "num_pages": 2}, "subaccount_id": TEST_SUBACCOUNT});
        response[field] = json!(
            records
                .into_iter()
                .map(|page| pagination_record(fills, page))
                .collect::<Vec<_>>()
        );
        responses.push(response);
    }

    if fills {
        *state.trade_history_pages.lock().await = responses;
    } else {
        *state.order_history_pages.lock().await = responses;
    }

    let mut tc = build_report_client(state.clone(), WsState::default()).await;
    tc.client.connect().await.unwrap();
    let result = if fills {
        tc.client
            .generate_fill_reports(GenerateFillReports::new(
                UUID4::new(),
                UnixNanos::default(),
                Some(InstrumentId::from("ETH-PERP.DERIVE")),
                None,
                None,
                None,
                None,
                None,
            ))
            .await
            .map(|reports| {
                reports
                    .into_iter()
                    .map(|r| r.venue_order_id)
                    .collect::<Vec<_>>()
            })
    } else {
        tc.client
            .generate_order_status_reports(&GenerateOrderStatusReports::new(
                UUID4::new(),
                UnixNanos::default(),
                false,
                Some(InstrumentId::from("ETH-PERP.DERIVE")),
                None,
                None,
                None,
                None,
            ))
            .await
            .map(|reports| {
                reports
                    .into_iter()
                    .map(|r| r.venue_order_id)
                    .collect::<Vec<_>>()
            })
    };

    if exhausts {
        assert_eq!(
            result.unwrap_err().to_string(),
            "decode error: Derive page count changed during collection"
        );
    } else {
        assert_eq!(
            result.unwrap(),
            vec![
                VenueOrderId::from("page-order-1"),
                VenueOrderId::from("page-order-2"),
                VenueOrderId::from("page-order-3")
            ]
        );
    }

    let calls = if fills {
        state.trade_history_calls.lock().await.clone()
    } else {
        state.order_history_calls.lock().await.clone()
    };

    assert_eq!(
        calls
            .iter()
            .map(|r| r["page"].as_u64().unwrap())
            .collect::<Vec<_>>(),
        if exhausts {
            vec![1, 2, 1, 2, 1, 2]
        } else {
            vec![1, 2, 1, 2]
        }
    );
    tc.client.disconnect().await.unwrap();
}
