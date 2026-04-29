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

//! Mock WebSocket and HTTP server for integration testing.

#![allow(dead_code)]

use std::{
    net::SocketAddr,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};

use axum::{
    Json, Router,
    extract::{
        State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    response::{IntoResponse, Response},
    routing::{get, post},
};
use nautilus_common::testing::wait_until_async;
use nautilus_model::{
    enums::AssetClass,
    identifiers::{InstrumentId, Symbol, Venue},
    instruments::{InstrumentAny, PerpetualContract},
    types::{Currency, Price, Quantity},
};
use rust_decimal::Decimal;
use serde_json::json;
use ustr::Ustr;

#[derive(Clone)]
pub struct TestServerState {
    pub connection_count: Arc<tokio::sync::Mutex<usize>>,
    pub subscriptions: Arc<tokio::sync::Mutex<Vec<String>>>,
    pub subscription_events: Arc<tokio::sync::Mutex<Vec<(String, bool)>>>,
    pub fail_next_subscriptions: Arc<tokio::sync::Mutex<Vec<String>>>,
    pub authenticated: Arc<AtomicBool>,
    pub disconnect_trigger: Arc<AtomicBool>,
    pub ping_count: Arc<AtomicUsize>,
    pub pong_count: Arc<AtomicUsize>,
    pub heartbeat_count: Arc<AtomicUsize>,
    pub messages_received: Arc<tokio::sync::Mutex<Vec<serde_json::Value>>>,
    pub cancel_all_count: Arc<AtomicUsize>,
    pub cancel_all_fail: Arc<AtomicBool>,
    pub preview_empty: Arc<AtomicBool>,
    pub preview_partial: Arc<AtomicBool>,
    pub replace_order_fail: Arc<AtomicBool>,
    pub replace_order_count: Arc<AtomicUsize>,
    pub open_orders_payload: Arc<tokio::sync::Mutex<Option<serde_json::Value>>>,
    pub fills_payload: Arc<tokio::sync::Mutex<Option<serde_json::Value>>>,
    pub positions_payload: Arc<tokio::sync::Mutex<Option<serde_json::Value>>>,
}

impl Default for TestServerState {
    fn default() -> Self {
        Self {
            connection_count: Arc::new(tokio::sync::Mutex::new(0)),
            subscriptions: Arc::new(tokio::sync::Mutex::new(Vec::new())),
            subscription_events: Arc::new(tokio::sync::Mutex::new(Vec::new())),
            fail_next_subscriptions: Arc::new(tokio::sync::Mutex::new(Vec::new())),
            authenticated: Arc::new(AtomicBool::new(false)),
            disconnect_trigger: Arc::new(AtomicBool::new(false)),
            ping_count: Arc::new(AtomicUsize::new(0)),
            pong_count: Arc::new(AtomicUsize::new(0)),
            heartbeat_count: Arc::new(AtomicUsize::new(0)),
            messages_received: Arc::new(tokio::sync::Mutex::new(Vec::new())),
            cancel_all_count: Arc::new(AtomicUsize::new(0)),
            cancel_all_fail: Arc::new(AtomicBool::new(false)),
            preview_empty: Arc::new(AtomicBool::new(false)),
            preview_partial: Arc::new(AtomicBool::new(false)),
            replace_order_fail: Arc::new(AtomicBool::new(false)),
            replace_order_count: Arc::new(AtomicUsize::new(0)),
            open_orders_payload: Arc::new(tokio::sync::Mutex::new(None)),
            fills_payload: Arc::new(tokio::sync::Mutex::new(None)),
            positions_payload: Arc::new(tokio::sync::Mutex::new(None)),
        }
    }
}

impl TestServerState {
    pub async fn reset(&self) {
        *self.connection_count.lock().await = 0;
        self.subscriptions.lock().await.clear();
        self.subscription_events.lock().await.clear();
        self.fail_next_subscriptions.lock().await.clear();
        self.authenticated.store(false, Ordering::Relaxed);
        self.disconnect_trigger.store(false, Ordering::Relaxed);
        self.ping_count.store(0, Ordering::Relaxed);
        self.pong_count.store(0, Ordering::Relaxed);
        self.heartbeat_count.store(0, Ordering::Relaxed);
        self.messages_received.lock().await.clear();
        self.cancel_all_count.store(0, Ordering::Relaxed);
    }

    pub async fn set_subscription_failures(&self, topics: Vec<String>) {
        *self.fail_next_subscriptions.lock().await = topics;
    }

    pub async fn subscription_events(&self) -> Vec<(String, bool)> {
        self.subscription_events.lock().await.clone()
    }

    pub async fn get_messages(&self) -> Vec<serde_json::Value> {
        self.messages_received.lock().await.clone()
    }
}

async fn handle_md_websocket(
    ws: WebSocketUpgrade,
    State(state): State<TestServerState>,
) -> Response {
    ws.on_upgrade(|socket| handle_md_socket(socket, state))
}

async fn handle_md_socket(mut socket: WebSocket, state: TestServerState) {
    {
        let mut count = state.connection_count.lock().await;
        *count += 1;
    }

    let state_clone = state.clone();

    let heartbeat_handle = tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(1));
        loop {
            interval.tick().await;

            if state_clone.disconnect_trigger.load(Ordering::Relaxed) {
                break;
            }
            state_clone.heartbeat_count.fetch_add(1, Ordering::Relaxed);
        }
    });

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

        match msg {
            Message::Text(text) => {
                let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
                    continue;
                };

                state.messages_received.lock().await.push(value.clone());

                let msg_type = value.get("type").and_then(|v| v.as_str());

                match msg_type {
                    Some("subscribe") => {
                        let symbol = value.get("symbol").and_then(|v| v.as_str()).unwrap_or("");
                        let level = value
                            .get("level")
                            .and_then(|v| v.as_str())
                            .unwrap_or("LEVEL_1");
                        let key = format!("{symbol}:{level}");

                        let fail_list = state.fail_next_subscriptions.lock().await.clone();
                        let should_fail = fail_list.contains(&key);

                        state
                            .subscription_events
                            .lock()
                            .await
                            .push((key.clone(), !should_fail));

                        if !should_fail {
                            let mut subs = state.subscriptions.lock().await;
                            if !subs.contains(&key) {
                                subs.push(key);
                            }
                        }

                        let book_msg = match level {
                            "LEVEL_1" => load_test_data("ws_md_book_l1.json"),
                            "LEVEL_2" => load_test_data("ws_md_book_l2.json"),
                            "LEVEL_3" => load_test_data("ws_md_book_l3.json"),
                            _ => load_test_data("ws_md_book_l1.json"),
                        };

                        if socket
                            .send(Message::Text(book_msg.to_string().into()))
                            .await
                            .is_err()
                        {
                            break;
                        }

                        let trade_msg = load_test_data("ws_md_trade.json");

                        if socket
                            .send(Message::Text(trade_msg.to_string().into()))
                            .await
                            .is_err()
                        {
                            break;
                        }
                    }
                    Some("unsubscribe") => {
                        let symbol = value.get("symbol").and_then(|v| v.as_str()).unwrap_or("");

                        let mut subs = state.subscriptions.lock().await;
                        subs.retain(|s| !s.starts_with(symbol));

                        let mut events = state.subscription_events.lock().await;
                        events.retain(|(t, _)| !t.starts_with(symbol));
                    }
                    Some("subscribe_candles") => {
                        let symbol = value.get("symbol").and_then(|v| v.as_str()).unwrap_or("");
                        let width = value.get("width").and_then(|v| v.as_str()).unwrap_or("1m");

                        let key = format!("{symbol}:candle:{width}");
                        state
                            .subscription_events
                            .lock()
                            .await
                            .push((key.clone(), true));

                        let mut subs = state.subscriptions.lock().await;
                        if !subs.contains(&key) {
                            subs.push(key);
                        }

                        // Send two candles with different timestamps to trigger bar emission
                        // (handler only emits when a candle closes, i.e., timestamp changes)
                        let candle1 = r#"{"t":"c","symbol":"EURUSD-PERP","width":"1m","open":"50000","high":"50100","low":"49900","close":"50050","volume":100,"buy_volume":60,"sell_volume":40,"ts":1234567890}"#;
                        if socket.send(Message::Text(candle1.into())).await.is_err() {
                            break;
                        }

                        let candle2 = r#"{"t":"c","symbol":"EURUSD-PERP","width":"1m","open":"50050","high":"50150","low":"49950","close":"50100","volume":110,"buy_volume":65,"sell_volume":45,"ts":1234567950}"#;
                        if socket.send(Message::Text(candle2.into())).await.is_err() {
                            break;
                        }
                    }
                    Some("unsubscribe_candles") => {
                        let symbol = value.get("symbol").and_then(|v| v.as_str()).unwrap_or("");

                        let mut subs = state.subscriptions.lock().await;
                        subs.retain(|s| !s.starts_with(&format!("{symbol}:candle")));
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
            Message::Pong(_) => {
                state.pong_count.fetch_add(1, Ordering::Relaxed);
            }
            Message::Close(_) => {
                break;
            }
            _ => {}
        }
    }

    heartbeat_handle.abort();

    let mut count = state.connection_count.lock().await;
    *count = count.saturating_sub(1);
}

