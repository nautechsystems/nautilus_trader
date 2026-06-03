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
    response::{IntoResponse, Json, Response},
    routing::{get, post},
};
use futures_util::StreamExt;
use nautilus_common::{
    cache::Cache,
    clients::ExecutionClient,
    live::runner::replace_exec_event_sender,
    messages::{
        ExecutionEvent,
        execution::{
            BatchCancelOrders, CancelAllOrders, CancelOrder, ExecutionReport, GenerateFillReports,
            GenerateOrderStatusReport, GenerateOrderStatusReports, GeneratePositionStatusReports,
            ModifyOrder, QueryAccount, QueryOrder, SubmitOrder, SubmitOrderList,
        },
    },
    testing::wait_until_async,
};
use nautilus_core::{UUID4, UnixNanos};
use nautilus_derive::{
    common::{
        consts::{DERIVE_VENUE, MIN_SIGNATURE_TTL, TRIGGER_ORDER_SIGNATURE_TTL},
        enums::DeriveEnvironment,
        parse::parse_derive_instrument_any,
    },
    config::DeriveExecClientConfig,
    execution::DeriveExecutionClient,
    http::models::DeriveInstrument,
};
use nautilus_live::ExecutionClientCore;
use nautilus_model::{
    accounts::{AccountAny, MarginAccount},
    data::QuoteTick,
    enums::{
        AccountType, OmsType, OrderSide, OrderStatus, OrderType, PositionSideSpecified,
        TimeInForce, TriggerType,
    },
    events::{AccountState, OrderEventAny, OrderInitialized},
    identifiers::{
        AccountId, ClientId, ClientOrderId, InstrumentId, OrderListId, StrategyId, TraderId,
        VenueOrderId,
    },
    orders::{
        LimitIfTouchedOrder, LimitOrder, MarketOrder, Order, OrderAny, OrderList, StopMarketOrder,
    },
    reports::{OrderStatusReport, PositionStatusReport},
    types::{AccountBalance, Money, Price, Quantity},
};
use nautilus_network::{http::HttpClient, websocket::TransportBackend};
use rstest::rstest;
use rust_decimal_macros::dec;
use serde_json::{Value, json};

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
    get_instrument_response: Arc<tokio::sync::Mutex<Value>>,
}

#[derive(Clone)]
struct WsState {
    connection_count: Arc<AtomicUsize>,
    login_frames: Arc<tokio::sync::Mutex<Vec<Value>>>,
    subscribe_frames: Arc<tokio::sync::Mutex<Vec<Value>>>,
    // Order entry now flows over the WebSocket Trading API. Each vector holds
    // the `params` object of a captured `private/*` frame so assertions read
    // the signed body fields directly (`body["instrument_name"]`, etc.).
    submitted_orders: Arc<tokio::sync::Mutex<Vec<Value>>>,
    submitted_trigger_orders: Arc<tokio::sync::Mutex<Vec<Value>>>,
    cancelled_orders: Arc<tokio::sync::Mutex<Vec<Value>>>,
    cancelled_trigger_orders: Arc<tokio::sync::Mutex<Vec<Value>>>,
    cancel_all_calls: Arc<tokio::sync::Mutex<Vec<Value>>>,
    replace_orders: Arc<tokio::sync::Mutex<Vec<Value>>>,
    // Injected JSON-RPC reply body (without `id`) per private method. When set,
    // the mock merges the request `id` and returns it instead of the default
    // success result, e.g. `json!({"error": {"code": -32602, "message": "x"}})`.
    order_reply: Arc<tokio::sync::Mutex<Option<Value>>>,
    trigger_order_reply: Arc<tokio::sync::Mutex<Option<Value>>>,
    cancel_reply: Arc<tokio::sync::Mutex<Option<Value>>>,
    cancel_trigger_reply: Arc<tokio::sync::Mutex<Option<Value>>>,
    replace_reply: Arc<tokio::sync::Mutex<Option<Value>>>,
    notification_tx: tokio::sync::mpsc::UnboundedSender<Value>,
    notification_rx: Arc<tokio::sync::Mutex<Option<tokio::sync::mpsc::UnboundedReceiver<Value>>>>,
}

impl Default for WsState {
    fn default() -> Self {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<Value>();
        Self {
            connection_count: Arc::new(AtomicUsize::new(0)),
            login_frames: Arc::new(tokio::sync::Mutex::new(Vec::new())),
            subscribe_frames: Arc::new(tokio::sync::Mutex::new(Vec::new())),
            submitted_orders: Arc::new(tokio::sync::Mutex::new(Vec::new())),
            submitted_trigger_orders: Arc::new(tokio::sync::Mutex::new(Vec::new())),
            cancelled_orders: Arc::new(tokio::sync::Mutex::new(Vec::new())),
            cancelled_trigger_orders: Arc::new(tokio::sync::Mutex::new(Vec::new())),
            cancel_all_calls: Arc::new(tokio::sync::Mutex::new(Vec::new())),
            replace_orders: Arc::new(tokio::sync::Mutex::new(Vec::new())),
            order_reply: Arc::new(tokio::sync::Mutex::new(None)),
            trigger_order_reply: Arc::new(tokio::sync::Mutex::new(None)),
            cancel_reply: Arc::new(tokio::sync::Mutex::new(None)),
            cancel_trigger_reply: Arc::new(tokio::sync::Mutex::new(None)),
            replace_reply: Arc::new(tokio::sync::Mutex::new(None)),
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
    state.get_order_calls.lock().await.push(parsed);
    let response = state.get_order_response.lock().await.clone();
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
    state.get_instrument_calls.lock().await.push(parsed);
    let response = state.get_instrument_response.lock().await.clone();
    let body = if response.is_null() {
        json!({"id": 1, "result": sample_instrument_json()})
    } else {
        json!({"id": 1, "result": response})
    };
    (StatusCode::OK, Json(body)).into_response()
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

                        let params = payload.get("params").cloned().unwrap_or(Value::Null);
                        let reply = match method {
                            "public/login" => {
                                state.login_frames.lock().await.push(payload.clone());
                                json!({"id": id, "result": {"success": true}})
                            }
                            "subscribe" => {
                                state.subscribe_frames.lock().await.push(payload.clone());
                                let channels = payload
                                    .get("params")
                                    .and_then(|p| p.get("channels"))
                                    .and_then(Value::as_array)
                                    .cloned()
                                    .unwrap_or_default();
                                json!({"id": id, "result": {"channels": channels}})
                            }
                            "private/order" => {
                                state.submitted_orders.lock().await.push(params);
                                ws_reply(id, &state.order_reply, || {
                                    json!({"result": {"order": sample_order_json()}})
                                })
                                .await
                            }
                            "private/trigger_order" => {
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
                            "private/replace" => {
                                state.replace_orders.lock().await.push(params);
                                ws_reply(id, &state.replace_reply, || {
                                    json!({
                                        "result": {
                                            "order": order_json_with(
                                                "ord-replaced-1",
                                                "STRAT-O-1",
                                                "buy",
                                                "ETH-PERP",
                                                1_700_000_001_000_i64,
                                                "open",
                                            ),
                                            "cancelled_order": order_json_with(
                                                "ord-stale-1",
                                                "STRAT-O-1",
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
                            "private/cancel_all" => {
                                state.cancel_all_calls.lock().await.push(params);
                                json!({"id": id, "result": {}})
                            }
                            _ => json!({"id": id, "result": {}}),
                        };

                        if socket
                            .send(Message::Text(reply.to_string().into()))
                            .await
                            .is_err()
                        {
                            break;
                        }
                    }
                    Message::Close(_) => break,
                    _ => {}
                }
            }
            notif = recv_notification(&mut notification_rx) => {
                let Some(notif) = notif else { continue };
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
        "scheduled_activation": 1_700_000_000_000_i64,
        "scheduled_deactivation": 32503680000000_i64,
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
        "nonce": 1,
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
        "nonce": 1,
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
        "nonce": 1,
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
        "tx_status": "settled",
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
        "currency": "USDC",
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

fn test_config(rest: SocketAddr, ws: SocketAddr) -> DeriveExecClientConfig {
    DeriveExecClientConfig {
        wallet_address: Some(TEST_WALLET.to_string()),
        session_key: Some(TEST_SESSION_KEY.to_string()),
        subaccount_id: Some(TEST_SUBACCOUNT),
        base_url_rest: Some(rest_url(rest)),
        base_url_ws: Some(ws_url(ws)),
        proxy_url: None,
        environment: DeriveEnvironment::Testnet,
        http_timeout_secs: 5,
        max_retries: 1,
        retry_delay_initial_ms: 50,
        retry_delay_max_ms: 500,
        max_fee_per_contract: None,
        transport_backend: TransportBackend::default(),
        domain_separator: Some(TEST_DOMAIN_SEPARATOR.to_string()),
        action_typehash: Some(TEST_ACTION_TYPEHASH.to_string()),
        trade_module_address: Some(TEST_TRADE_MODULE_ADDRESS.to_string()),
        signature_expiry_secs: 600,
        market_order_slippage_bps: 50,
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
    build_client_with_config(rest_state, ws_state, |config| config).await
}

async fn build_client_with_config(
    rest_state: RestState,
    ws_state: WsState,
    configure: impl FnOnce(DeriveExecClientConfig) -> DeriveExecClientConfig,
) -> TestClient {
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
    let mut client = DeriveExecutionClient::new(build_core(cache.clone()), config)
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
    let init_id = UUID4::new();
    OrderAny::Limit(LimitOrder::new(
        TraderId::from("TRADER-001"),
        StrategyId::from("S-1"),
        instrument_id,
        client_order_id,
        side,
        quantity,
        price,
        time_in_force,
        None,
        post_only,
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
        init_id,
        UnixNanos::default(),
    ))
}

fn build_reduce_only_limit_order(
    instrument_id: InstrumentId,
    client_order_id: ClientOrderId,
    side: OrderSide,
    price: Price,
    quantity: Quantity,
) -> OrderAny {
    let init_id = UUID4::new();
    OrderAny::Limit(LimitOrder::new(
        TraderId::from("TRADER-001"),
        StrategyId::from("S-1"),
        instrument_id,
        client_order_id,
        side,
        quantity,
        price,
        TimeInForce::Gtc,
        None,
        false,
        true,
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
        init_id,
        UnixNanos::default(),
    ))
}

fn build_market_order(
    instrument_id: InstrumentId,
    client_order_id: ClientOrderId,
    side: OrderSide,
    quantity: Quantity,
) -> OrderAny {
    let init_id = UUID4::new();
    OrderAny::Market(MarketOrder::new(
        TraderId::from("TRADER-001"),
        StrategyId::from("S-1"),
        instrument_id,
        client_order_id,
        side,
        quantity,
        TimeInForce::Gtc,
        init_id,
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

fn build_stop_market_order(
    instrument_id: InstrumentId,
    client_order_id: ClientOrderId,
    side: OrderSide,
    trigger_price: Price,
    quantity: Quantity,
) -> OrderAny {
    OrderAny::StopMarket(StopMarketOrder::new(
        TraderId::from("TRADER-001"),
        StrategyId::from("S-1"),
        instrument_id,
        client_order_id,
        side,
        quantity,
        trigger_price,
        TriggerType::MarkPrice,
        TimeInForce::Gtc,
        None,
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

fn build_limit_if_touched_order(
    instrument_id: InstrumentId,
    client_order_id: ClientOrderId,
    side: OrderSide,
    price: Price,
    trigger_price: Price,
    quantity: Quantity,
) -> OrderAny {
    OrderAny::LimitIfTouched(LimitIfTouchedOrder::new(
        TraderId::from("TRADER-001"),
        StrategyId::from("S-1"),
        instrument_id,
        client_order_id,
        side,
        quantity,
        price,
        trigger_price,
        TriggerType::MarkPrice,
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
#[tokio::test]
async fn test_exec_client_connect_subscribes_private_channels() {
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    let mut tc = build_client(rest_state, ws_state.clone()).await;

    tc.client.connect().await.expect("connect succeeds");

    wait_until(
        || {
            let state = ws_state.clone();
            async move { !state.subscribe_frames.lock().await.is_empty() }
        },
        "subscribe frame received",
    )
    .await;

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
}

#[rstest]
#[tokio::test]
async fn test_submit_order_limit_posts_signed_payload() {
    let rest_state = RestState::default();
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
    assert!(body["nonce"].as_u64().unwrap() > 0);

    tc.client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test]
async fn test_submit_order_accepts_signature_ttl_above_minimum() {
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    let mut tc = build_client_with_config(rest_state, ws_state.clone(), |mut config| {
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
#[case(MIN_SIGNATURE_TTL.as_secs(), "must be greater than the Derive minimum")]
#[case(MIN_SIGNATURE_TTL.as_secs() - 1, "must be greater than the Derive minimum")]
#[tokio::test]
async fn test_submit_order_rejects_signature_ttl_minimum_or_lower_before_posting(
    #[case] signature_expiry_secs: u64,
    #[case] reason_fragment: &str,
) {
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    let mut tc = build_client_with_config(rest_state, ws_state.clone(), |mut config| {
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
            rejected
                .reason
                .as_str()
                .contains("order expiry validation failed")
                && rejected.reason.as_str().contains(reason_fragment),
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
#[tokio::test]
async fn test_submit_stop_market_posts_trigger_order_and_emits_accepted() {
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    let mut tc = build_client(rest_state, ws_state.clone()).await;
    tc.client.connect().await.expect("connect succeeds");

    let instrument_id = InstrumentId::from("ETH-PERP.DERIVE");
    let client_order_id = ClientOrderId::from("STRAT-STOP-1");
    let order = build_stop_market_order(
        instrument_id,
        client_order_id,
        OrderSide::Sell,
        Price::from("3600.00"),
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
        "private/trigger_order posted",
    )
    .await;

    let posts = ws_state.submitted_trigger_orders.lock().await;
    let body = &posts[0];
    assert_eq!(body["instrument_name"].as_str(), Some("ETH-PERP"));
    assert_eq!(body["direction"].as_str(), Some("sell"));
    assert_eq!(body["order_type"].as_str(), Some("market"));
    assert_eq!(body["time_in_force"].as_str(), Some("gtc"));
    assert_eq!(body["label"].as_str(), Some("STRAT-STOP-1"));
    assert_eq!(body["limit_price"].as_str(), Some("3582.00"));
    assert_eq!(body["amount"].as_str(), Some("1.000"));
    assert_eq!(body["trigger_price"].as_str(), Some("3600.00"));
    assert_eq!(body["trigger_price_type"].as_str(), Some("mark"));
    assert_eq!(body["trigger_type"].as_str(), Some("stoploss"));
    assert_eq!(body["subaccount_id"].as_u64(), Some(TEST_SUBACCOUNT));
    assert!(body["signature"].as_str().unwrap().starts_with("0x"));
    assert!(
        body["conn_id"].as_str().is_some_and(|v| !v.is_empty()),
        "conn_id must be present",
    );
    assert!(
        body["order_id"].as_str().is_some_and(|v| !v.is_empty()),
        "trigger order_id must be present",
    );
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
        "trigger submit must not post private/order",
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
#[case(TimeInForce::Day, false, "unsupported time in force")]
#[case(TimeInForce::Day, true, "unsupported time in force")]
#[case(TimeInForce::Ioc, true, "post-only Derive orders only support GTC")]
#[case(TimeInForce::Fok, true, "post-only Derive orders only support GTC")]
#[tokio::test]
async fn test_submit_order_rejects_unsupported_time_in_force_before_posting(
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
            rejected.reason.as_str().contains("order encoding failed")
                && rejected.reason.as_str().contains(reason_fragment),
            "unexpected reject reason: {}",
            rejected.reason,
        );
    } else {
        unreachable!();
    }
    assert!(
        ws_state.submitted_orders.lock().await.is_empty(),
        "invalid TIF must not post to the venue",
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
        assert!(denied.reason.as_str().contains("no cached quote"));
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
                .as_str()
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
#[tokio::test]
async fn test_submit_order_jsonrpc_ambiguous_does_not_emit_order_rejected() {
    // JSON-RPC `-32603` (generic internal error) is the only code currently in
    // the write-outcome-ambiguous set: the venue's own process is known to
    // have run for some unknown distance before failing, so the order may
    // have been accepted before the failure response. `send_private_once`
    // does not replay; emitting OrderRejected on those would let the engine
    // treat a live order as rejected. Mirrors the cancel/modify policy.
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    *ws_state.order_reply.lock().await = Some(json!({
        "error": {"code": -32603, "message": "Internal venue error"}
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

    tc.client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test]
async fn test_submit_order_rate_limit_jsonrpc_emits_order_rejected() {
    // Observed venue behaviour: Derive returns `-32000 Rate limit exceeded`
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

    tc.client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test]
async fn test_cancel_all_orders_with_no_side_calls_cancel_all() {
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    let mut tc = build_client(rest_state, ws_state.clone()).await;
    tc.client.connect().await.expect("connect succeeds");

    let cmd = CancelAllOrders::new(
        TraderId::from("TRADER-001"),
        Some(ClientId::from("DERIVE")),
        StrategyId::from("S-1"),
        InstrumentId::from("ETH-PERP.DERIVE"),
        OrderSide::NoOrderSide,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );
    tc.client.cancel_all_orders(cmd).expect("cancel_all Ok");

    wait_until(
        || {
            let state = ws_state.clone();
            async move { !state.cancel_all_calls.lock().await.is_empty() }
        },
        "cancel_all posted",
    )
    .await;
    let posts = ws_state.cancel_all_calls.lock().await;
    let body = &posts[0];
    assert_eq!(body["subaccount_id"].as_u64(), Some(TEST_SUBACCOUNT));
    assert_eq!(body["instrument_name"].as_str(), Some("ETH-PERP"));
    assert!(ws_state.cancelled_orders.lock().await.is_empty());

    tc.client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test]
async fn test_cancel_all_orders_buy_side_iterates_filtered_open_orders() {
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    // Three open orders: buy on ETH-PERP, sell on ETH-PERP, buy on BTC-PERP.
    *rest_state.open_orders_response.lock().await = json!({
        "orders": [
            order_json_with("buy-eth", "L1", "buy", "ETH-PERP", 1, "open"),
            order_json_with("sell-eth", "L2", "sell", "ETH-PERP", 1, "open"),
            order_json_with("buy-btc", "L3", "buy", "BTC-PERP", 1, "open"),
        ],
        "subaccount_id": TEST_SUBACCOUNT,
    });
    let mut tc = build_client(rest_state.clone(), ws_state.clone()).await;
    tc.client.connect().await.expect("connect succeeds");

    let cmd = CancelAllOrders::new(
        TraderId::from("TRADER-001"),
        Some(ClientId::from("DERIVE")),
        StrategyId::from("S-1"),
        InstrumentId::from("ETH-PERP.DERIVE"),
        OrderSide::Buy,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );
    tc.client.cancel_all_orders(cmd).expect("cancel_all Ok");

    wait_until(
        || {
            let state = ws_state.clone();
            async move { !state.cancelled_orders.lock().await.is_empty() }
        },
        "filtered cancel posted",
    )
    .await;

    let posts = ws_state.cancelled_orders.lock().await;
    assert_eq!(posts.len(), 1, "expected exactly one filtered cancel");
    let body = &posts[0];
    assert_eq!(body["order_id"].as_str(), Some("buy-eth"));
    assert_eq!(body["instrument_name"].as_str(), Some("ETH-PERP"));
    // Bulk cancel_all endpoint must not have been hit.
    assert!(ws_state.cancel_all_calls.lock().await.is_empty());

    tc.client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test]
async fn test_modify_order_posts_replace_and_emits_order_updated() {
    let rest_state = RestState::default();
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
#[case(
    MIN_SIGNATURE_TTL.as_secs(),
    "must be greater than the Derive minimum"
)]
#[case(
    MIN_SIGNATURE_TTL.as_secs() - 1,
    "must be greater than the Derive minimum"
)]
#[case(i64::MAX as u64, "overflows Derive signature_expiry_sec")]
#[tokio::test]
async fn test_modify_order_rejects_invalid_signature_ttl_before_posting_replace(
    #[case] signature_expiry_secs: u64,
    #[case] reason_fragment: &str,
) {
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    let mut tc = build_client_with_config(rest_state, ws_state.clone(), |mut config| {
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
async fn test_modify_order_unexpected_response_shape_does_not_emit_updated() {
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    // Venue returned `result: {}` (no `order.order_id`). Over the WS Trading
    // API the typed handle cannot decode the missing `order`, so the request
    // fails with a `Serde` error. A response the client cannot read leaves the
    // replace outcome ambiguous (the venue may have applied it), so the adapter
    // emits no terminal event and lets reconciliation settle the order rather
    // than rebinding to a stale VOI or falsely rejecting a live order.
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
async fn test_modify_order_jsonrpc_rejection_emits_modify_rejected() {
    let rest_state = RestState::default();
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
#[tokio::test]
async fn test_modify_order_jsonrpc_ambiguous_does_not_emit_modify_rejected() {
    // JSON-RPC `-32603` (generic internal error) leaves the replace outcome
    // ambiguous: the venue may have processed it and merely failed to
    // respond. Emitting OrderModifyRejected on those would let the engine
    // revert a successfully-replaced order; the adapter must stay silent
    // and rely on WS reconciliation. Mirrors the submit/cancel policy.
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    *ws_state.replace_reply.lock().await = Some(json!({
        "error": {"code": -32603, "message": "Internal venue error"}
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
            rejected.reason.as_str().contains(reason_fragment),
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
            rejected.reason.as_str(),
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
#[tokio::test]
async fn test_query_order_emits_order_status_report() {
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    let mut tc = build_client(rest_state.clone(), ws_state).await;
    tc.client.connect().await.expect("connect succeeds");

    let cmd = QueryOrder::new(
        TraderId::from("TRADER-001"),
        Some(ClientId::from("DERIVE")),
        StrategyId::from("S-1"),
        InstrumentId::from("ETH-PERP.DERIVE"),
        ClientOrderId::from("STRAT-O-1"),
        Some(VenueOrderId::from("ord-mock-1")),
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
        assert_eq!(report.venue_order_id.as_str(), "ord-mock-1");
    } else {
        unreachable!();
    }

    tc.client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test]
async fn test_query_account_emits_account_state_event() {
    let rest_state = RestState::default();
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
        // sample subaccount carries 1000 USDC total / 100 USDC initial margin.
        assert_eq!(state.balances.len(), 1);
        assert_eq!(state.balances[0].total.as_decimal(), dec!(1000));
        assert_eq!(state.margins.len(), 1);
        assert_eq!(state.margins[0].initial.as_decimal(), dec!(100));
    } else {
        unreachable!();
    }

    let calls = rest_state.get_subaccount_calls.lock().await;
    // At least one call (connect refresh) plus the explicit query.
    assert!(calls.len() >= 2);

    tc.client.disconnect().await.expect("disconnect");
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
    *rest_state.order_history_response.lock().await = json!({
        "orders": [order_json_with(
            "from-history", "L-HIST", "buy", "ETH-PERP", 1_700_000_001_000, "filled",
        )],
        "pagination": {"count": 1, "num_pages": 1},
        "subaccount_id": TEST_SUBACCOUNT,
    });
    let mut tc = build_client(rest_state.clone(), ws_state).await;
    tc.client.connect().await.expect("connect succeeds");

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
#[tokio::test]
async fn test_generate_order_status_reports_history_path_when_not_open_only() {
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
    let mut tc = build_client(rest_state.clone(), ws_state).await;
    tc.client.connect().await.expect("connect succeeds");

    let cmd = GenerateOrderStatusReports::new(
        UUID4::new(),
        UnixNanos::default(),
        false,
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
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].venue_order_id.as_str(), "from-history");
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
    let mut tc = build_client(rest_state.clone(), ws_state).await;
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
    assert_eq!(venue_order_ids, vec!["order-page-1", "order-page-2"]);

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
async fn test_generate_order_status_reports_open_only_applies_time_window() {
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
    let mut tc = build_client(rest_state, ws_state).await;
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
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].venue_order_id.as_str(), "middle");

    tc.client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test]
async fn test_generate_order_status_report_falls_back_to_history_by_label() {
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    *rest_state.open_orders_response.lock().await = json!({
        "orders": [],
        "subaccount_id": TEST_SUBACCOUNT,
    });
    *rest_state.order_history_response.lock().await = json!({
        "orders": [order_json_with(
            "ord-hist-1", "STRAT-LABEL", "buy", "ETH-PERP", 1, "filled",
        )],
        "pagination": {"count": 1, "num_pages": 1},
        "subaccount_id": TEST_SUBACCOUNT,
    });
    let mut tc = build_client(rest_state.clone(), ws_state).await;
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
    let report = tc
        .client
        .generate_order_status_report(&cmd)
        .await
        .expect("report")
        .expect("some");
    assert_eq!(report.venue_order_id.as_str(), "ord-hist-1");
    assert!(!rest_state.order_history_calls.lock().await.is_empty());
    let calls = rest_state.order_history_calls.lock().await;
    assert_eq!(calls[0]["page_size"].as_u64(), Some(500));

    tc.client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test]
async fn test_generate_order_status_report_finds_trigger_order_by_label_before_history() {
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    *rest_state.open_orders_response.lock().await = json!({
        "orders": [],
        "subaccount_id": TEST_SUBACCOUNT,
    });
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
            "3600",
            "mark",
            "takeprofit",
        )],
        "subaccount_id": TEST_SUBACCOUNT,
    });
    *rest_state.order_history_response.lock().await = json!({
        "orders": [order_json_with(
            "ord-hist-1", "STRAT-TRIG-LABEL", "buy", "ETH-PERP", 1, "filled",
        )],
        "pagination": {"count": 1, "num_pages": 1},
        "subaccount_id": TEST_SUBACCOUNT,
    });
    let mut tc = build_client(rest_state.clone(), ws_state).await;
    tc.client.connect().await.expect("connect succeeds");

    let cmd = GenerateOrderStatusReport::new(
        UUID4::new(),
        UnixNanos::default(),
        Some(InstrumentId::from("ETH-PERP.DERIVE")),
        Some(ClientOrderId::from("STRAT-TRIG-LABEL")),
        None,
        None,
        None,
    );
    let report = tc
        .client
        .generate_order_status_report(&cmd)
        .await
        .expect("report")
        .expect("some");
    assert_eq!(report.venue_order_id.as_str(), "trig-label-1");
    assert_eq!(report.order_type, OrderType::LimitIfTouched);
    assert_eq!(report.order_status, OrderStatus::Accepted);
    assert_eq!(report.price, Some(Price::from("3700")));
    assert_eq!(report.trigger_price, Some(Price::from("3600")));
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
    let mut tc = build_client(rest_state, ws_state).await;
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
    let mut tc = build_client(rest_state, ws_state).await;
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
    let mut tc = build_client(rest_state, ws_state).await;
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
    assert_eq!(reports[0].signed_decimal_qty.to_string(), "3");

    tc.client.disconnect().await.expect("disconnect");
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
    let mut tc = build_client(rest_state.clone(), ws_state).await;
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
    let mut tc = build_client(rest_state, ws_state).await;
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
    assert_eq!(eth_reports[0].position_side, PositionSideSpecified::Flat);
    assert_eq!(eth_reports[0].signed_decimal_qty, dec!(0));

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
    let mut tc = build_client(rest_state.clone(), ws_state).await;
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
async fn test_generate_mass_status_prefers_open_order_snapshot_on_overlap() {
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
    let mut tc = build_client(rest_state, ws_state).await;
    tc.client.connect().await.expect("connect succeeds");

    let mass_status = tc
        .client
        .generate_mass_status(Some(10_000_000))
        .await
        .expect("mass status request succeeds")
        .expect("Derive returns mass status");
    let order_reports = mass_status.order_reports();
    let report = order_reports
        .get(&VenueOrderId::from("ord-overlap-1"))
        .expect("overlapping order report");

    assert_eq!(order_reports.len(), 1);
    assert_eq!(report.order_status, OrderStatus::Accepted);
    assert_eq!(
        report.client_order_id.map(|id| id.as_str().to_string()),
        Some("L-OPEN".to_string()),
    );

    tc.client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test]
async fn test_generate_order_status_report_by_venue_id_uses_get_order() {
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    let mut tc = build_client(rest_state.clone(), ws_state).await;
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
#[tokio::test]
async fn test_generate_order_status_report_by_venue_id_falls_back_to_trigger_orders() {
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    *rest_state.get_order_response.lock().await = json!({
        "jsonrpc": "2.0",
        "error": {"code": -32602, "message": "Order not found"},
    });
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
    let mut tc = build_client(rest_state.clone(), ws_state).await;
    tc.client.connect().await.expect("connect succeeds");

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
    assert_eq!(rest_state.trigger_orders_calls.lock().await.len(), 1);

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
    ws_state.push_notification(make_subscription_frame(&channel, &data));
    ws_state.push_notification(make_subscription_frame(&channel, &data));

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
    let mut tc = build_client(rest_state.clone(), ws_state.clone()).await;
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
async fn test_ws_dispatch_tracked_terminal_status_emits_proper_event_and_forgets_identity(
    #[case] status: &str,
    #[case] expected: &str,
) {
    // Tracked Canceled / Expired must:
    //   1. Synthesize OrderAccepted (carrying ts_accepted from the report)
    //   2. Emit the terminal event (carrying ts_last)
    //   3. Forget identity, so a replayed `.orders` frame for the same CID
    //      falls back to OrderStatusReport
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

    // Replay the same frame. Identity was forgotten, so the dispatch must
    // fall back to the report path.
    ws_state.push_notification(make_subscription_frame(&channel, &frame));
    let event = drain_until(
        &mut tc.rx,
        |e| {
            matches!(
                e,
                ExecutionEvent::Report(ExecutionReport::Order(_))
                    | ExecutionEvent::Order(
                        OrderEventAny::Canceled(_)
                            | OrderEventAny::Expired(_)
                            | OrderEventAny::Accepted(_),
                    )
            )
        },
        "post-terminal replay",
    )
    .await;
    assert!(
        matches!(event, ExecutionEvent::Report(ExecutionReport::Order(_))),
        "after terminal, replayed frame must fall back to OrderStatusReport, was {event:?}",
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
            assert_eq!(rejected.reason.as_str(), "Order rejected by Derive");
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
            assert_eq!(
                rejected.reason.as_str(),
                "Post only order cannot cross the market"
            );
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
async fn test_submit_order_jsonrpc_rejection_forgets_identity() {
    // Regression for the submit_order forget-on-rejection paths: after a
    // JSON-RPC rejection, the registered identity must be cleared so a later
    // `.orders` frame for the same CID falls back to the untracked report
    // path (the engine has already terminated the order locally).
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

    // Now push a `.orders` Open frame reusing the same label. With the
    // identity forgotten, this must take the untracked report path; before
    // the fix, it would have re-synthesized OrderAccepted for a CID the
    // local engine had already rejected.
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

    let event = drain_until(
        &mut tc.rx,
        |e| {
            matches!(
                e,
                ExecutionEvent::Report(ExecutionReport::Order(_))
                    | ExecutionEvent::Order(OrderEventAny::Accepted(_))
            )
        },
        "post-reject .orders frame outcome",
    )
    .await;
    assert!(
        matches!(event, ExecutionEvent::Report(ExecutionReport::Order(_))),
        "identity must be forgotten after rejection; got {event:?}",
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
#[tokio::test]
async fn test_cancel_order_jsonrpc_ambiguous_does_not_emit_cancel_rejected() {
    // JSON-RPC `-32603` (generic internal error) leaves the cancel outcome
    // ambiguous because `send_private_once` does not replay and the venue
    // may have processed the cancel before the failure response. The
    // adapter must NOT emit OrderCancelRejected; WS reconciliation drives
    // the terminal state.
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    *ws_state.cancel_reply.lock().await = Some(json!({
        "error": {"code": -32603, "message": "Internal venue error"}
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

    tc.client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test]
async fn test_cancel_all_orders_buy_side_with_no_open_orders_is_noop() {
    // Side-filtered cancel-all must tolerate an empty open-orders response:
    // no further cancel posts must land. The adapter's only choice on an
    // empty list is to do nothing, since `private/cancel_all` would drop
    // both sides and violate the caller's filter.
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    *rest_state.open_orders_response.lock().await = json!({
        "orders": [],
        "subaccount_id": TEST_SUBACCOUNT,
    });
    let mut tc = build_client(rest_state.clone(), ws_state.clone()).await;
    tc.client.connect().await.expect("connect succeeds");

    let cmd = CancelAllOrders::new(
        TraderId::from("TRADER-001"),
        Some(ClientId::from("DERIVE")),
        StrategyId::from("S-1"),
        InstrumentId::from("ETH-PERP.DERIVE"),
        OrderSide::Buy,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );
    tc.client.cancel_all_orders(cmd).expect("cancel_all Ok");

    wait_until(
        || {
            let state = rest_state.clone();
            async move { !state.open_orders_calls.lock().await.is_empty() }
        },
        "open orders queried",
    )
    .await;

    // Intentional quiet window: `wait_until_async` cannot prove absence of a future cancel.
    tokio::time::sleep(Duration::from_millis(100)).await;
    let cancels = ws_state.cancelled_orders.lock().await;
    assert!(
        cancels.is_empty(),
        "no cancels should be sent when open_orders is empty, saw {}",
        cancels.len(),
    );
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
    let mut tc = build_client(rest_state, ws_state).await;
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
    let mut tc = build_client(rest_state.clone(), ws_state).await;
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
    assert_eq!(eth1.order_side, OrderSide::Buy);

    let eth2 = by_voi.get("ord-eth-2").expect("ord-eth-2 present");
    assert_eq!(eth2.order_side, OrderSide::Sell);

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
    let mut tc = build_client(rest_state, ws_state).await;
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
    assert_eq!(eth.position_side, PositionSideSpecified::Long);
    assert_eq!(eth.signed_decimal_qty, dec!(3));

    let btc = by_symbol.get("BTC-PERP").expect("BTC-PERP present");
    assert_eq!(btc.position_side, PositionSideSpecified::Short);
    assert_eq!(btc.signed_decimal_qty, dec!(-1.5));

    let sol = by_symbol.get("SOL-PERP").expect("SOL-PERP present");
    assert_eq!(sol.position_side, PositionSideSpecified::Flat);
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
    let mut tc = build_client(rest_state.clone(), ws_state).await;
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
        "tx_status": "settled",
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
    assert!(body["nonce"].as_u64().unwrap() > 0);

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
    let mut tc = build_client(rest_state, ws_state).await;
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
    assert_eq!(reports[0].position_side, PositionSideSpecified::Short);
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
    let _ = drain_until(
        &mut tc.rx,
        |e| matches!(e, ExecutionEvent::Account(_)),
        "initial AccountState",
    )
    .await;
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
        "tx_status": "settled",
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
        "tx_status": "settled",
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
        assert!(
            denied.reason.as_str().contains("reduce-only"),
            "unexpected deny reason: {}",
            denied.reason,
        );
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
            rejected.reason.as_str().contains("reduce-only"),
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