async fn handle_orders_websocket(
    ws: WebSocketUpgrade,
    State(state): State<TestServerState>,
) -> Response {
    ws.on_upgrade(|socket| handle_orders_socket(socket, state))
}

async fn handle_orders_socket(mut socket: WebSocket, state: TestServerState) {
    {
        let mut count = state.connection_count.lock().await;
        *count += 1;
    }

    state.authenticated.store(true, Ordering::Relaxed);

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

        match msg {
            Message::Text(text) => {
                let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
                    continue;
                };

                state.messages_received.lock().await.push(value.clone());

                let msg_type = value.get("t").and_then(|v| v.as_str());

                match msg_type {
                    Some("p") => {
                        let rid = value.get("rid").and_then(|v| v.as_i64()).unwrap_or(0);
                        let ack = json!({
                            "t": "a",
                            "rid": rid,
                            "oid": format!("order-{rid}"),
                            "s": value.get("s").and_then(|v| v.as_str()).unwrap_or(""),
                            "d": value.get("d").and_then(|v| v.as_str()).unwrap_or("BUY"),
                            "q": value.get("q").and_then(|v| v.as_i64()).unwrap_or(0),
                            "p": value.get("p").and_then(|v| v.as_str()).unwrap_or("0"),
                        });

                        if socket
                            .send(Message::Text(ack.to_string().into()))
                            .await
                            .is_err()
                        {
                            break;
                        }
                    }
                    Some("x") => {
                        // CancelOrder request: ack with a cancel-response shape
                        let rid = value.get("rid").and_then(|v| v.as_i64()).unwrap_or(0);
                        let ack = json!({
                            "rid": rid,
                            "res": {
                                "cxl_rx": true,
                            },
                        });

                        if socket
                            .send(Message::Text(ack.to_string().into()))
                            .await
                            .is_err()
                        {
                            break;
                        }
                    }
                    Some("o") => {
                        let rid = value.get("rid").and_then(|v| v.as_i64()).unwrap_or(0);
                        let mut response = load_test_data("ws_orders_open_orders.json");
                        if let Some(obj) = response.as_object_mut() {
                            obj.insert("rid".to_string(), json!(rid));
                        }

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
            Message::Pong(_) => {
                state.pong_count.fetch_add(1, Ordering::Relaxed);
            }
            Message::Close(_) => {
                break;
            }
            _ => {}
        }
    }

    let mut count = state.connection_count.lock().await;
    *count = count.saturating_sub(1);
}

pub fn load_test_data(filename: &str) -> serde_json::Value {
    let path = format!("{}/test_data/{filename}", env!("CARGO_MANIFEST_DIR"));
    let content = std::fs::read_to_string(&path).unwrap_or_else(|_| match filename {
        "ws_md_book_l1.json" => r#"{"t":"1","s":"EURUSD-PERP","b":"50000.00","B":"1.0","a":"50001.00","A":"1.0","ts":"1234567890000000000"}"#.to_string(),
        "ws_md_book_l2.json" => r#"{"t":"2","s":"EURUSD-PERP","b":[],"a":[],"ts":"1234567890000000000"}"#.to_string(),
        "ws_md_book_l3.json" => r#"{"t":"3","s":"EURUSD-PERP","b":[],"a":[],"ts":"1234567890000000000"}"#.to_string(),
        "ws_md_trade.json" => r#"{"t":"s","s":"EURUSD-PERP","p":"50000.00","q":1,"d":"BUY","tx":"123","ts":"1234567890000000000"}"#.to_string(),
        "ws_md_candle.json" => r#"{"t":"c","s":"EURUSD-PERP","o":"50000","h":"50100","l":"49900","c":"50050","v":100,"ts":"1234567890000000000"}"#.to_string(),
        "ws_orders_open_orders.json" => r#"{"t":"O","orders":[]}"#.to_string(),
        _ => "{}".to_string(),
    });
    serde_json::from_str(&content).unwrap_or_else(|_| json!({}))
}

async fn handle_get_instruments() -> Json<serde_json::Value> {
    Json(load_test_data("http_get_instruments.json"))
}

async fn handle_get_balances() -> Json<serde_json::Value> {
    Json(load_test_data("http_get_balances.json"))
}

async fn handle_authenticate() -> Json<serde_json::Value> {
    Json(json!({
        "token": "mock_session_token_for_testing"
    }))
}

async fn handle_cancel_all_orders(
    State(state): State<TestServerState>,
) -> axum::response::Response {
    state.cancel_all_count.fetch_add(1, Ordering::Relaxed);
    if state.cancel_all_fail.load(Ordering::Relaxed) {
        (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error":"server error"})),
        )
            .into_response()
    } else {
        Json(load_test_data("http_cancel_all_orders.json")).into_response()
    }
}

async fn handle_preview_aggressive_limit_order(
    State(state): State<TestServerState>,
) -> Json<serde_json::Value> {
    if state.preview_empty.load(Ordering::Relaxed) {
        return Json(json!({
            "filled_quantity": 0,
            "remaining_quantity": 100,
            "limit_price": null,
            "vwap": null,
        }));
    }

    if state.preview_partial.load(Ordering::Relaxed) {
        return Json(json!({
            "filled_quantity": 40,
            "remaining_quantity": 60,
            "limit_price": "50001.00",
            "vwap": "50000.50",
        }));
    }
    Json(json!({
        "filled_quantity": 100,
        "remaining_quantity": 0,
        "limit_price": "50001.00",
        "vwap": "50000.50",
    }))
}

async fn handle_replace_order(
    State(state): State<TestServerState>,
    axum::Json(payload): axum::Json<serde_json::Value>,
) -> axum::response::Response {
    state.replace_order_count.fetch_add(1, Ordering::Relaxed);
    if state.replace_order_fail.load(Ordering::Relaxed) {
        return (
            axum::http::StatusCode::BAD_REQUEST,
            Json(json!({"error":"invalid modification"})),
        )
            .into_response();
    }
    let old_oid = payload
        .get("oid")
        .and_then(|v| v.as_str())
        .unwrap_or("OLD-OID");
    let new_oid = format!("{old_oid}-REPL");
    Json(json!({ "oid": new_oid })).into_response()
}

async fn handle_open_orders(State(state): State<TestServerState>) -> Json<serde_json::Value> {
    let guard = state.open_orders_payload.lock().await;
    if let Some(v) = guard.as_ref() {
        return Json(v.clone());
    }
    Json(load_test_data("http_get_open_orders.json"))
}

async fn handle_fills(State(state): State<TestServerState>) -> Json<serde_json::Value> {
    let guard = state.fills_payload.lock().await;
    if let Some(v) = guard.as_ref() {
        return Json(v.clone());
    }
    Json(load_test_data("http_get_fills.json"))
}

async fn handle_positions(State(state): State<TestServerState>) -> Json<serde_json::Value> {
    let guard = state.positions_payload.lock().await;
    if let Some(v) = guard.as_ref() {
        return Json(v.clone());
    }
    Json(load_test_data("http_get_positions.json"))
}

fn create_test_router(state: TestServerState) -> Router {
    Router::new()
        // WebSocket routes
        .route("/md/ws", get(handle_md_websocket))
        .route("/orders/ws", get(handle_orders_websocket))
        // HTTP API routes
        .route("/authenticate", post(handle_authenticate))
        .route("/instruments", get(handle_get_instruments))
        .route("/balances", get(handle_get_balances))
        .route("/positions", get(handle_positions))
        .route("/cancel_all_orders", post(handle_cancel_all_orders))
        .route(
            "/preview-aggressive-limit-order",
            post(handle_preview_aggressive_limit_order),
        )
        .route("/replace_order", post(handle_replace_order))
        .route("/open_orders", get(handle_open_orders))
        .route("/fills", get(handle_fills))
        .with_state(state)
}

pub async fn start_test_server()
-> Result<(SocketAddr, TestServerState), Box<dyn std::error::Error + Send + Sync>> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let state = TestServerState::default();
    let router = create_test_router(state.clone());

    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });

    wait_until_async(
        || async { tokio::net::TcpStream::connect(addr).await.is_ok() },
        Duration::from_secs(5),
    )
    .await;

    Ok((addr, state))
}

pub async fn wait_for_connection(state: &TestServerState) {
    wait_until_async(
        || async { *state.connection_count.lock().await > 0 },
        Duration::from_secs(5),
    )
    .await;
}

pub fn create_test_instrument(symbol: &str) -> InstrumentAny {
    let underlying = Ustr::from(symbol.split('-').next().unwrap_or(symbol));
    let instrument = PerpetualContract::new(
        InstrumentId::new(Symbol::new(symbol), Venue::new("AX")),
        Symbol::new(symbol),
        underlying,
        AssetClass::Cryptocurrency,
        None,
        Currency::USD(),
        Currency::USD(),
        false,
        2,
        3,
        Price::new(0.01, 2),
        Quantity::new(0.001, 3),
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        Some(Decimal::new(1, 2)),
        Some(Decimal::new(5, 3)),
        Some(Decimal::new(2, 4)),
        Some(Decimal::new(5, 4)),
        None,
        0.into(),
        0.into(),
    );
    InstrumentAny::PerpetualContract(instrument)
}
