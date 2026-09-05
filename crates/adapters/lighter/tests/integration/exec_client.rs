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

//! Integration tests for the [`LighterExecutionClient`].
//!
//! These tests stand up a unified Axum mock that serves the Lighter REST
//! endpoints (`/api/v1/orderBookDetails`, `/api/v1/nextNonce`,
//! `/api/v1/getMakerOnlyApiKeys`, `/api/v1/accountActiveOrders`,
//! `/api/v1/accountInactiveOrders`, `/api/v1/trades`) and the venue WebSocket (`/stream`). The
//! harness mirrors the data-client scaffolding in `tests/integration/data_client.rs`: the same
//! `TestServerState` records every inbound WS message, including signed
//! `jsonapi/sendtx` frames. Two primitives drive in-test pushes:
//! [`TestServerState::push_frame`] flushes a frame to the live socket via
//! a broadcast inbox, and `close_after_next_frame` arms a server-side
//! close so the WS layer's auto-reconnect path can be exercised.
//!
//! Coverage focuses on the public `ExecutionClient` trait surface and the
//! Lighter-specific invariants that live in `execution.rs` and
//! `websocket/dispatch.rs`: cloid registration, sendTx attribution,
//! TradeId-based fill dedup, empty-position snapshot replacement, and the
//! mass-status REST fan-out. Lower-level WS parsing and HTTP fixture
//! coverage lives in `tests/integration/websocket.rs` and `tests/integration/http.rs`.

use std::{
    cell::RefCell,
    collections::VecDeque,
    net::SocketAddr,
    path::PathBuf,
    process::Command,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicI64, AtomicU8, AtomicU64, AtomicUsize, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use axum::{
    Router,
    body::Bytes,
    extract::{
        Query, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use futures_util::{SinkExt, StreamExt};
use nautilus_common::{
    cache::Cache,
    clients::ExecutionClient,
    live::runner::{replace_exec_event_sender, replace_system_event_sender},
    messages::{
        ExecutionEvent, ExecutionReport, SystemEvent,
        execution::{
            BatchCancelOrders, CancelAllOrders, CancelOrder, GenerateFillReports,
            GenerateOrderStatusReport, GenerateOrderStatusReports, GeneratePositionStatusReports,
            ModifyOrder, SubmitOrder, SubmitOrderList,
        },
        system::SocketState,
    },
    testing::wait_until_async,
};
use nautilus_core::{UUID4, UnixNanos};
use nautilus_lighter::{
    common::{
        consts::{LIGHTER_NAUTILUS_INTEGRATOR_ACCOUNT_INDEX, LIGHTER_VENUE},
        enums::{LighterDeployment, LighterEnvironment},
    },
    config::LighterExecutionClientConfig,
    execution::LighterExecutionClient,
};
use nautilus_live::{ExecutionClientCore, SocketReconnectRegistry, SocketReconnectRequestOutcome};
use nautilus_model::{
    accounts::{AccountAny, MarginAccount},
    enums::{
        AccountType, OmsType, OrderSide, OrderStatus, OrderType, PositionSide, TimeInForce,
        TriggerType,
    },
    events::{AccountState, OrderAccepted, OrderEventAny, OrderPendingCancel, OrderPendingUpdate},
    identifiers::{
        AccountId, ClientId, ClientOrderId, InstrumentId, OrderListId, StrategyId, Symbol, TradeId,
        TraderId, Venue, VenueOrderId,
    },
    instruments::{CryptoPerpetual, CurrencyPair, InstrumentAny},
    orders::{Order, OrderAny, OrderList, OrderTestBuilder},
    types::{AccountBalance, Currency, Money, Price, Quantity},
};
use rstest::rstest;
use rust_decimal::Decimal;
use serde_json::{Value, json};
use ustr::Ustr;

const PRIVATE_KEY_HEX: &str =
    "0b8e0f63c24d8baacd9d29ad4e9a4b73c4a8d2bb8b16dc4fa9d7c2e1d3a8b1f0e8d3a4c5b6e7f001";
const LIGHTER_ENV_VARS: [&str; 6] = [
    "LIGHTER_API_KEY_INDEX",
    "LIGHTER_API_SECRET",
    "LIGHTER_ACCOUNT_INDEX",
    "LIGHTER_TESTNET_API_KEY_INDEX",
    "LIGHTER_TESTNET_API_SECRET",
    "LIGHTER_TESTNET_ACCOUNT_INDEX",
];
const TEST_ACCOUNT_INDEX: u64 = 12_345;
const TEST_API_KEY_INDEX: u8 = 5;
const ETH_PERP_SYMBOL: &str = "ETH-PERP";
const ETH_SPOT_SYMBOL: &str = "ETH/USDC-SPOT";
const TEST_MARKET_INDEX: i16 = 0;
const TEST_NEXT_NONCE: i64 = 9_999;
const TEST_ORDER_NONCE: i64 = 281_474_720_725_346;
const INTEGRATOR_APPROVAL_MAX_TTL_MS: i64 = 5 * 365 * 24 * 60 * 60 * 1_000;

fn data_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test_data")
}

fn load_text(filename: &str) -> String {
    std::fs::read_to_string(data_path().join(filename))
        .unwrap_or_else(|_| panic!("failed to read {filename}"))
}

fn load_json(filename: &str) -> Value {
    serde_json::from_str(&load_text(filename)).expect("invalid json")
}

fn eth_perp_id() -> InstrumentId {
    InstrumentId::from(format!("{ETH_PERP_SYMBOL}.LIGHTER").as_str())
}

fn eth_spot_id() -> InstrumentId {
    InstrumentId::from(format!("{ETH_SPOT_SYMBOL}.LIGHTER").as_str())
}

fn client_id() -> ClientId {
    ClientId::new("LIGHTER")
}

fn trader_id() -> TraderId {
    TraderId::from("TESTER-001")
}

fn strategy_id() -> StrategyId {
    StrategyId::from("S-001")
}

fn account_id() -> AccountId {
    AccountId::from("LIGHTER-001")
}

/// Shared mock-server state for the exec-client integration tests.
///
/// Records every inbound WS message (`subscribes`, `unsubscribes`,
/// `send_txs`), REST endpoint call counts, and trade queries. Tests inject
/// venue responses via the corresponding `*_response` and
/// `next_send_tx_ack` overrides, push synthetic frames through `inbox_tx`
/// (consumed via [`Self::push_frame`]), and arm a server-side close by
/// toggling `close_after_next_frame` so the WS layer's auto-reconnect path can
/// be exercised.
#[derive(Clone)]
struct TestServerState {
    connection_count: Arc<tokio::sync::Mutex<usize>>,
    subscribes: Arc<tokio::sync::Mutex<Vec<Value>>>,
    unsubscribes: Arc<tokio::sync::Mutex<Vec<Value>>>,
    send_txs: Arc<tokio::sync::Mutex<Vec<Value>>>,
    rest_send_txs: Arc<tokio::sync::Mutex<Vec<Value>>>,
    account_type: Arc<AtomicU8>,
    maker_only_calls: Arc<AtomicUsize>,
    maker_only_api_key_indexes: Arc<tokio::sync::Mutex<Vec<i64>>>,
    maker_only_authorizations: Arc<tokio::sync::Mutex<Vec<String>>>,
    referral_use_calls: Arc<AtomicUsize>,
    referral_use_authorizations: Arc<tokio::sync::Mutex<Vec<String>>>,
    referral_use_requests: Arc<tokio::sync::Mutex<Vec<std::collections::HashMap<String, String>>>>,
    next_referral_use_response: Arc<tokio::sync::Mutex<Option<Value>>>,
    active_orders_calls: Arc<AtomicUsize>,
    tx_calls: Arc<AtomicUsize>,
    inactive_orders_calls: Arc<AtomicUsize>,
    trades_calls: Arc<AtomicUsize>,
    trades_queries: Arc<tokio::sync::Mutex<Vec<std::collections::HashMap<String, String>>>>,
    active_orders_response: Arc<tokio::sync::Mutex<Option<Value>>>,
    active_orders_responses: Arc<tokio::sync::Mutex<VecDeque<Value>>>,
    tx_responses: Arc<tokio::sync::Mutex<VecDeque<Value>>>,
    tx_response_blocked: Arc<AtomicBool>,
    tx_response_release: Arc<tokio::sync::Notify>,
    inactive_orders_response: Arc<tokio::sync::Mutex<Option<Value>>>,
    inactive_orders_unscoped_response: Arc<tokio::sync::Mutex<Option<Value>>>,
    trades_response: Arc<tokio::sync::Mutex<Option<Value>>>,
    trades_responses: Arc<tokio::sync::Mutex<VecDeque<Value>>>,
    next_rest_send_tx_response: Arc<tokio::sync::Mutex<Option<Value>>>,
    next_send_tx_ack: Arc<tokio::sync::Mutex<Option<Value>>>,
    inbox_tx: tokio::sync::broadcast::Sender<String>,
    close_after_next_frame: Arc<AtomicBool>,
    subscribe_ack_delay_ms: Arc<AtomicU64>,
    tx_hash_seq: Arc<AtomicI64>,
    // Mirrors the real venue contract: after each `account_all_*` subscribe
    // ack the venue emits a typed `subscribed/account_all_*` frame so the
    // execution client can clear the strict-await readiness gate even on a
    // fresh account. Disable in tests that want to drive readiness manually.
    auto_emit_account_subscribed_frames: Arc<AtomicBool>,
}

impl Default for TestServerState {
    fn default() -> Self {
        let (inbox_tx, _) = tokio::sync::broadcast::channel(64);
        Self {
            connection_count: Arc::new(tokio::sync::Mutex::new(0)),
            subscribes: Arc::new(tokio::sync::Mutex::new(Vec::new())),
            unsubscribes: Arc::new(tokio::sync::Mutex::new(Vec::new())),
            send_txs: Arc::new(tokio::sync::Mutex::new(Vec::new())),
            rest_send_txs: Arc::new(tokio::sync::Mutex::new(Vec::new())),
            account_type: Arc::new(AtomicU8::new(0)),
            maker_only_calls: Arc::new(AtomicUsize::new(0)),
            maker_only_api_key_indexes: Arc::new(tokio::sync::Mutex::new(Vec::new())),
            maker_only_authorizations: Arc::new(tokio::sync::Mutex::new(Vec::new())),
            referral_use_calls: Arc::new(AtomicUsize::new(0)),
            referral_use_authorizations: Arc::new(tokio::sync::Mutex::new(Vec::new())),
            referral_use_requests: Arc::new(tokio::sync::Mutex::new(Vec::new())),
            next_referral_use_response: Arc::new(tokio::sync::Mutex::new(None)),
            active_orders_calls: Arc::new(AtomicUsize::new(0)),
            tx_calls: Arc::new(AtomicUsize::new(0)),
            inactive_orders_calls: Arc::new(AtomicUsize::new(0)),
            trades_calls: Arc::new(AtomicUsize::new(0)),
            trades_queries: Arc::new(tokio::sync::Mutex::new(Vec::new())),
            active_orders_response: Arc::new(tokio::sync::Mutex::new(None)),
            active_orders_responses: Arc::new(tokio::sync::Mutex::new(VecDeque::new())),
            tx_responses: Arc::new(tokio::sync::Mutex::new(VecDeque::new())),
            tx_response_blocked: Arc::new(AtomicBool::new(false)),
            tx_response_release: Arc::new(tokio::sync::Notify::new()),
            inactive_orders_response: Arc::new(tokio::sync::Mutex::new(None)),
            inactive_orders_unscoped_response: Arc::new(tokio::sync::Mutex::new(None)),
            trades_response: Arc::new(tokio::sync::Mutex::new(None)),
            trades_responses: Arc::new(tokio::sync::Mutex::new(VecDeque::new())),
            next_rest_send_tx_response: Arc::new(tokio::sync::Mutex::new(None)),
            next_send_tx_ack: Arc::new(tokio::sync::Mutex::new(None)),
            inbox_tx,
            close_after_next_frame: Arc::new(AtomicBool::new(false)),
            subscribe_ack_delay_ms: Arc::new(AtomicU64::new(0)),
            tx_hash_seq: Arc::new(AtomicI64::new(0)),
            auto_emit_account_subscribed_frames: Arc::new(AtomicBool::new(true)),
        }
    }
}

impl TestServerState {
    async fn subscribes(&self) -> Vec<Value> {
        self.subscribes.lock().await.clone()
    }

    async fn send_txs(&self) -> Vec<Value> {
        self.send_txs.lock().await.clone()
    }

    async fn rest_send_txs(&self) -> Vec<Value> {
        self.rest_send_txs.lock().await.clone()
    }

    async fn maker_only_authorizations(&self) -> Vec<String> {
        self.maker_only_authorizations.lock().await.clone()
    }

    async fn referral_use_requests(&self) -> Vec<std::collections::HashMap<String, String>> {
        self.referral_use_requests.lock().await.clone()
    }

    fn push_frame(&self, frame: &Value) {
        let _ = self.inbox_tx.send(frame.to_string());
    }
}

async fn order_book_details() -> Response {
    (StatusCode::OK, load_text("http_order_book_details.json")).into_response()
}

async fn account(State(state): State<Arc<TestServerState>>) -> Response {
    let mut response = load_json("http_account.json");
    response["accounts"][0]["account_type"] =
        Value::from(state.account_type.load(Ordering::Relaxed));
    (StatusCode::OK, response.to_string()).into_response()
}

async fn next_nonce() -> Response {
    // Always return the same nonce baseline. The execution client refreshes
    // on connect and again on reconnect; both fetches resolve to this value.
    (
        StatusCode::OK,
        json!({
            "code": 200,
            "nonce": TEST_NEXT_NONCE,
        })
        .to_string(),
    )
        .into_response()
}

async fn maker_only_api_keys(
    State(state): State<Arc<TestServerState>>,
    headers: HeaderMap,
    Query(query): Query<std::collections::HashMap<String, String>>,
) -> Response {
    state.maker_only_calls.fetch_add(1, Ordering::Relaxed);
    let authorization = headers
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_string();

    if authorization.is_empty()
        || query.get("account_index").map(String::as_str) != Some("12345")
        || query.contains_key("auth")
        || query.contains_key("authorization")
    {
        return (
            StatusCode::BAD_REQUEST,
            json!({"code":400,"message":"unexpected maker-only request"}).to_string(),
        )
            .into_response();
    }

    state
        .maker_only_authorizations
        .lock()
        .await
        .push(authorization);
    let api_key_indexes = state.maker_only_api_key_indexes.lock().await.clone();

    (
        StatusCode::OK,
        json!({"code":200,"api_key_indexes":api_key_indexes}).to_string(),
    )
        .into_response()
}

async fn referral_use(
    State(state): State<Arc<TestServerState>>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    state.referral_use_calls.fetch_add(1, Ordering::Relaxed);
    let authorization = headers
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_string();
    let content_type = headers
        .get("content-type")
        .and_then(|value| value.to_str().ok());
    let fields = url::form_urlencoded::parse(&body)
        .into_owned()
        .collect::<std::collections::HashMap<String, String>>();

    if authorization.is_empty()
        || content_type != Some("application/x-www-form-urlencoded")
        || fields.get("l1_address").map(String::as_str)
            != Some("0x0000000000000000000000000000000000000000")
        || fields.get("referral_code").map(String::as_str) != Some("NAUTILUS")
    {
        return (
            StatusCode::BAD_REQUEST,
            json!({"code":400,"message":"unexpected referral use request"}).to_string(),
        )
            .into_response();
    }

    state
        .referral_use_authorizations
        .lock()
        .await
        .push(authorization);
    state.referral_use_requests.lock().await.push(fields);

    if let Some(response) = state.next_referral_use_response.lock().await.take() {
        return (StatusCode::OK, response.to_string()).into_response();
    }
    (
        StatusCode::OK,
        json!({"code":200,"message":null}).to_string(),
    )
        .into_response()
}

async fn account_active_orders(
    State(state): State<Arc<TestServerState>>,
    Query(_query): Query<std::collections::HashMap<String, String>>,
) -> Response {
    state.active_orders_calls.fetch_add(1, Ordering::Relaxed);
    if let Some(body) = state.active_orders_responses.lock().await.pop_front() {
        return (StatusCode::OK, body.to_string()).into_response();
    }

    if let Some(body) = state.active_orders_response.lock().await.clone() {
        return (StatusCode::OK, body.to_string()).into_response();
    }
    (StatusCode::OK, json!({"code":200,"orders":[]}).to_string()).into_response()
}

async fn tx(
    State(state): State<Arc<TestServerState>>,
    Query(query): Query<std::collections::HashMap<String, String>>,
) -> Response {
    state.tx_calls.fetch_add(1, Ordering::Relaxed);
    if state.tx_response_blocked.load(Ordering::Acquire) {
        state.tx_response_release.notified().await;
    }
    let Some(mut body) = state.tx_responses.lock().await.pop_front() else {
        return (
            StatusCode::BAD_REQUEST,
            json!({"code":21500,"message":"transaction not found"}).to_string(),
        )
            .into_response();
    };
    let Some(tx_hash) = query
        .get("value")
        .filter(|_| query.get("by").map(String::as_str) == Some("hash"))
    else {
        return (
            StatusCode::BAD_REQUEST,
            json!({"code":20001,"message":"unexpected transaction query"}).to_string(),
        )
            .into_response();
    };
    let frames = state.send_txs.lock().await;
    let Some(frame) = frames.last() else {
        return (
            StatusCode::BAD_REQUEST,
            json!({"code":21500,"message":"transaction not found"}).to_string(),
        )
            .into_response();
    };
    let info = send_tx_info(frame);
    let object = body
        .as_object_mut()
        .expect("transaction fixture must be an object");
    object.insert("code".to_string(), json!(200));
    object.insert("hash".to_string(), json!(tx_hash));
    object.insert("type".to_string(), json!(14));
    object.insert("info".to_string(), json!(info.to_string()));
    object.insert("account_index".to_string(), info["AccountIndex"].clone());
    object.insert("api_key_index".to_string(), info["ApiKeyIndex"].clone());
    object.insert("nonce".to_string(), info["Nonce"].clone());
    (StatusCode::OK, body.to_string()).into_response()
}

async fn account_inactive_orders(
    State(state): State<Arc<TestServerState>>,
    Query(query): Query<std::collections::HashMap<String, String>>,
) -> Response {
    state.inactive_orders_calls.fetch_add(1, Ordering::Relaxed);
    if !query.contains_key("market_id")
        && let Some(body) = state.inactive_orders_unscoped_response.lock().await.clone()
    {
        return (StatusCode::OK, body.to_string()).into_response();
    }

    if let Some(body) = state.inactive_orders_response.lock().await.clone() {
        return (StatusCode::OK, body.to_string()).into_response();
    }
    (StatusCode::OK, json!({"code":200,"orders":[]}).to_string()).into_response()
}

async fn trades(
    State(state): State<Arc<TestServerState>>,
    Query(query): Query<std::collections::HashMap<String, String>>,
) -> Response {
    state.trades_calls.fetch_add(1, Ordering::Relaxed);
    state.trades_queries.lock().await.push(query);
    if let Some(body) = state.trades_responses.lock().await.pop_front() {
        return (StatusCode::OK, body.to_string()).into_response();
    }

    if let Some(body) = state.trades_response.lock().await.clone() {
        return (StatusCode::OK, body.to_string()).into_response();
    }
    (StatusCode::OK, json!({"code":200,"trades":[]}).to_string()).into_response()
}

async fn handle_ws_upgrade(
    ws: WebSocketUpgrade,
    State(state): State<Arc<TestServerState>>,
) -> Response {
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

/// Build the typed `subscribed/account_all_*` payload the venue emits after
/// each account subscribe ack. Returns `None` for non-account channels.
fn account_subscribed_frame(channel: &str) -> Option<Value> {
    if channel.starts_with("account_all_orders:") {
        Some(json!({
            "type": "subscribed/account_all_orders",
            "channel": channel,
            "orders": {},
        }))
    } else if channel.starts_with("account_all_trades:") {
        Some(json!({
            "type": "subscribed/account_all_trades",
            "channel": channel,
            "trades": [],
            "total_volume": "0",
            "monthly_volume": "0",
            "weekly_volume": "0",
            "daily_volume": "0",
        }))
    } else if channel.starts_with("account_all_positions:") {
        Some(json!({
            "type": "subscribed/account_all_positions",
            "channel": channel,
            "positions": {},
            "shares": [],
        }))
    } else if channel.starts_with("account_all_assets:") {
        Some(json!({
            "type": "subscribed/account_all_assets",
            "channel": channel,
            "assets": {},
            "timestamp": 1_700_000_000_000_u64,
        }))
    } else if channel.starts_with("user_stats:") {
        Some(json!({
            "type": "subscribed/user_stats",
            "channel": channel,
            "stats": {
                "account_trading_mode": 0,
                "available_balance": "0",
                "buying_power": "0",
                "collateral": "0",
                "leverage": "0",
                "margin_usage": "0",
                "portfolio_value": "0"
            },
            "timestamp": 1_700_000_000_000_u64,
        }))
    } else {
        None
    }
}

async fn handle_socket(socket: WebSocket, state: Arc<TestServerState>) {
    {
        let mut count = state.connection_count.lock().await;
        *count += 1;
    }

    let (mut sink, mut stream) = socket.split();
    let _ = sink
        .send(Message::Text(
            json!({"type":"connected"}).to_string().into(),
        ))
        .await;

    let mut inbox = state.inbox_tx.subscribe();

    loop {
        tokio::select! {
            biased;
            // Direct frame pushes from tests. The broadcast channel may
            // surface lagged errors when many frames are queued before
            // the socket subscribes; those are non-fatal so the loop
            // continues.
            inbox_msg = inbox.recv() => {
                match inbox_msg {
                    Ok(frame) => {
                        if sink.send(Message::Text(frame.into())).await.is_err() {
                            break;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {}
                }
            }
            // Inbound from the client.
            next = stream.next() => {
                let Some(Ok(message)) = next else { break };
                match message {
            Message::Text(text) => {
                let Ok(value) = serde_json::from_str::<Value>(&text) else {
                    continue;
                };
                let kind = value.get("type").and_then(Value::as_str).unwrap_or("");
                let should_close = state.close_after_next_frame.swap(false, Ordering::Relaxed);

                match kind {
                    "subscribe" => {
                        state.subscribes.lock().await.push(value.clone());

                        let channel = value
                            .get("channel")
                            .and_then(Value::as_str)
                            .map(|s| s.replace('/', ":"))
                            .unwrap_or_default();

                        let ack_delay_ms =
                            state.subscribe_ack_delay_ms.load(Ordering::Relaxed);

                        if ack_delay_ms > 0 {
                            tokio::time::sleep(Duration::from_millis(ack_delay_ms)).await;
                        }
                        let ack = json!({"type":"subscribed", "channel": channel});
                        if sink
                            .send(Message::Text(ack.to_string().into()))
                            .await
                            .is_err()
                        {
                            break;
                        }

                        if state
                            .auto_emit_account_subscribed_frames
                            .load(Ordering::Relaxed)
                            && let Some(typed) = account_subscribed_frame(&channel)
                            && sink
                                .send(Message::Text(typed.to_string().into()))
                                .await
                                .is_err()
                        {
                            break;
                        }
                    }
                    "unsubscribe" => {
                        state.unsubscribes.lock().await.push(value.clone());

                        let channel = value
                            .get("channel")
                            .and_then(Value::as_str)
                            .map(|s| s.replace('/', ":"))
                            .unwrap_or_default();

                        let ack = json!({"type":"unsubscribed", "channel": channel});
                        if sink
                            .send(Message::Text(ack.to_string().into()))
                            .await
                            .is_err()
                        {
                            break;
                        }
                    }
                    "jsonapi/sendtx" => {
                        state.send_txs.lock().await.push(value);

                        // Ack so the handler clears the pending sendTx head.
                        // No `tx_hash`: the mock cannot recompute the
                        // Poseidon hash, and a fabricated one would go
                        // unattributed. Tests drive venue rejections via the
                        // `next_send_tx_ack` override.
                        let ack = state
                            .next_send_tx_ack
                            .lock()
                            .await
                            .take()
                            .unwrap_or_else(|| {
                                json!({
                                    "type": "jsonapi/sendtx",
                                    "code": 200,
                                })
                            });

                        if sink
                            .send(Message::Text(ack.to_string().into()))
                            .await
                            .is_err()
                        {
                            break;
                        }
                    }
                    _ => {}
                }

                if should_close {
                    let _ = sink.send(Message::Close(None)).await;
                    break;
                }
            }
            Message::Ping(payload) if sink.send(Message::Pong(payload.clone())).await.is_err() => {
                break;
            }
            Message::Close(_) => break,
            _ => {}
                }
            }
        }
    }

    let mut count = state.connection_count.lock().await;
    *count = count.saturating_sub(1);
}

fn build_router(state: Arc<TestServerState>) -> Router {
    Router::new()
        .route("/api/v1/orderBookDetails", get(order_book_details))
        .route("/api/v1/account", get(account))
        .route("/api/v1/nextNonce", get(next_nonce))
        .route("/api/v1/getMakerOnlyApiKeys", get(maker_only_api_keys))
        .route("/api/v1/referral/use", post(referral_use))
        .route("/api/v1/accountActiveOrders", get(account_active_orders))
        .route(
            "/api/v1/accountInactiveOrders",
            get(account_inactive_orders),
        )
        .route("/api/v1/trades", get(trades))
        .route("/api/v1/tx", get(tx))
        .route("/api/v1/sendTx", post(send_tx_post_stub))
        .route("/api/v1/sendTxBatch", post(send_tx_batch_post_stub))
        .route("/stream", get(handle_ws_upgrade))
        .with_state(state)
}

async fn send_tx_post_stub(State(state): State<Arc<TestServerState>>, body: Bytes) -> Response {
    let body = String::from_utf8_lossy(&body);
    let tx_type: u8 = multipart_field(&body, "tx_type")
        .parse()
        .expect("tx_type field must be a u8");
    let tx_info: Value =
        serde_json::from_str(&multipart_field(&body, "tx_info")).expect("tx_info must be JSON");

    state
        .rest_send_txs
        .lock()
        .await
        .push(json!({"tx_type": tx_type, "tx_info": tx_info}));

    let response = state
        .next_rest_send_tx_response
        .lock()
        .await
        .take()
        .unwrap_or_else(|| {
            json!({
                "code": 200,
                "tx_hash": "deadbeef",
                "predicted_execution_time_ms": 1,
                "volume_quota_remaining": 123,
            })
        });

    (StatusCode::OK, response.to_string()).into_response()
}

async fn send_tx_batch_post_stub(
    State(state): State<Arc<TestServerState>>,
    body: Bytes,
) -> Response {
    let body = String::from_utf8_lossy(&body);
    let tx_types: Value = serde_json::from_str(&multipart_field(&body, "tx_types"))
        .expect("tx_types field must be JSON");
    let tx_infos: Value = serde_json::from_str(&multipart_field(&body, "tx_infos"))
        .expect("tx_infos field must be JSON");
    let tx_count = tx_types.as_array().map_or(0, Vec::len);

    state.send_txs.lock().await.push(
        json!({"type":"jsonapi/sendtxbatch","data":{"tx_types":tx_types,"tx_infos":tx_infos}}),
    );

    let ack = state
        .next_send_tx_ack
        .lock()
        .await
        .take()
        .unwrap_or_else(|| {
            let start = state
                .tx_hash_seq
                .fetch_add(tx_count as i64, Ordering::Relaxed);
            let tx_hashes = (0..tx_count)
                .map(|i| Value::String(format!("0000{:016x}", start + i as i64)))
                .collect::<Vec<_>>();
            json!({
                "code": 200,
                "tx_hash": tx_hashes,
                "predicted_execution_time_ms": 1,
                "volume_quota_remaining": 123,
            })
        });

    (StatusCode::OK, ack.to_string()).into_response()
}

fn multipart_field(body: &str, name: &str) -> String {
    let marker = format!("name=\"{name}\"");
    let after_name = body
        .split(&marker)
        .nth(1)
        .unwrap_or_else(|| panic!("missing multipart field {name}"));
    let after_header = after_name
        .split("\r\n\r\n")
        .nth(1)
        .unwrap_or_else(|| panic!("missing multipart value for {name}"));
    after_header
        .split("\r\n--")
        .next()
        .unwrap_or_default()
        .to_string()
}

async fn start_server() -> (SocketAddr, Arc<TestServerState>) {
    let state = Arc::new(TestServerState::default());
    let router = build_router(Arc::clone(&state));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind listener");
    let addr = listener.local_addr().expect("local addr");
    tokio::spawn(async move {
        axum::serve(listener, router).await.expect("serve");
    });
    wait_until_async(
        || async { tokio::net::TcpStream::connect(addr).await.is_ok() },
        Duration::from_secs(2),
    )
    .await;
    (addr, state)
}

fn build_config(addr: SocketAddr) -> LighterExecutionClientConfig {
    // Pin every credential field explicitly so a stray `LIGHTER_*` env var
    // cannot leak into a test.
    LighterExecutionClientConfig {
        account_id: account_id(),
        account_index: Some(TEST_ACCOUNT_INDEX),
        api_key_index: Some(TEST_API_KEY_INDEX),
        private_key: Some(PRIVATE_KEY_HEX.into()),
        base_url_http: Some(format!("http://{addr}")),
        base_url_ws: Some(format!("ws://{addr}/stream")),
        proxy_url: None,
        environment: LighterEnvironment::Testnet,
        deployment: Default::default(),
        venue: None,
        http_timeout_secs: 5,
        ws_timeout_secs: 5,
        market_order_slippage_bps: 50,
        rest_quota_per_min: None,
        sendtx_quota_per_min: None,
        transport_backend: Default::default(),
    }
}

fn build_config_no_credentials(addr: SocketAddr) -> LighterExecutionClientConfig {
    LighterExecutionClientConfig {
        private_key: None,
        account_index: None,
        api_key_index: None,
        ..build_config(addr)
    }
}

fn test_perp_instrument() -> InstrumentAny {
    InstrumentAny::CryptoPerpetual(
        CryptoPerpetual::builder()
            .instrument_id(eth_perp_id())
            .raw_symbol(Symbol::new(ETH_PERP_SYMBOL))
            .base_currency(Currency::from("ETH"))
            .quote_currency(Currency::from("USDC"))
            .settlement_currency(Currency::from("USDC"))
            .is_inverse(false)
            .price_precision(2)
            .size_precision(4)
            .price_increment(Price::from("0.01"))
            .size_increment(Quantity::from("0.0001"))
            .min_notional(Money::from("10.000000 USDC"))
            .ts_event(UnixNanos::default())
            .ts_init(UnixNanos::default())
            .build()
            .unwrap(),
    )
}

fn test_spot_instrument() -> InstrumentAny {
    InstrumentAny::CurrencyPair(
        CurrencyPair::builder()
            .instrument_id(eth_spot_id())
            .raw_symbol(Symbol::new("ETH/USDC"))
            .base_currency(Currency::from("ETH"))
            .quote_currency(Currency::from("USDC"))
            .price_precision(4)
            .size_precision(2)
            .price_increment(Price::from("0.0001"))
            .size_increment(Quantity::from("0.01"))
            .min_quantity(Quantity::from("0.01"))
            .min_notional(Money::from("1.0000 USDC"))
            .ts_event(UnixNanos::default())
            .ts_init(UnixNanos::default())
            .build()
            .unwrap(),
    )
}

fn build_cache_with_account_and_instrument() -> Rc<RefCell<Cache>> {
    let cache = Rc::new(RefCell::new(Cache::default()));
    let instrument = test_perp_instrument();
    cache
        .borrow_mut()
        .add_instrument(instrument)
        .expect("add instrument");
    add_test_account(&cache);
    cache
}

fn add_test_account(cache: &Rc<RefCell<Cache>>) {
    let state = AccountState::new(
        account_id(),
        AccountType::Margin,
        vec![AccountBalance::new(
            Money::from("10000.000000 USDC"),
            Money::from("0.000000 USDC"),
            Money::from("10000.000000 USDC"),
        )],
        vec![],
        true,
        UUID4::new(),
        UnixNanos::default(),
        UnixNanos::default(),
        None,
    );
    let account = AccountAny::Margin(MarginAccount::new(state, true));
    cache
        .borrow_mut()
        .add_account(account)
        .expect("add account");
}

fn build_client(
    addr: SocketAddr,
) -> (
    LighterExecutionClient,
    tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
    Rc<RefCell<Cache>>,
) {
    build_client_with(build_config(addr))
}

fn build_client_mainnet(
    addr: SocketAddr,
) -> (
    LighterExecutionClient,
    tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
    Rc<RefCell<Cache>>,
) {
    let mut config = build_config(addr);
    config.environment = LighterEnvironment::Mainnet;
    build_client_with(config)
}

fn build_client_robinhood_mainnet(
    addr: SocketAddr,
) -> (
    LighterExecutionClient,
    tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
    Rc<RefCell<Cache>>,
) {
    let mut config = build_config(addr);
    config.environment = LighterEnvironment::Mainnet;
    config.deployment = LighterDeployment::Robinhood;
    config.venue = Some(*LIGHTER_VENUE);
    build_client_with(config)
}

fn build_client_with(
    config: LighterExecutionClientConfig,
) -> (
    LighterExecutionClient,
    tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
    Rc<RefCell<Cache>>,
) {
    let cache = build_cache_with_account_and_instrument();
    build_client_with_cache(config, cache)
}

fn build_client_with_cache(
    config: LighterExecutionClientConfig,
    cache: Rc<RefCell<Cache>>,
) -> (
    LighterExecutionClient,
    tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
    Rc<RefCell<Cache>>,
) {
    // Installing a fresh sender per test isolates the channel from any
    // prior test that ran on this thread; mirrors `data_client.rs`.
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    replace_exec_event_sender(sender);

    let venue = config.resolved_venue();
    let core = ExecutionClientCore::new(
        trader_id(),
        client_id(),
        venue,
        OmsType::Netting,
        config.account_id,
        AccountType::Margin,
        None,
        cache.clone(),
    );
    let mut client = LighterExecutionClient::new(core, config).expect("construct exec client");
    client.start().expect("start client");
    (client, receiver, cache)
}

async fn next_event_matching<F>(
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
    timeout: Duration,
    mut predicate: F,
) -> Option<ExecutionEvent>
where
    F: FnMut(&ExecutionEvent) -> bool,
{
    let started = std::time::Instant::now();
    loop {
        let remaining = timeout.checked_sub(started.elapsed())?;
        match tokio::time::timeout(remaining, rx.recv()).await {
            Ok(Some(event)) => {
                if predicate(&event) {
                    return Some(event);
                }
            }
            Ok(None) | Err(_) => return None,
        }
    }
}

async fn next_order_event(
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
    timeout: Duration,
) -> Option<OrderEventAny> {
    let event = next_event_matching(rx, timeout, |e| matches!(e, ExecutionEvent::Order(_))).await?;
    if let ExecutionEvent::Order(order_event) = event {
        Some(order_event)
    } else {
        None
    }
}

async fn await_send_tx_count(state: &TestServerState, target: usize) {
    wait_until_async(
        || {
            let state = state.clone();
            async move { state.send_txs.lock().await.len() >= target }
        },
        Duration::from_secs(5),
    )
    .await;
}

async fn await_tx_calls(state: &TestServerState, target: usize) {
    wait_until_async(
        || {
            let state = state.clone();
            async move { state.tx_calls.load(Ordering::Relaxed) >= target }
        },
        Duration::from_secs(8),
    )
    .await;
}

async fn await_subscribe_count(state: &TestServerState, target: usize) {
    wait_until_async(
        || {
            let state = state.clone();
            async move { state.subscribes.lock().await.len() >= target }
        },
        Duration::from_secs(5),
    )
    .await;
}

async fn await_connection_count(state: &TestServerState, target: usize) {
    wait_until_async(
        || {
            let state = state.clone();
            async move { *state.connection_count.lock().await == target }
        },
        Duration::from_secs(5),
    )
    .await;
}

async fn assert_local_order_denied_once(
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
    state: &TestServerState,
    reason_part: &str,
) -> String {
    let event = next_order_event(rx, Duration::from_secs(2))
        .await
        .expect("expected denied event");
    let reason = match event {
        OrderEventAny::Denied(d) => {
            assert!(
                d.reason.as_str().contains(reason_part),
                "expected reason containing `{reason_part}`, was {:?}",
                d.reason,
            );
            assert!(
                [
                    "INSTRUMENT_NOT_FOUND:",
                    "SUBMIT_FAILED:",
                    "UNSUPPORTED_ORDER_LIST:",
                    "UNSUPPORTED_ORDER_TYPE:",
                    "UNSUPPORTED_TIME_IN_FORCE:",
                    "VALIDATION_FAILED:",
                ]
                .iter()
                .any(|prefix| d.reason.as_str().starts_with(prefix)),
                "expected standardized denial code, was {:?}",
                d.reason,
            );
            d.reason.to_string()
        }
        other => panic!("expected OrderDenied, was {other:?}"),
    };

    assert!(
        next_order_event(rx, Duration::from_millis(100))
            .await
            .is_none(),
        "local denial should emit exactly one order event",
    );
    assert_eq!(state.send_txs().await.len(), 0);
    reason
}

fn make_limit_order(
    id: &str,
    side: OrderSide,
    qty: Quantity,
    price: Price,
    tif: TimeInForce,
    post_only: bool,
    reduce_only: bool,
) -> OrderAny {
    OrderTestBuilder::new(OrderType::Limit)
        .trader_id(trader_id())
        .strategy_id(strategy_id())
        .instrument_id(eth_perp_id())
        .client_order_id(ClientOrderId::from(id))
        .side(side)
        .quantity(qty)
        .price(price)
        .time_in_force(tif)
        .post_only(post_only)
        .reduce_only(reduce_only)
        .build()
}

fn make_limit_order_with_quantity_options(
    id: &str,
    quote_quantity: bool,
    display_qty: Option<Quantity>,
) -> OrderAny {
    let mut builder = OrderTestBuilder::new(OrderType::Limit);
    builder
        .trader_id(trader_id())
        .strategy_id(strategy_id())
        .instrument_id(eth_perp_id())
        .client_order_id(ClientOrderId::from(id))
        .side(OrderSide::Buy)
        .quantity(Quantity::from("0.0050"))
        .price(Price::from("2361.31"))
        .quote_quantity(quote_quantity);

    if let Some(display_qty) = display_qty {
        builder.display_qty(display_qty);
    }

    builder.build()
}

fn make_market_order(id: &str, side: OrderSide, qty: Quantity) -> OrderAny {
    OrderTestBuilder::new(OrderType::Market)
        .trader_id(trader_id())
        .strategy_id(strategy_id())
        .instrument_id(eth_perp_id())
        .client_order_id(ClientOrderId::from(id))
        .side(side)
        .quantity(qty)
        .time_in_force(TimeInForce::Ioc)
        .build()
}

fn make_stop_market_order(id: &str, side: OrderSide, qty: Quantity, trigger: Price) -> OrderAny {
    make_conditional_order_for(
        eth_perp_id(),
        OrderType::StopMarket,
        id,
        side,
        qty,
        trigger,
        TimeInForce::Gtc,
    )
}

fn make_conditional_order_for(
    instrument_id: InstrumentId,
    order_type: OrderType,
    id: &str,
    side: OrderSide,
    qty: Quantity,
    trigger: Price,
    tif: TimeInForce,
) -> OrderAny {
    assert!(
        matches!(
            order_type,
            OrderType::StopMarket
                | OrderType::StopLimit
                | OrderType::MarketIfTouched
                | OrderType::LimitIfTouched
        ),
        "expected conditional order type, was {order_type:?}",
    );

    OrderTestBuilder::new(order_type)
        .trader_id(trader_id())
        .strategy_id(strategy_id())
        .instrument_id(instrument_id)
        .client_order_id(ClientOrderId::from(id))
        .side(side)
        .quantity(qty)
        .price(Price::from("2401.00"))
        .trigger_price(trigger)
        .trigger_type(TriggerType::Default)
        .time_in_force(tif)
        .build()
}

fn make_stop_market_order_with_tif(
    id: &str,
    side: OrderSide,
    qty: Quantity,
    trigger: Price,
    tif: TimeInForce,
) -> OrderAny {
    make_conditional_order_for(
        eth_perp_id(),
        OrderType::StopMarket,
        id,
        side,
        qty,
        trigger,
        tif,
    )
}

fn cache_order(cache: &Rc<RefCell<Cache>>, order: OrderAny) {
    cache
        .borrow_mut()
        .add_order(order, None, Some(client_id()), false)
        .expect("add order to cache");
}

fn cache_accepted_order(
    cache: &Rc<RefCell<Cache>>,
    order: OrderAny,
    venue_order_id: VenueOrderId,
) -> (InstrumentId, ClientOrderId) {
    let instrument_id = order.instrument_id();
    let client_order_id = order.client_order_id();
    cache_order(cache, order);

    let accepted = OrderEventAny::Accepted(OrderAccepted::new(
        trader_id(),
        strategy_id(),
        instrument_id,
        client_order_id,
        venue_order_id,
        account_id(),
        UUID4::new(),
        UnixNanos::default(),
        UnixNanos::default(),
        false,
    ));
    cache
        .borrow_mut()
        .update_order(&accepted)
        .expect("apply OrderAccepted");

    (instrument_id, client_order_id)
}

fn cache_pending_cancel_order(
    cache: &Rc<RefCell<Cache>>,
    order: OrderAny,
    venue_order_id: VenueOrderId,
) {
    let (instrument_id, client_order_id) = cache_accepted_order(cache, order, venue_order_id);

    let pending_cancel = OrderEventAny::PendingCancel(OrderPendingCancel::new(
        trader_id(),
        strategy_id(),
        instrument_id,
        client_order_id,
        Some(account_id()),
        UUID4::new(),
        UnixNanos::default(),
        UnixNanos::default(),
        false,
        Some(venue_order_id),
    ));
    cache
        .borrow_mut()
        .update_order(&pending_cancel)
        .expect("apply OrderPendingCancel");
}

fn submit_command(order: &OrderAny) -> SubmitOrder {
    SubmitOrder::from_order(
        order,
        order.trader_id(),
        Some(client_id()),
        None,
        UUID4::new(),
        UnixNanos::default(),
    )
}

fn submit_order_list_command(orders: &[OrderAny], order_list_id: &str) -> SubmitOrderList {
    let order_list = OrderList::new(
        OrderListId::from(order_list_id),
        orders[0].instrument_id(),
        strategy_id(),
        orders.iter().map(|order| order.client_order_id()).collect(),
        UnixNanos::default(),
    );
    let order_inits = orders
        .iter()
        .map(|order| order.init_event().clone())
        .collect();

    SubmitOrderList::new(
        trader_id(),
        Some(client_id()),
        strategy_id(),
        order_list,
        order_inits,
        None,
        None,
        None,
        UUID4::new(),
        UnixNanos::default(),
        None,
    )
}

// The handler renders `tx_info` as a raw JSON string, so the recorded
// outer Value carries either a string (the common case) or the parsed
// object (when serde stored it inline). Both shapes resolve to the inner
// tx-body object the assertion code expects.
fn send_tx_info(send_tx: &Value) -> Value {
    let inner = send_tx
        .get("data")
        .expect("sendTx data field missing")
        .get("tx_info")
        .expect("tx_info missing");
    match inner {
        Value::String(s) => serde_json::from_str(s).expect("tx_info string is invalid json"),
        other => other.clone(),
    }
}

fn send_tx_type(send_tx: &Value) -> u8 {
    send_tx
        .get("data")
        .and_then(|d| d.get("tx_type"))
        .and_then(Value::as_u64)
        .expect("missing tx_type") as u8
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn test_connect_disconnect_lifecycle() {
    let (addr, state) = start_server().await;
    let (mut client, _rx, _cache) = build_client(addr);

    // Drive the strict-await readiness gate manually so the test pins the
    // ordering contract: `connect()` must remain pending until each of the
    // five account streams has delivered a first frame.
    state
        .auto_emit_account_subscribed_frames
        .store(false, Ordering::Relaxed);

    assert!(!client.is_connected());

    let channel_for = |stream: &str| -> String {
        if stream == "user_stats" {
            format!("user_stats:{TEST_ACCOUNT_INDEX}")
        } else {
            format!("account_all_{stream}:{TEST_ACCOUNT_INDEX}")
        }
    };
    let orders_frame =
        account_subscribed_frame(&channel_for("orders")).expect("orders frame template");
    let trades_frame =
        account_subscribed_frame(&channel_for("trades")).expect("trades frame template");
    let positions_frame =
        account_subscribed_frame(&channel_for("positions")).expect("positions frame template");
    let assets_frame =
        account_subscribed_frame(&channel_for("assets")).expect("assets frame template");
    let user_stats_frame =
        account_subscribed_frame(&channel_for("user_stats")).expect("user_stats frame template");

    {
        let mut connect_fut = std::pin::pin!(client.connect());
        tokio::select! {
            result = &mut connect_fut => {
                panic!("connect returned before account subscriptions were sent: {result:?}");
            }
            () = await_subscribe_count(&state, 5) => {}
        }

        for frame in [
            orders_frame.clone(),
            trades_frame.clone(),
            positions_frame.clone(),
            assets_frame.clone(),
        ] {
            state.push_frame(&frame);
        }
        assert!(
            tokio::time::timeout(Duration::from_millis(250), &mut connect_fut)
                .await
                .is_err(),
            "connect returned with fewer than five account frames",
        );

        // Push the fifth frame; connect must now return promptly.
        state.push_frame(&user_stats_frame);
        tokio::time::timeout(Duration::from_secs(2), &mut connect_fut)
            .await
            .expect("connect did not return after the fifth account frame")
            .expect("connect");
    }

    assert!(client.is_connected());

    wait_until_async(
        || {
            let state = Arc::clone(&state);
            async move { *state.connection_count.lock().await == 1 }
        },
        Duration::from_secs(2),
    )
    .await;

    let subs = state.subscribes().await;
    assert!(
        subs.len() >= 5,
        "expected at least 5 account subscribes, was {}",
        subs.len(),
    );
    let channels: Vec<&str> = subs
        .iter()
        .map(|s| s["channel"].as_str().unwrap_or(""))
        .collect();
    assert!(channels.iter().any(|c| c == &"account_all_orders/12345"));
    assert!(channels.iter().any(|c| c == &"account_all_trades/12345"));
    assert!(channels.iter().any(|c| c == &"account_all_positions/12345"));
    assert!(channels.iter().any(|c| c == &"account_all_assets/12345"));
    assert!(channels.iter().any(|c| c == &"user_stats/12345"));

    // Subscribe frames must carry the L2 auth token; the data-client tests
    // pin the token shape via the REST `auth=` parameter, here the same
    // contract reaches the venue via the WS `auth` field.
    for sub in &subs {
        let auth = sub["auth"].as_str().expect("auth on account subscribe");
        assert_eq!(
            auth.split(':').count(),
            4,
            "unexpected auth shape on {sub:?}",
        );
    }

    client.disconnect().await.expect("disconnect");
    assert!(!client.is_connected());

    wait_until_async(
        || {
            let state = Arc::clone(&state);
            async move { *state.connection_count.lock().await == 0 }
        },
        Duration::from_secs(2),
    )
    .await;
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn stop_disconnects_tasks_and_allows_reconnect() {
    let (addr, state) = start_server().await;
    let (mut client, _rx, _cache) = build_client(addr);

    client.start().expect("start");
    client.connect().await.expect("connect");
    await_connection_count(&state, 1).await;

    client.stop().expect("stop");
    client.stop().expect("repeated stop");
    await_connection_count(&state, 0).await;

    client.start().expect("restart");
    client.connect().await.expect("reconnect");
    await_connection_count(&state, 1).await;

    client.disconnect().await.expect("disconnect");
    client.disconnect().await.expect("repeated disconnect");
    await_connection_count(&state, 0).await;
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn reset_disconnects_tasks_and_allows_reconnect() {
    let (addr, state) = start_server().await;
    let (mut client, _rx, _cache) = build_client(addr);

    client.start().expect("start");
    client.connect().await.expect("connect");
    await_connection_count(&state, 1).await;

    client.reset().expect("reset");
    client.reset().expect("repeated reset");
    await_connection_count(&state, 0).await;

    client.connect().await.expect("reconnect");
    await_connection_count(&state, 1).await;

    client.disconnect().await.expect("disconnect");
    await_connection_count(&state, 0).await;
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn dispose_disconnects_tasks() {
    let (addr, state) = start_server().await;
    let (mut client, _rx, _cache) = build_client(addr);

    client.start().expect("start");
    client.connect().await.expect("connect");
    await_connection_count(&state, 1).await;

    client.dispose().expect("dispose");
    client.dispose().expect("repeated dispose");
    await_connection_count(&state, 0).await;
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn connect_timeout_disconnects_tasks_and_allows_retry() {
    let (addr, state) = start_server().await;
    let (mut client, _rx, _cache) = build_client(addr);
    state
        .auto_emit_account_subscribed_frames
        .store(false, Ordering::Relaxed);

    let error = client
        .connect()
        .await
        .expect_err("connect without account frames should time out");

    assert!(error.to_string().contains("Lighter account streams"));
    assert!(!client.is_connected());
    await_connection_count(&state, 0).await;

    state
        .auto_emit_account_subscribed_frames
        .store(true, Ordering::Relaxed);
    client.connect().await.expect("retry connect");
    await_connection_count(&state, 1).await;

    client.disconnect().await.expect("disconnect");
    await_connection_count(&state, 0).await;
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn connect_reports_configured_venue_on_socket_state() {
    let (addr, state) = start_server().await;
    let venue = Venue::new("LIGHTER_CUSTOM");
    let account_id = AccountId::new("LIGHTER_CUSTOM-001");
    let mut config = build_config(addr);
    config.venue = Some(venue);
    config.account_id = account_id;
    let (system_sender, mut system_rx) = tokio::sync::mpsc::unbounded_channel();
    replace_system_event_sender(system_sender);
    let (mut client, _rx, _cache) = build_client_with(config);

    client.connect().await.expect("connect");

    let event = tokio::time::timeout(Duration::from_secs(2), system_rx.recv())
        .await
        .expect("timed out waiting for a socket state change")
        .expect("system event channel closed");
    let SystemEvent::SocketState(change) = event;
    let endpoint = Ustr::from("lighter-user-streams");

    assert_eq!(client.client_id(), client_id());
    assert_eq!(client.account_id(), account_id);
    assert_eq!(client.venue(), venue);
    assert_eq!(change.client_id, client_id());
    assert_eq!(change.venue, Some(venue));
    assert_eq!(change.endpoint, endpoint);
    assert_eq!(change.state, SocketState::Connected);

    client.disconnect().await.expect("disconnect");
    wait_until_async(
        || {
            let state = Arc::clone(&state);
            async move { *state.connection_count.lock().await == 0 }
        },
        Duration::from_secs(2),
    )
    .await;
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn generate_mass_status_uses_configured_venue() {
    let (addr, _state) = start_server().await;
    let venue = Venue::new("LIGHTER_CUSTOM");
    let account_id = AccountId::new("LIGHTER_CUSTOM-001");
    let mut config = build_config(addr);
    config.venue = Some(venue);
    config.account_id = account_id;
    let (mut client, _rx, _cache) = build_client_with(config);

    client.connect().await.expect("connect");
    let mass_status = client
        .generate_mass_status(None)
        .await
        .expect("mass status")
        .expect("mass status should be available");

    assert_eq!(mass_status.client_id, client_id());
    assert_eq!(mass_status.account_id, account_id);
    assert_eq!(mass_status.venue, venue);

    client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn connect_reports_socket_state_on_the_user_streams_endpoint() {
    let (addr, _state) = start_server().await;
    let (system_sender, mut system_rx) = tokio::sync::mpsc::unbounded_channel();
    replace_system_event_sender(system_sender);
    let registry = SocketReconnectRegistry::default();
    let (mut client, _rx, _cache) = registry.scope(|| build_client(addr));

    client.connect().await.expect("connect");

    let event = tokio::time::timeout(Duration::from_secs(2), system_rx.recv())
        .await
        .expect("timed out waiting for a socket state change")
        .expect("system event channel closed");
    let SystemEvent::SocketState(change) = event;
    let endpoint = Ustr::from("lighter-user-streams");
    let handle = registry.handle(client_id(), endpoint).unwrap();

    assert_eq!(change.client_id, client_id());
    assert_eq!(change.venue, Some(*LIGHTER_VENUE));
    assert_eq!(change.endpoint, endpoint);
    assert_eq!(change.state, SocketState::Connected);
    assert_eq!(
        handle.request_reconnect(),
        SocketReconnectRequestOutcome::Accepted
    );

    let event = tokio::time::timeout(Duration::from_secs(2), system_rx.recv())
        .await
        .expect("timed out waiting for a socket state change")
        .expect("system event channel closed");
    let SystemEvent::SocketState(change) = event;

    assert_eq!(change.client_id, client_id());
    assert_eq!(change.venue, Some(*LIGHTER_VENUE));
    assert_eq!(change.endpoint, endpoint);
    assert_eq!(change.state, SocketState::Disconnected);

    client.disconnect().await.expect("disconnect");
    assert!(registry.handle(client_id(), endpoint).is_none());
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn connect_premium_account_submits_l2_only_integrator_auto_approval() {
    let (addr, state) = start_server().await;
    state.account_type.store(1, Ordering::Relaxed);
    let (mut client, _rx, _cache) = build_client_mainnet(addr);

    client.connect().await.expect("connect");

    let approvals = state.rest_send_txs().await;
    assert_eq!(approvals.len(), 1);
    assert_eq!(approvals[0]["tx_type"], 45);

    let tx_info = &approvals[0]["tx_info"];
    assert_eq!(tx_info["AccountIndex"], TEST_ACCOUNT_INDEX);
    assert_eq!(tx_info["ApiKeyIndex"], TEST_API_KEY_INDEX);
    assert_eq!(
        tx_info["IntegratorAccountIndex"],
        LIGHTER_NAUTILUS_INTEGRATOR_ACCOUNT_INDEX,
    );
    assert_eq!(tx_info["MaxPerpsTakerFee"], 0);
    assert_eq!(tx_info["MaxPerpsMakerFee"], 0);
    assert_eq!(tx_info["MaxSpotTakerFee"], 0);
    assert_eq!(tx_info["MaxSpotMakerFee"], 0);
    assert_eq!(tx_info["L1Sig"], "");
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time must be after UNIX epoch")
        .as_millis() as i64;
    let approval_expiry = tx_info["ApprovalExpiry"]
        .as_i64()
        .expect("ApprovalExpiry must be an i64");
    assert!(
        (now_ms + INTEGRATOR_APPROVAL_MAX_TTL_MS - 60_000
            ..=now_ms + INTEGRATOR_APPROVAL_MAX_TTL_MS)
            .contains(&approval_expiry),
        "ApprovalExpiry must use the maximum five-year TTL",
    );
    assert_eq!(state.referral_use_calls.load(Ordering::Relaxed), 0);

    client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn connect_standard_account_skips_integrator_auto_approval() {
    let (addr, state) = start_server().await;
    let (mut client, _rx, _cache) = build_client_mainnet(addr);

    client.connect().await.expect("connect");

    assert_eq!(state.maker_only_calls.load(Ordering::Relaxed), 0);
    assert_eq!(state.rest_send_txs().await, Vec::<Value>::new());

    client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn connect_applies_robinhood_referral_on_each_process_start() {
    let (addr, state) = start_server().await;
    let (mut first_client, _rx, _cache) = build_client_robinhood_mainnet(addr);
    let expected_request = std::collections::HashMap::from([
        (
            "l1_address".to_string(),
            "0x0000000000000000000000000000000000000000".to_string(),
        ),
        ("referral_code".to_string(), "NAUTILUS".to_string()),
    ]);

    first_client.connect().await.expect("first connect");

    assert_eq!(state.referral_use_calls.load(Ordering::Relaxed), 1);
    assert_eq!(state.referral_use_authorizations.lock().await.len(), 1);
    assert_eq!(
        state.referral_use_requests().await,
        vec![expected_request.clone()],
    );
    assert_eq!(state.maker_only_calls.load(Ordering::Relaxed), 0);
    assert!(state.rest_send_txs().await.is_empty());

    first_client.disconnect().await.expect("first disconnect");
    drop(first_client);

    let (mut second_client, _rx, _cache) = build_client_robinhood_mainnet(addr);
    second_client.connect().await.expect("second connect");

    assert_eq!(state.referral_use_calls.load(Ordering::Relaxed), 2);
    assert_eq!(state.referral_use_authorizations.lock().await.len(), 2);
    assert_eq!(
        state.referral_use_requests().await,
        vec![expected_request.clone(), expected_request],
    );
    assert_eq!(state.maker_only_calls.load(Ordering::Relaxed), 0);
    assert!(state.rest_send_txs().await.is_empty());

    second_client.disconnect().await.expect("second disconnect");
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn connect_continues_when_robinhood_referral_fails() {
    let (addr, state) = start_server().await;
    *state.next_referral_use_response.lock().await = Some(json!({
        "code": 20001,
        "message": "referral unavailable",
    }));

    let (mut client, _rx, _cache) = build_client_robinhood_mainnet(addr);

    client.connect().await.expect("connect");

    assert_eq!(state.referral_use_calls.load(Ordering::Relaxed), 1);
    assert!(client.is_connected());

    client.disconnect().await.expect("disconnect");
}

#[rstest]
#[case::lighter(LighterDeployment::Lighter)]
#[case::robinhood(LighterDeployment::Robinhood)]
#[tokio::test(flavor = "multi_thread")]
async fn connect_omits_attribution_on_testnet(#[case] deployment: LighterDeployment) {
    let (addr, state) = start_server().await;
    let mut config = build_config(addr);
    config.deployment = deployment;
    config.venue = Some(*LIGHTER_VENUE);
    let (mut client, _rx, _cache) = build_client_with(config);

    client.connect().await.expect("connect");

    assert_eq!(state.maker_only_calls.load(Ordering::Relaxed), 0);
    assert_eq!(
        state.maker_only_authorizations().await,
        Vec::<String>::new()
    );
    assert_eq!(state.rest_send_txs().await, Vec::<Value>::new());
    assert_eq!(state.referral_use_calls.load(Ordering::Relaxed), 0);

    client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn connect_skips_integrator_auto_approval_for_maker_only_api_key() {
    let (addr, state) = start_server().await;
    state.account_type.store(1, Ordering::Relaxed);
    state
        .maker_only_api_key_indexes
        .lock()
        .await
        .push(i64::from(TEST_API_KEY_INDEX));
    let (mut client, _rx, _cache) = build_client_mainnet(addr);

    client.connect().await.expect("connect");

    assert_eq!(state.maker_only_calls.load(Ordering::Relaxed), 1);
    assert_eq!(state.maker_only_authorizations().await.len(), 1);
    assert_eq!(state.rest_send_txs().await.len(), 0);

    client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn connect_bails_when_integrator_auto_approval_reports_unapproved() {
    let (addr, state) = start_server().await;
    state.account_type.store(1, Ordering::Relaxed);
    *state.next_rest_send_tx_response.lock().await = Some(json!({
        "code": 21149,
        "message": "integrator is not approved",
    }));
    let (mut client, _rx, _cache) = build_client_mainnet(addr);

    let err = client.connect().await.unwrap_err();
    let msg = format!("{err:#}");

    assert!(
        msg.contains("Lighter account is not integrator-approved (venue 21149)"),
        "unexpected error: {msg}",
    );
    assert!(msg.contains("orders cannot be placed"));
    assert_eq!(state.maker_only_calls.load(Ordering::Relaxed), 1);
    assert_eq!(state.rest_send_txs().await.len(), 1);
    assert!(!client.is_connected());
    assert_eq!(*state.connection_count.lock().await, 0);
}

/// Pins the per-stream marker dispatch in the execution consumption loop.
///
/// Each parametric case drives a different stream as the FIFTH (last) frame.
/// A regression that crosses any of the five `AccountStreamFirstFrame` arms
/// (for example, the `Assets` arm calling `mark_orders()` instead of
/// `mark_assets()`) would leave the named stream unmarked even after its
/// frame lands. The final `connect_fut` await would then time out, failing
/// the specific case whose dispatch is broken.
#[rstest]
#[case::orders_last("orders")]
#[case::trades_last("trades")]
#[case::positions_last("positions")]
#[case::assets_last("assets")]
#[case::user_stats_last("user_stats")]
#[tokio::test(flavor = "multi_thread")]
async fn connect_returns_only_after_each_distinct_stream_marks_its_own_flag(
    #[case] last_stream: &str,
) {
    let (addr, state) = start_server().await;
    let (mut client, _rx, _cache) = build_client(addr);

    state
        .auto_emit_account_subscribed_frames
        .store(false, Ordering::Relaxed);

    // `user_stats` uses a flat channel name; the other four share the
    // `account_all_*` prefix.
    let channel_for = |stream: &str| -> String {
        if stream == "user_stats" {
            format!("user_stats:{TEST_ACCOUNT_INDEX}")
        } else {
            format!("account_all_{stream}:{TEST_ACCOUNT_INDEX}")
        }
    };
    let frame_for =
        |stream: &str| account_subscribed_frame(&channel_for(stream)).expect("frame template");
    let all_streams = ["orders", "trades", "positions", "assets", "user_stats"];
    let frames: std::collections::HashMap<&str, Value> =
        all_streams.iter().map(|s| (*s, frame_for(s))).collect();
    let first_four: Vec<Value> = all_streams
        .iter()
        .filter(|s| **s != last_stream)
        .map(|s| frames[*s].clone())
        .collect();
    let last_frame = frames[last_stream].clone();

    {
        let mut connect_fut = std::pin::pin!(client.connect());
        tokio::select! {
            result = &mut connect_fut => {
                panic!(
                    "connect returned before account subscriptions were sent: {result:?}",
                );
            }
            () = await_subscribe_count(&state, 5) => {}
        }

        for frame in first_four {
            state.push_frame(&frame);
        }
        assert!(
            tokio::time::timeout(Duration::from_millis(250), &mut connect_fut)
                .await
                .is_err(),
            "connect returned before {last_stream} frame landed",
        );

        state.push_frame(&last_frame);
        tokio::time::timeout(Duration::from_secs(2), &mut connect_fut)
            .await
            .unwrap_or_else(|_| {
                panic!(
                    "connect did not return after the {last_stream} frame; \
                     consumption loop likely dispatched mark_* to the wrong flag",
                )
            })
            .expect("connect");
    }

    client.disconnect().await.expect("disconnect");
}

/// Pins the connect-time position-cache clear. Without it a stale
/// prior-session position would survive a disconnect/reconnect cycle and
/// keep surfacing through `generate_position_status_reports` before the
/// venue delivers a replacement snapshot.
#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn connect_clears_prior_position_cache_across_reconnect() {
    let (addr, state) = start_server().await;
    let (mut client, _rx, _cache) = build_client(addr);

    client.connect().await.expect("first connect");
    await_subscribe_count(&state, 4).await;

    // Seed a position so the prior session's cache is non-empty before
    // disconnect.
    state.push_frame(&load_json("ws_account_all_positions_update.json"));
    wait_until_async(
        || {
            let client_ptr = std::ptr::addr_of!(client);
            async move {
                let client = unsafe { &*client_ptr };
                !client
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
                    .unwrap_or_default()
                    .is_empty()
            }
        },
        Duration::from_secs(5),
    )
    .await;

    client.disconnect().await.expect("disconnect");

    // Reconnect. The mock server's auto-emit pushes an empty
    // `account_all_positions` frame. `connect()` still clears the dispatch
    // cache itself so no prior-session position can surface before that
    // frame lands.
    client.connect().await.expect("second connect");

    let positions = client
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
        .expect("position reports");
    assert!(
        positions.is_empty(),
        "prior-session position must not survive a disconnect/reconnect cycle, was {positions:?}",
    );

    client.disconnect().await.expect("final disconnect");
}

mod serial_tests {
    use super::*;

    #[rstest]
    fn test_credentialless_paths_in_isolated_environment() {
        for test_name in [
            "serial_tests::test_connect_without_credentials_fails_fast",
            "serial_tests::test_submit_order_without_credentials_errors_synchronously",
        ] {
            let mut command =
                Command::new(std::env::current_exe().expect("test executable must exist"));
            command.arg(test_name).arg("--exact").arg("--ignored");
            for name in LIGHTER_ENV_VARS {
                command.env_remove(name);
            }
            let output = command.output().expect("isolated test process must run");
            assert!(
                output.status.success(),
                "isolated credentialless case {test_name} failed",
            );
        }
    }

    #[rstest]
    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "runs only in an isolated child process"]
    async fn test_connect_without_credentials_fails_fast() {
        let (addr, state) = start_server().await;
        let (mut client, _rx, _cache) = build_client_with(build_config_no_credentials(addr));

        let err = client.connect().await.unwrap_err();
        assert!(
            err.to_string().contains("requires credentials"),
            "unexpected error: {err}",
        );
        assert!(!client.is_connected());
        // The WS layer must never be dialed when credentials are missing.
        let connections = *state.connection_count.lock().await;
        assert_eq!(connections, 0, "WS must not be opened without credentials");
    }

    #[rstest]
    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "runs only in an isolated child process"]
    async fn test_submit_order_without_credentials_errors_synchronously() {
        let (addr, _state) = start_server().await;
        let (client, cache, _rx) = {
            let (c, rx, ca) = build_client_with(build_config_no_credentials(addr));
            (c, ca, rx)
        };
        let order = make_limit_order(
            "O-NO-CREDS",
            OrderSide::Buy,
            Quantity::from("0.0050"),
            Price::from("2361.31"),
            TimeInForce::Gtc,
            false,
            false,
        );
        cache_order(&cache, order.clone());

        let err = client.submit_order(submit_command(&order)).unwrap_err();
        assert!(
            err.to_string()
                .contains("cannot submit without credentials"),
            "unexpected error: {err}",
        );
    }
}

#[rstest]
#[case::testnet(LighterEnvironment::Testnet, 0, Value::Null)]
#[case::mainnet_standard(LighterEnvironment::Mainnet, 0, Value::Null)]
#[case::mainnet_premium(
    LighterEnvironment::Mainnet,
    1,
    json!({"1": LIGHTER_NAUTILUS_INTEGRATOR_ACCOUNT_INDEX}),
)]
#[tokio::test(flavor = "multi_thread")]
async fn test_submit_limit_order_emits_submitted_and_signs_sendtx(
    #[case] environment: LighterEnvironment,
    #[case] account_type: u8,
    #[case] expected_attributes: Value,
) {
    let (addr, state) = start_server().await;
    state.account_type.store(account_type, Ordering::Relaxed);
    let mut config = build_config(addr);
    config.environment = environment;
    let (mut client, mut rx, cache) = build_client_with(config);
    client.connect().await.expect("connect");

    let order = make_limit_order(
        "O-LIMIT-1",
        OrderSide::Buy,
        Quantity::from("0.0050"),
        Price::from("2361.31"),
        TimeInForce::Gtc,
        false,
        false,
    );
    cache_order(&cache, order.clone());

    client.submit_order(submit_command(&order)).expect("submit");

    // The optimistic OrderSubmitted is emitted synchronously from the
    // dispatch path; it must precede any venue ack on the channel.
    let event = next_order_event(&mut rx, Duration::from_secs(2))
        .await
        .expect("expected OrderSubmitted");

    match event {
        OrderEventAny::Submitted(s) => assert_eq!(s.client_order_id, order.client_order_id()),
        other => panic!("expected OrderSubmitted, was {other:?}"),
    }

    await_send_tx_count(&state, 1).await;
    let frames = state.send_txs().await;
    assert_eq!(frames.len(), 1, "single CreateOrder sendTx expected");
    // CreateOrder = tx_type 14 per Lighter's L2 transaction taxonomy.
    assert_eq!(send_tx_type(&frames[0]), 14);

    let info = send_tx_info(&frames[0]);
    assert_eq!(
        info["MarketIndex"], TEST_MARKET_INDEX,
        "tx_info.MarketIndex must point at the registered market",
    );
    assert_eq!(info["IsAsk"], 0); // buys serialize as 0
    assert_eq!(info["Price"], 236_131); // 2361.31 * 100
    assert_eq!(info["BaseAmount"], 50); // 0.0050 * 10_000
    assert_eq!(info["L2TxAttributes"], expected_attributes);

    client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn test_submit_order_list_fans_out_correlated_create_orders() {
    let (addr, state) = start_server().await;
    let (mut client, mut rx, cache) = build_client(addr);
    client.connect().await.expect("connect");

    let order_a = make_limit_order(
        "O-LIST-A",
        OrderSide::Buy,
        Quantity::from("0.0050"),
        Price::from("2361.31"),
        TimeInForce::Gtc,
        false,
        false,
    );
    let order_b = make_limit_order(
        "O-LIST-B",
        OrderSide::Sell,
        Quantity::from("0.0100"),
        Price::from("2400.00"),
        TimeInForce::Gtc,
        true,
        false,
    );
    cache_order(&cache, order_a.clone());
    cache_order(&cache, order_b.clone());

    let command = submit_order_list_command(&[order_a.clone(), order_b.clone()], "OL-NATIVE");
    client.submit_order_list(command).expect("submit list");

    let submitted_a = next_order_event(&mut rx, Duration::from_secs(2))
        .await
        .expect("OrderSubmitted A");
    let submitted_b = next_order_event(&mut rx, Duration::from_secs(2))
        .await
        .expect("OrderSubmitted B");
    let submitted_ids = [submitted_a, submitted_b].map(|event| match event {
        OrderEventAny::Submitted(e) => e.client_order_id,
        other => panic!("expected Submitted, was {other:?}"),
    });
    assert!(submitted_ids.contains(&order_a.client_order_id()));
    assert!(submitted_ids.contains(&order_b.client_order_id()));

    await_send_tx_count(&state, 2).await;
    let frames = state.send_txs().await;
    assert_eq!(frames.len(), 2, "one sendTx frame per list child expected");
    assert!(frames.iter().all(|frame| frame["type"] == "jsonapi/sendtx"));
    assert!(frames.iter().all(|frame| send_tx_type(frame) == 14));

    let infos = frames.iter().map(send_tx_info).collect::<Vec<_>>();
    assert_eq!(infos.len(), 2);
    assert_eq!(infos[0]["MarketIndex"], TEST_MARKET_INDEX);
    assert_eq!(infos[0]["IsAsk"], 0);
    assert_eq!(infos[1]["MarketIndex"], TEST_MARKET_INDEX);
    assert_eq!(infos[1]["IsAsk"], 1);
    assert_eq!(infos[1]["TimeInForce"], 2);
    let first_nonce = infos[0]["Nonce"].as_i64().expect("first nonce");
    assert_eq!(infos[1]["Nonce"].as_i64(), Some(first_nonce + 1));
    assert!(
        next_order_event(&mut rx, Duration::from_millis(100))
            .await
            .is_none(),
        "sendTx handoff is not a per-order terminal outcome",
    );

    client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn test_order_list_fanout_precedes_later_single_sendtx() {
    let (addr, state) = start_server().await;
    let mut config = build_config(addr);
    config.sendtx_quota_per_min = Some(24_000);
    let (mut client, mut rx, cache) = build_client_with(config);
    client.connect().await.expect("connect");

    let batch_a = make_limit_order(
        "O-SEQ-A",
        OrderSide::Buy,
        Quantity::from("0.0050"),
        Price::from("2361.31"),
        TimeInForce::Gtc,
        false,
        false,
    );
    let batch_b = make_limit_order(
        "O-SEQ-B",
        OrderSide::Sell,
        Quantity::from("0.0100"),
        Price::from("2400.00"),
        TimeInForce::Gtc,
        true,
        false,
    );
    let single = make_limit_order(
        "O-SEQ-C",
        OrderSide::Buy,
        Quantity::from("0.0050"),
        Price::from("2350.00"),
        TimeInForce::Gtc,
        false,
        false,
    );
    cache_order(&cache, batch_a.clone());
    cache_order(&cache, batch_b.clone());
    cache_order(&cache, single.clone());

    let command = submit_order_list_command(&[batch_a.clone(), batch_b.clone()], "OL-SEQ");
    client.submit_order_list(command).expect("submit list");

    let submitted_a = next_order_event(&mut rx, Duration::from_secs(2))
        .await
        .expect("OrderSubmitted A");
    let submitted_b = next_order_event(&mut rx, Duration::from_secs(2))
        .await
        .expect("OrderSubmitted B");
    let submitted_ids = [submitted_a, submitted_b].map(|event| match event {
        OrderEventAny::Submitted(e) => e.client_order_id,
        other => panic!("expected Submitted, was {other:?}"),
    });
    assert!(submitted_ids.contains(&batch_a.client_order_id()));
    assert!(submitted_ids.contains(&batch_b.client_order_id()));

    await_send_tx_count(&state, 2).await;

    client
        .submit_order(submit_command(&single))
        .expect("submit");
    let submitted_single = next_order_event(&mut rx, Duration::from_secs(2))
        .await
        .expect("OrderSubmitted single");
    match submitted_single {
        OrderEventAny::Submitted(e) => assert_eq!(e.client_order_id, single.client_order_id()),
        other => panic!("expected Submitted, was {other:?}"),
    }

    await_send_tx_count(&state, 3).await;

    let frames = state.send_txs().await;
    assert_eq!(frames.len(), 3);
    assert!(frames.iter().all(|frame| frame["type"] == "jsonapi/sendtx"));
    assert!(frames.iter().all(|frame| send_tx_type(frame) == 14));
    let infos = frames.iter().map(send_tx_info).collect::<Vec<_>>();
    assert_eq!(infos[0]["IsAsk"], 0);
    assert_eq!(infos[1]["IsAsk"], 1);
    assert_eq!(infos[2]["Price"], 235_000);
    let first_nonce = infos[0]["Nonce"].as_i64().expect("first nonce");
    assert_eq!(infos[1]["Nonce"].as_i64(), Some(first_nonce + 1));
    assert_eq!(infos[2]["Nonce"].as_i64(), Some(first_nonce + 2));

    client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn test_submit_post_only_order_carries_post_only_tif() {
    let (addr, state) = start_server().await;
    let (mut client, mut rx, cache) = build_client(addr);
    client.connect().await.expect("connect");

    let order = make_limit_order(
        "O-POST-ONLY",
        OrderSide::Sell,
        Quantity::from("0.0100"),
        Price::from("2400.00"),
        TimeInForce::Gtc,
        true,
        false,
    );
    cache_order(&cache, order.clone());
    client.submit_order(submit_command(&order)).expect("submit");

    let _submitted = next_order_event(&mut rx, Duration::from_secs(2)).await;
    await_send_tx_count(&state, 1).await;
    let frames = state.send_txs().await;
    let info = send_tx_info(&frames[0]);
    // Lighter's TIF taxonomy: post-only carries a dedicated tif byte
    // (`LighterOrderTimeInForce::PostOnly = 2`).
    assert_eq!(info["TimeInForce"], 2);
    assert_eq!(info["IsAsk"], 1); // sells serialize as 1

    client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn test_submit_reduce_only_flag_propagates_to_sendtx() {
    let (addr, state) = start_server().await;
    let (mut client, mut rx, cache) = build_client(addr);
    client.connect().await.expect("connect");

    let order = make_limit_order(
        "O-REDUCE-ONLY",
        OrderSide::Sell,
        Quantity::from("0.0050"),
        Price::from("2361.31"),
        TimeInForce::Gtc,
        false,
        true,
    );
    cache_order(&cache, order.clone());
    client.submit_order(submit_command(&order)).expect("submit");

    let _submitted = next_order_event(&mut rx, Duration::from_secs(2)).await;
    await_send_tx_count(&state, 1).await;
    let frames = state.send_txs().await;
    let info = send_tx_info(&frames[0]);
    assert_eq!(info["ReduceOnly"], 1);

    client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn test_submit_stop_market_order_uses_ioc_priced_with_slippage() {
    let (addr, state) = start_server().await;
    let (mut client, mut rx, cache) = build_client(addr);
    client.connect().await.expect("connect");

    let trigger = Price::from("2400.00");
    let order = make_stop_market_order(
        "O-STOP-MKT",
        OrderSide::Buy,
        Quantity::from("0.0050"),
        trigger,
    );
    cache_order(&cache, order.clone());
    client.submit_order(submit_command(&order)).expect("submit");

    let _submitted = next_order_event(&mut rx, Duration::from_secs(2)).await;
    await_send_tx_count(&state, 1).await;
    let frames = state.send_txs().await;
    let info = send_tx_info(&frames[0]);
    // The signed payload carries the trigger price plus the slippage-bounded
    // protection price (>= trigger for buys). The exact price is the trigger
    // adjusted by 50 bps (config default), but only the ordering is pinned
    // to keep the test resilient to a config tweak.
    assert_eq!(info["TimeInForce"], 0);
    assert!(
        info["OrderExpiry"].as_i64().unwrap() > 0,
        "conditional market trigger must carry a positive resting expiry",
    );
    let price = info["Price"].as_i64().unwrap();
    let trigger_ticks = info["TriggerPrice"].as_i64().unwrap();
    assert_eq!(trigger_ticks, 240_000);
    assert!(
        price >= trigger_ticks,
        "buy protection price must be >= trigger; price={price} trigger={trigger_ticks}",
    );

    client.disconnect().await.expect("disconnect");
}

#[rstest]
#[case::stop_limit_buy(OrderType::StopLimit, OrderSide::Buy, 3, 1)]
#[case::market_if_touched_buy(OrderType::MarketIfTouched, OrderSide::Buy, 4, 0)]
#[case::limit_if_touched_buy(OrderType::LimitIfTouched, OrderSide::Buy, 5, 1)]
#[case::stop_market_sell(OrderType::StopMarket, OrderSide::Sell, 2, 0)]
#[tokio::test(flavor = "multi_thread")]
async fn test_submit_conditional_order_matrix_signs_expected_wire_shape(
    #[case] order_type: OrderType,
    #[case] side: OrderSide,
    #[case] expected_type: u8,
    #[case] expected_tif: u8,
) {
    let (addr, state) = start_server().await;
    let (mut client, mut rx, cache) = build_client(addr);
    client.connect().await.expect("connect");

    let order = make_conditional_order_for(
        eth_perp_id(),
        order_type,
        &format!("O-CONDITIONAL-{order_type:?}-{side:?}"),
        side,
        Quantity::from("0.0050"),
        Price::from("2400.00"),
        TimeInForce::Gtc,
    );
    cache_order(&cache, order.clone());
    client.submit_order(submit_command(&order)).expect("submit");

    let submitted = next_order_event(&mut rx, Duration::from_secs(2))
        .await
        .expect("OrderSubmitted");
    assert!(matches!(submitted, OrderEventAny::Submitted(_)));
    await_send_tx_count(&state, 1).await;
    let frames = state.send_txs().await;
    let info = send_tx_info(&frames[0]);
    assert_eq!(info["Type"], expected_type);
    assert_eq!(info["TimeInForce"], expected_tif);
    assert_eq!(info["IsAsk"], u8::from(side == OrderSide::Sell));
    assert_eq!(info["TriggerPrice"], 240_000);
    assert!(info["OrderExpiry"].as_i64().unwrap() > 0);

    if matches!(
        order_type,
        OrderType::StopMarket | OrderType::MarketIfTouched
    ) {
        let price = info["Price"].as_i64().unwrap();
        let trigger = info["TriggerPrice"].as_i64().unwrap();
        if side == OrderSide::Buy {
            assert!(price >= trigger);
        } else {
            assert!(price <= trigger);
        }
    } else {
        assert_eq!(info["Price"], 240_100);
    }

    client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn test_submit_limit_ioc_signs_zero_expiry() {
    let (addr, state) = start_server().await;
    let (mut client, mut rx, cache) = build_client(addr);
    client.connect().await.expect("connect");

    let order = make_limit_order(
        "O-LIMIT-IOC",
        OrderSide::Buy,
        Quantity::from("0.0050"),
        Price::from("2361.31"),
        TimeInForce::Ioc,
        false,
        false,
    );
    cache_order(&cache, order.clone());
    client.submit_order(submit_command(&order)).expect("submit");

    assert!(matches!(
        next_order_event(&mut rx, Duration::from_secs(2)).await,
        Some(OrderEventAny::Submitted(_)),
    ));
    await_send_tx_count(&state, 1).await;
    let frames = state.send_txs().await;
    let info = send_tx_info(&frames[0]);
    assert_eq!(info["Type"], 0);
    assert_eq!(info["TimeInForce"], 0);
    assert_eq!(info["OrderExpiry"], 0);

    client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn test_submit_market_order_without_quote_denies_locally() {
    // Market orders require a cached quote to derive the worst-acceptable
    // price; without one, dispatch fails and the order is denied. This
    // guard exists so we never burn a nonce on an unpriced market order.
    let (addr, state) = start_server().await;
    let (mut client, mut rx, cache) = build_client(addr);
    client.connect().await.expect("connect");

    let order = make_market_order(
        "O-MARKET-NO-QUOTE",
        OrderSide::Buy,
        Quantity::from("0.0050"),
    );
    cache_order(&cache, order.clone());

    client
        .submit_order(submit_command(&order))
        .expect("local denial should not return Err to the engine");
    let reason = assert_local_order_denied_once(&mut rx, &state, "no cached quote").await;
    assert!(reason.starts_with("VALIDATION_FAILED:"));

    client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn test_submit_fok_limit_order_denies_once_without_error() {
    let (addr, state) = start_server().await;
    let (client, mut rx, cache) = build_client(addr);

    let order = make_limit_order(
        "O-LIMIT-FOK",
        OrderSide::Buy,
        Quantity::from("0.0050"),
        Price::from("2361.31"),
        TimeInForce::Fok,
        false,
        false,
    );
    cache_order(&cache, order.clone());

    client
        .submit_order(submit_command(&order))
        .expect("local denial should not return Err to the engine");
    assert_local_order_denied_once(&mut rx, &state, "UNSUPPORTED_TIME_IN_FORCE: FOK").await;
}

#[rstest]
#[case::quote_quantity(
    make_limit_order_with_quantity_options("O-LIMIT-QUOTE-QTY", true, None),
    "quote_quantity"
)]
#[case::display_qty(
    make_limit_order_with_quantity_options(
        "O-LIMIT-DISPLAY-QTY",
        false,
        Some(Quantity::from("0.0010")),
    ),
    "display_qty"
)]
#[tokio::test(flavor = "multi_thread")]
async fn test_submit_unsupported_quantity_options_deny_locally(
    #[case] order: OrderAny,
    #[case] reason_part: &str,
) {
    let (addr, state) = start_server().await;
    let (client, mut rx, cache) = build_client(addr);
    cache_order(&cache, order.clone());

    client
        .submit_order(submit_command(&order))
        .expect("local denial should not return Err to the engine");
    assert_local_order_denied_once(&mut rx, &state, reason_part).await;
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn test_submit_conditional_market_ioc_denies_once_without_error() {
    let (addr, state) = start_server().await;
    let (client, mut rx, cache) = build_client(addr);

    let order = make_stop_market_order_with_tif(
        "O-STOP-MARKET-IOC",
        OrderSide::Buy,
        Quantity::from("0.0050"),
        Price::from("2400.00"),
        TimeInForce::Ioc,
    );
    cache_order(&cache, order.clone());

    client
        .submit_order(submit_command(&order))
        .expect("local denial should not return Err to the engine");
    assert_local_order_denied_once(&mut rx, &state, "positive expiry").await;
}

#[rstest]
#[case(OrderType::StopMarket)]
#[case(OrderType::StopLimit)]
#[case(OrderType::MarketIfTouched)]
#[case(OrderType::LimitIfTouched)]
#[tokio::test(flavor = "multi_thread")]
async fn test_submit_spot_conditional_order_denies_locally(#[case] order_type: OrderType) {
    let (addr, state) = start_server().await;
    let (client, mut rx, cache) = build_client(addr);
    cache
        .borrow_mut()
        .add_instrument(test_spot_instrument())
        .expect("add spot instrument");

    let order = make_conditional_order_for(
        eth_spot_id(),
        order_type,
        &format!("O-SPOT-{order_type:?}"),
        OrderSide::Buy,
        Quantity::from("1.00"),
        Price::from("1.2000"),
        TimeInForce::Gtc,
    );
    cache_order(&cache, order.clone());

    client
        .submit_order(submit_command(&order))
        .expect("local denial should not return Err to the engine");
    assert_local_order_denied_once(&mut rx, &state, "spot markets").await;
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn test_submit_order_venue_rejection_emits_order_rejected() {
    // Pins commit 5de009e15c: when the venue replies to a sendTx with a
    // non-200 code, the adapter must emit a typed `OrderRejected` keyed on
    // the head-of-queue pending CreateOrder cloid.
    let (addr, state) = start_server().await;
    let (mut client, mut rx, cache) = build_client(addr);
    client.connect().await.expect("connect");

    // Install a rejection on the next sendTx ack.
    *state.next_send_tx_ack.lock().await = Some(json!({
        "type": "jsonapi/sendtx",
        "code": 21029,
        "message": "insufficient margin",
    }));

    let order = make_limit_order(
        "O-VENUE-REJECT",
        OrderSide::Buy,
        Quantity::from("0.0050"),
        Price::from("2361.31"),
        TimeInForce::Gtc,
        false,
        false,
    );
    cache_order(&cache, order.clone());
    client.submit_order(submit_command(&order)).expect("submit");

    // OrderSubmitted is optimistic; OrderRejected follows from the ack.
    let submitted = next_order_event(&mut rx, Duration::from_secs(2))
        .await
        .expect("expected OrderSubmitted");
    assert!(matches!(submitted, OrderEventAny::Submitted(_)));

    let rejected = next_order_event(&mut rx, Duration::from_secs(2))
        .await
        .expect("expected OrderRejected");

    match rejected {
        OrderEventAny::Rejected(r) => {
            assert_eq!(r.client_order_id, order.client_order_id());
            let reason = r.reason.as_str();
            assert_eq!(reason, "LIGHTER_21029: insufficient margin");
        }
        other => panic!("expected OrderRejected, was {other:?}"),
    }

    client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn test_acknowledged_create_failed_by_sequencer_emits_order_rejected() {
    let (addr, state) = start_server().await;
    let (mut client, mut rx, cache) = build_client(addr);
    client.connect().await.expect("connect");
    state.tx_responses.lock().await.push_back(json!({
        "status": 0,
        "event_info": json!({"ae":"reduce only increases position"}).to_string(),
    }));

    let order = make_limit_order(
        "O-ACK-CREATE-REJECT",
        OrderSide::Sell,
        Quantity::from("0.0050"),
        Price::from("2361.31"),
        TimeInForce::Gtc,
        false,
        true,
    );
    cache_order(&cache, order.clone());
    client.submit_order(submit_command(&order)).expect("submit");
    await_send_tx_count(&state, 1).await;
    let first_info = send_tx_info(&state.send_txs().await[0]);

    assert!(matches!(
        next_order_event(&mut rx, Duration::from_secs(2)).await,
        Some(OrderEventAny::Submitted(_)),
    ));
    let rejected = next_order_event(&mut rx, Duration::from_secs(4))
        .await
        .expect("acknowledged create rejection");

    match rejected {
        OrderEventAny::Rejected(event) => {
            assert_eq!(event.client_order_id, order.client_order_id());
            assert!(
                event.reason.as_str().contains("sequencer rejected"),
                "unexpected rejection reason: {}",
                event.reason,
            );
            assert!(
                event
                    .reason
                    .as_str()
                    .contains("reduce only increases position"),
                "unexpected rejection reason: {}",
                event.reason,
            );
        }
        other => panic!("expected OrderRejected, was {other:?}"),
    }
    assert_eq!(state.active_orders_calls.load(Ordering::Relaxed), 1);
    assert_eq!(state.tx_calls.load(Ordering::Relaxed), 1);

    client
        .submit_order(submit_command(&order))
        .expect("resubmit");
    await_send_tx_count(&state, 2).await;
    let second_info = send_tx_info(&state.send_txs().await[1]);
    assert_eq!(
        second_info["ClientOrderIndex"],
        first_info["ClientOrderIndex"],
    );
    assert_eq!(
        second_info["Nonce"].as_i64(),
        first_info["Nonce"].as_i64().map(|nonce| nonce + 1),
    );

    client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn test_acknowledged_create_allows_delayed_active_order() {
    let (addr, state) = start_server().await;
    let (mut client, mut rx, cache) = build_client(addr);
    client.connect().await.expect("connect");

    let order = make_limit_order(
        "O-ACK-CREATE-DELAYED",
        OrderSide::Buy,
        Quantity::from("0.0050"),
        Price::from("2361.31"),
        TimeInForce::Gtc,
        false,
        false,
    );
    cache_order(&cache, order.clone());
    client.submit_order(submit_command(&order)).expect("submit");
    await_send_tx_count(&state, 1).await;
    let info = send_tx_info(&state.send_txs().await[0]);
    let client_order_index = info["ClientOrderIndex"].as_i64().unwrap();
    let nonce = info["Nonce"].as_i64().unwrap();
    let mut active_order = http_order_fixture(
        "281476929510500",
        &client_order_index.to_string(),
        "open",
        "0.0000",
    );
    active_order["nonce"] = json!(nonce);
    let mut stale_order = active_order.clone();
    stale_order["order_index"] = json!("281476929510499");
    stale_order["nonce"] = json!(nonce - 1);
    state.active_orders_responses.lock().await.extend([
        http_orders_payload(&[stale_order], None),
        http_orders_payload(&[active_order], None),
    ]);
    state.tx_responses.lock().await.push_back(json!({
        "status": 1,
        "event_info": json!({"ae":""}).to_string(),
    }));

    let report = next_event_matching(&mut rx, Duration::from_secs(7), |event| {
        matches!(event, ExecutionEvent::Report(ExecutionReport::Order(_)))
    })
    .await
    .expect("delayed order status report");
    match report {
        ExecutionEvent::Report(ExecutionReport::Order(report)) => {
            assert_eq!(report.client_order_id, Some(order.client_order_id()));
            assert_eq!(report.venue_order_id, VenueOrderId::from("281476929510500"));
            assert_eq!(report.order_status, OrderStatus::Accepted);
        }
        other => panic!("expected OrderStatusReport, was {other:?}"),
    }
    assert_eq!(state.active_orders_calls.load(Ordering::Relaxed), 2);
    assert_eq!(state.tx_calls.load(Ordering::Relaxed), 1);
    assert!(
        !matches!(
            next_order_event(&mut rx, Duration::from_millis(500)).await,
            Some(OrderEventAny::Rejected(_)),
        ),
        "delayed valid create must not be rejected",
    );

    client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn test_account_order_event_wins_create_probe_race() {
    let (addr, state) = start_server().await;
    let (mut client, mut rx, cache) = build_client(addr);
    client.connect().await.expect("connect");
    state.tx_response_blocked.store(true, Ordering::Release);
    state.tx_responses.lock().await.push_back(json!({
        "status": 0,
        "event_info": json!({"ae":"reduce only increases position"}).to_string(),
    }));

    let order = make_limit_order(
        "O-ACK-CREATE-RACE",
        OrderSide::Buy,
        Quantity::from("0.0050"),
        Price::from("2361.31"),
        TimeInForce::Gtc,
        false,
        false,
    );
    cache_order(&cache, order.clone());
    client.submit_order(submit_command(&order)).expect("submit");
    await_send_tx_count(&state, 1).await;
    let info = send_tx_info(&state.send_txs().await[0]);
    let client_order_index = info["ClientOrderIndex"].as_i64().unwrap();
    let nonce = info["Nonce"].as_i64().unwrap();
    assert!(matches!(
        next_order_event(&mut rx, Duration::from_secs(2)).await,
        Some(OrderEventAny::Submitted(_)),
    ));

    await_tx_calls(&state, 1).await;
    state.push_frame(&json!({
        "type": "update/account_all_orders",
        "channel": format!("account_all_orders:{TEST_ACCOUNT_INDEX}"),
        "orders": {
            "0": [account_all_orders_open_entry(
                client_order_index,
                "281476929510501",
                &client_order_index.to_string(),
                nonce,
            )]
        }
    }));
    assert!(matches!(
        next_order_event(&mut rx, Duration::from_secs(2)).await,
        Some(OrderEventAny::Accepted(_)),
    ));
    state.tx_response_release.notify_one();
    assert!(
        !matches!(
            next_order_event(&mut rx, Duration::from_millis(2500)).await,
            Some(OrderEventAny::Rejected(_)),
        ),
        "account order event must prevent a probe rejection",
    );
    assert_eq!(state.active_orders_calls.load(Ordering::Relaxed), 1);
    assert_eq!(state.tx_calls.load(Ordering::Relaxed), 1);

    client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn test_reconnect_during_create_probe_preserves_identity() {
    let (addr, state) = start_server().await;
    let (mut client, mut rx, cache) = build_client(addr);
    client.connect().await.expect("connect");
    state.tx_response_blocked.store(true, Ordering::Release);
    state.tx_responses.lock().await.push_back(json!({
        "status": 0,
        "event_info": json!({"ae":"reduce only increases position"}).to_string(),
    }));

    let order = make_limit_order(
        "O-ACK-CREATE-RECONNECT",
        OrderSide::Buy,
        Quantity::from("0.0050"),
        Price::from("2361.31"),
        TimeInForce::Gtc,
        false,
        false,
    );
    cache_order(&cache, order.clone());
    client.submit_order(submit_command(&order)).expect("submit");
    await_send_tx_count(&state, 1).await;
    let info = send_tx_info(&state.send_txs().await[0]);
    let client_order_index = info["ClientOrderIndex"].as_i64().unwrap();
    let nonce = info["Nonce"].as_i64().unwrap();
    assert!(matches!(
        next_order_event(&mut rx, Duration::from_secs(2)).await,
        Some(OrderEventAny::Submitted(_)),
    ));
    await_tx_calls(&state, 1).await;

    let subscribe_count = state.subscribes().await.len();
    state.close_after_next_frame.store(true, Ordering::Release);
    let tickle = make_limit_order(
        "O-ACK-CREATE-RECONNECT-TICKLE",
        OrderSide::Buy,
        Quantity::from("0.0050"),
        Price::from("2361.31"),
        TimeInForce::Gtc,
        false,
        false,
    );
    let tickle_id = tickle.client_order_id();
    cache_order(&cache, tickle);
    client
        .cancel_order(CancelOrder::new(
            trader_id(),
            Some(client_id()),
            strategy_id(),
            eth_perp_id(),
            tickle_id,
            Some(VenueOrderId::from("1")),
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ))
        .expect("reconnect tickle");
    await_subscribe_count(&state, subscribe_count + 4).await;
    state.tx_response_release.notify_one();

    assert!(
        !matches!(
            next_order_event(&mut rx, Duration::from_millis(500)).await,
            Some(OrderEventAny::Rejected(_)),
        ),
        "a stale-epoch transaction response must not reject the create",
    );
    state.push_frame(&json!({
        "type": "update/account_all_orders",
        "channel": format!("account_all_orders:{TEST_ACCOUNT_INDEX}"),
        "orders": {
            "0": [account_all_orders_open_entry(
                client_order_index,
                "281476929510503",
                &client_order_index.to_string(),
                nonce,
            )]
        }
    }));
    assert!(matches!(
        next_order_event(&mut rx, Duration::from_secs(2)).await,
        Some(OrderEventAny::Accepted(_)),
    ));

    client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn test_acknowledged_create_pending_final_remains_reconcilable() {
    let (addr, state) = start_server().await;
    let (mut client, mut rx, cache) = build_client(addr);
    client.connect().await.expect("connect");
    state.tx_responses.lock().await.extend((0..3).map(|_| {
        json!({
            "status": 3,
            "event_info": json!({"ae":""}).to_string(),
        })
    }));

    let order = make_limit_order(
        "O-ACK-CREATE-PENDING-FINAL",
        OrderSide::Buy,
        Quantity::from("0.0050"),
        Price::from("2361.31"),
        TimeInForce::Gtc,
        false,
        false,
    );
    cache_order(&cache, order.clone());
    client.submit_order(submit_command(&order)).expect("submit");
    await_send_tx_count(&state, 1).await;
    let info = send_tx_info(&state.send_txs().await[0]);
    let client_order_index = info["ClientOrderIndex"].as_i64().unwrap();
    let nonce = info["Nonce"].as_i64().unwrap();
    assert!(matches!(
        next_order_event(&mut rx, Duration::from_secs(2)).await,
        Some(OrderEventAny::Submitted(_)),
    ));

    await_tx_calls(&state, 3).await;
    assert_eq!(state.active_orders_calls.load(Ordering::Relaxed), 3);
    assert!(
        !matches!(
            next_order_event(&mut rx, Duration::from_millis(200)).await,
            Some(OrderEventAny::Rejected(_)),
        ),
        "pending-final transaction status must remain unresolved",
    );

    state.push_frame(&json!({
        "type": "update/account_all_orders",
        "channel": format!("account_all_orders:{TEST_ACCOUNT_INDEX}"),
        "orders": {
            "0": [account_all_orders_open_entry(
                client_order_index,
                "281476929510502",
                &client_order_index.to_string(),
                nonce,
            )]
        }
    }));
    assert!(matches!(
        next_order_event(&mut rx, Duration::from_secs(2)).await,
        Some(OrderEventAny::Accepted(_)),
    ));

    client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn test_submit_order_subscription_error_does_not_reject() {
    // A bare subscription error (30003 "Already Subscribed", typical of
    // reconnect replay) arriving while a create is pending is outside the
    // venue's transaction code range: it must not pop the pending queue and
    // must not emit OrderRejected for the live order.
    let (addr, state) = start_server().await;
    let (mut client, mut rx, cache) = build_client(addr);
    client.connect().await.expect("connect");

    // Respond to the sendTx with a wrapped bare error instead of an ack so
    // the create entry is still pending when the frame is classified.
    *state.next_send_tx_ack.lock().await = Some(json!({
        "error": {"code": 30003, "message": "Already Subscribed to : ticker:3"},
    }));

    let order = make_limit_order(
        "O-SUB-ERROR",
        OrderSide::Buy,
        Quantity::from("0.0050"),
        Price::from("2361.31"),
        TimeInForce::Gtc,
        false,
        false,
    );
    cache_order(&cache, order.clone());
    client.submit_order(submit_command(&order)).expect("submit");

    let submitted = next_order_event(&mut rx, Duration::from_secs(2))
        .await
        .expect("expected OrderSubmitted");
    assert!(matches!(submitted, OrderEventAny::Submitted(_)));

    let follow_up = next_order_event(&mut rx, Duration::from_secs(1)).await;
    assert!(
        follow_up.is_none(),
        "subscription error must not reject the pending order, was {follow_up:?}",
    );

    client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn test_cancel_order_signs_cancel_sendtx() {
    let (addr, state) = start_server().await;
    let (mut client, _rx, cache) = build_client(addr);
    client.connect().await.expect("connect");

    let order = make_limit_order(
        "O-CANCEL-1",
        OrderSide::Buy,
        Quantity::from("0.0050"),
        Price::from("2361.31"),
        TimeInForce::Gtc,
        false,
        false,
    );
    let client_order_id = order.client_order_id();
    cache_order(&cache, order);

    let voi = VenueOrderId::from("281476929510110");
    let cmd = CancelOrder::new(
        trader_id(),
        Some(client_id()),
        strategy_id(),
        eth_perp_id(),
        client_order_id,
        Some(voi),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );

    client.cancel_order(cmd).expect("cancel_order");
    await_send_tx_count(&state, 1).await;

    let frames = state.send_txs().await;
    assert_eq!(send_tx_type(&frames[0]), 15); // CancelOrder
    let info = send_tx_info(&frames[0]);
    assert_eq!(info["MarketIndex"], TEST_MARKET_INDEX);
    assert_eq!(info["Index"], 281_476_929_510_110_i64);

    client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn test_cancel_order_venue_rejection_emits_cancel_rejected_for_pending_cancel_order() {
    let (addr, state) = start_server().await;
    let (mut client, mut rx, cache) = build_client(addr);
    client.connect().await.expect("connect");

    let order = make_limit_order(
        "O-CANCEL-VENUE-REJECT",
        OrderSide::Buy,
        Quantity::from("0.0050"),
        Price::from("2361.31"),
        TimeInForce::Gtc,
        false,
        false,
    );
    let client_order_id = order.client_order_id();
    let venue_order_id = VenueOrderId::from("281476929510112");
    cache_pending_cancel_order(&cache, order, venue_order_id);

    *state.next_send_tx_ack.lock().await = Some(json!({
        "type": "jsonapi/sendtx",
        "code": 21727,
        "message": "order is not cancelable",
    }));

    let baseline = state.send_txs().await.len();
    let cmd = CancelOrder::new(
        trader_id(),
        Some(client_id()),
        strategy_id(),
        eth_perp_id(),
        client_order_id,
        Some(venue_order_id),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );
    client.cancel_order(cmd).expect("cancel_order");
    await_send_tx_count(&state, baseline + 1).await;

    let frames = state.send_txs().await;
    assert_eq!(send_tx_type(&frames[baseline]), 15);
    assert_eq!(
        send_tx_info(&frames[baseline])["Index"],
        281_476_929_510_112_i64,
    );

    let rejected = next_order_event(&mut rx, Duration::from_secs(2))
        .await
        .expect("expected OrderCancelRejected");

    match rejected {
        OrderEventAny::CancelRejected(e) => {
            assert_eq!(e.client_order_id, client_order_id);
            assert_eq!(e.instrument_id, eth_perp_id());
            assert_eq!(e.venue_order_id, Some(venue_order_id));
            let reason = e.reason.as_str();
            assert_eq!(reason, "LIGHTER_21727: order is not cancelable");
        }
        other => panic!("expected OrderCancelRejected, was {other:?}"),
    }

    assert!(
        next_order_event(&mut rx, Duration::from_millis(100))
            .await
            .is_none(),
        "cancel venue rejection must emit exactly one order event",
    );

    client.disconnect().await.expect("disconnect");
}

#[rstest]
#[case::testnet(LighterEnvironment::Testnet, 0, Value::Null)]
#[case::mainnet_standard(LighterEnvironment::Mainnet, 0, Value::Null)]
#[case::mainnet_premium(
    LighterEnvironment::Mainnet,
    1,
    json!({"1": LIGHTER_NAUTILUS_INTEGRATOR_ACCOUNT_INDEX}),
)]
#[tokio::test(flavor = "multi_thread")]
async fn test_modify_order_signs_modify_sendtx(
    #[case] environment: LighterEnvironment,
    #[case] account_type: u8,
    #[case] expected_attributes: Value,
) {
    let (addr, state) = start_server().await;
    state.account_type.store(account_type, Ordering::Relaxed);
    let mut config = build_config(addr);
    config.environment = environment;
    let (mut client, _rx, cache) = build_client_with(config);
    client.connect().await.expect("connect");

    let order = make_limit_order(
        "O-MODIFY-1",
        OrderSide::Buy,
        Quantity::from("0.0050"),
        Price::from("2361.31"),
        TimeInForce::Gtc,
        false,
        false,
    );
    cache_order(&cache, order.clone());

    let voi = VenueOrderId::from("281476929510111");
    let cmd = ModifyOrder::new(
        trader_id(),
        Some(client_id()),
        strategy_id(),
        eth_perp_id(),
        order.client_order_id(),
        Some(voi),
        Some(Quantity::from("0.0100")),
        Some(Price::from("2400.00")),
        None,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );
    client.modify_order(cmd).expect("modify_order");
    await_send_tx_count(&state, 1).await;

    let frames = state.send_txs().await;
    // LighterTxType::ModifyOrder discriminant is 17 (CancelAllOrders takes 16).
    assert_eq!(send_tx_type(&frames[0]), 17);
    let info = send_tx_info(&frames[0]);
    assert_eq!(info["MarketIndex"], TEST_MARKET_INDEX);
    assert_eq!(info["Index"], 281_476_929_510_111_i64);
    assert_eq!(info["BaseAmount"], 100);
    assert_eq!(info["Price"], 240_000);
    assert_eq!(info["L2TxAttributes"], expected_attributes);

    client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn test_modify_order_venue_rejection_emits_modify_rejected() {
    let (addr, state) = start_server().await;
    let (mut client, mut rx, cache) = build_client(addr);
    client.connect().await.expect("connect");

    let (_client_order_index, venue_order_id) = seed_open_order(
        &client,
        &cache,
        &state,
        &mut rx,
        "O-MODIFY-VENUE-REJECT",
        "281476929510113",
    )
    .await;
    let client_order_id = ClientOrderId::from("O-MODIFY-VENUE-REJECT");

    *state.next_send_tx_ack.lock().await = Some(json!({
        "type": "jsonapi/sendtx",
        "code": 21702,
        "message": "modify rejected by venue",
    }));

    let baseline = state.send_txs().await.len();
    let cmd = ModifyOrder::new(
        trader_id(),
        Some(client_id()),
        strategy_id(),
        eth_perp_id(),
        client_order_id,
        Some(venue_order_id),
        Some(Quantity::from("0.0100")),
        Some(Price::from("2400.00")),
        None,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );
    client.modify_order(cmd).expect("modify_order");
    await_send_tx_count(&state, baseline + 1).await;

    let frames = state.send_txs().await;
    assert_eq!(send_tx_type(&frames[baseline]), 17);
    assert_eq!(
        send_tx_info(&frames[baseline])["Index"],
        281_476_929_510_113_i64,
    );

    let rejected = next_order_event(&mut rx, Duration::from_secs(2))
        .await
        .expect("expected OrderModifyRejected");

    match rejected {
        OrderEventAny::ModifyRejected(e) => {
            assert_eq!(e.client_order_id, client_order_id);
            assert_eq!(e.instrument_id, eth_perp_id());
            assert_eq!(e.venue_order_id, Some(venue_order_id));
            let reason = e.reason.as_str();
            assert_eq!(reason, "LIGHTER_21702: modify rejected by venue");
        }
        other => panic!("expected OrderModifyRejected, was {other:?}"),
    }

    assert!(
        next_order_event(&mut rx, Duration::from_millis(100))
            .await
            .is_none(),
        "modify venue rejection must emit exactly one order event",
    );

    client.disconnect().await.expect("disconnect");
}

/// Drives an order through submit → venue echo so it ends up `Accepted`
/// in the cache and present in `dispatch.venue_id_map`. Returns the
/// `(client_order_index, venue_order_id)` chosen for the seeded order.
///
/// `cancel_all_orders` consults both pieces of state: the cache for the
/// open-orders iteration, the dispatch state for `lookup_venue_order_id`.
/// Tests that exercise the full open-iteration path go through this
/// `seed_open_order` so the two stay in sync.
async fn seed_open_order(
    client: &LighterExecutionClient,
    cache: &Rc<RefCell<Cache>>,
    state: &TestServerState,
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
    id: &str,
    voi_str: &str,
) -> (i64, VenueOrderId) {
    let order = make_limit_order(
        id,
        OrderSide::Buy,
        Quantity::from("0.0050"),
        Price::from("2361.31"),
        TimeInForce::Gtc,
        false,
        false,
    );
    cache_order(cache, order.clone());
    let cloid = order.client_order_id();

    let baseline = state.send_txs().await.len();
    client.submit_order(submit_command(&order)).expect("submit");
    await_send_tx_count(state, baseline + 1).await;

    let frames = state.send_txs().await;
    let info = send_tx_info(&frames[baseline]);
    let client_order_index = info["ClientOrderIndex"]
        .as_i64()
        .expect("ClientOrderIndex in tx_info");
    let submission_nonce = info["Nonce"].as_i64().expect("Nonce in tx_info");
    assert_ne!(submission_nonce, TEST_ORDER_NONCE);

    // The optimistic OrderSubmitted is emitted synchronously by submit_order
    // and applied to the cache so the state matches what the engine would
    // see after dispatching the corresponding event.
    let submitted_event = next_order_event(rx, Duration::from_secs(2))
        .await
        .expect("OrderSubmitted");
    assert!(matches!(submitted_event, OrderEventAny::Submitted(_)));
    cache
        .borrow_mut()
        .update_order(&submitted_event)
        .expect("apply OrderSubmitted");

    // The venue echo lands as a tracked-path frame because submit_order
    // registered the identity. The dispatcher resolves the echo's
    // `client_order_id` field through `cloid_map[i64]`, so the wire
    // value must be the numeric client_order_index (as a string) the
    // adapter signed in the sendTx - not the Nautilus cloid label.
    // Routing the test through the numeric form pins the cloid-map
    // path; a regression there would surface as a missing OrderAccepted.
    let _ = cloid; // retained for readability; assertion uses client_order_index
    let voi = VenueOrderId::from(voi_str);
    state.push_frame(&json!({
        "type": "update/account_all_orders",
        "channel": format!("account_all_orders:{TEST_ACCOUNT_INDEX}"),
        "orders": {
            "0": [account_all_orders_open_entry(
                client_order_index,
                voi.as_str(),
                &client_order_index.to_string(),
                TEST_ORDER_NONCE,
            )]
        }
    }));

    let accepted = next_order_event(rx, Duration::from_secs(2))
        .await
        .expect("OrderAccepted");
    assert!(matches!(accepted, OrderEventAny::Accepted(_)));
    cache
        .borrow_mut()
        .update_order(&accepted)
        .expect("apply OrderAccepted");

    (client_order_index, voi)
}

fn account_all_orders_open_entry(
    client_order_index: i64,
    order_id: &str,
    cloid_label: &str,
    nonce: i64,
) -> Value {
    // Numeric values pinned to the venue's published `account_all_orders`
    // shape (see test_data/ws_account_orders_update.json for the wire
    // form). Only the dispatch-routing fields (client_order_index,
    // order_id, status, market_index, owner_account_index) carry
    // assertion weight here; the rest mirror typical venue defaults so
    // the parser does not reject the frame.
    json!({
        "order_index": client_order_index,
        "client_order_index": client_order_index,
        "order_id": order_id,
        "client_order_id": cloid_label,
        "market_index": 0,
        "owner_account_index": TEST_ACCOUNT_INDEX as i64,
        "initial_base_amount": "0.0050",
        "price": "2361.31",
        "nonce": nonce,
        "remaining_base_amount": "0.0050",
        "is_ask": false,
        "base_size": 50,
        "base_price": 236_131,
        "filled_base_amount": "0.0000",
        "filled_quote_amount": "0.000000",
        "side": "buy",
        "type": "limit",
        "time_in_force": "good-till-time",
        "reduce_only": false,
        "trigger_price": "0.00",
        "order_expiry": 1_780_360_584_479_i64,
        "status": "open",
        "trigger_status": "na",
        "trigger_time": 0,
        "parent_order_index": 0,
        "parent_order_id": "0",
        "to_trigger_order_id_0": "0",
        "to_trigger_order_id_1": "0",
        "to_cancel_order_id_0": "0",
        "integrator_fee_collector_index": "0",
        "integrator_taker_fee": "0",
        "integrator_maker_fee": "0",
        "block_height": 227_535_532,
        "timestamp": 1_777_941_383_576_i64,
        "created_at": 1_777_941_383_576_i64,
        "updated_at": 1_777_941_383_576_i64,
        "transaction_time": 1_777_941_383_576_735_i64,
    })
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn test_cancel_all_orders_iterates_open_orders_and_dispatches_cancel_per_order() {
    // `cancel_all_orders` walks `cache.orders_open` for the target
    // instrument and routes each through `cancel_order`, which depends
    // on `dispatch.lookup_venue_order_id` because the synthesised
    // CancelOrder commands carry `venue_order_id: None`. The test seeds
    // both halves of that contract via [`seed_open_order`] so a
    // regression that stops iterating (or stops resolving venue order
    // ids) would surface here as a zero-frame count.
    let (addr, state) = start_server().await;
    let (mut client, mut rx, cache) = build_client(addr);
    client.connect().await.expect("connect");
    await_subscribe_count(&state, 4).await;

    seed_open_order(
        &client,
        &cache,
        &state,
        &mut rx,
        "O-CXLALL-1",
        "281476929510120",
    )
    .await;
    seed_open_order(
        &client,
        &cache,
        &state,
        &mut rx,
        "O-CXLALL-2",
        "281476929510121",
    )
    .await;

    let baseline = state.send_txs().await.len();
    let cancel_all = CancelAllOrders::new(
        trader_id(),
        Some(client_id()),
        strategy_id(),
        eth_perp_id(),
        None,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );
    client
        .cancel_all_orders(cancel_all)
        .expect("cancel_all_orders");

    await_send_tx_count(&state, baseline + 2).await;
    let new_frames = state.send_txs().await[baseline..].to_vec();
    assert_eq!(new_frames.len(), 2);
    let mut cancelled_indices: Vec<i64> = new_frames
        .iter()
        .map(|frame| {
            // CancelOrder tx_type discriminant.
            assert_eq!(send_tx_type(frame), 15);
            send_tx_info(frame)["Index"]
                .as_i64()
                .expect("CancelOrder tx_info.Index")
        })
        .collect();
    cancelled_indices.sort_unstable();
    // The two voi values pinned by `seed_open_order` above. Asserting
    // both Index values appear (rather than just the frame count) rules
    // out a regression where `cancel_all_orders` cancels the same order
    // twice, or cancels the wrong subset of open orders.
    assert_eq!(
        cancelled_indices,
        vec![281_476_929_510_120_i64, 281_476_929_510_121_i64],
    );

    client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn test_cancel_all_orders_venue_rejection_suppresses_cancel_rejected_for_open_order() {
    let (addr, state) = start_server().await;
    let (mut client, mut rx, cache) = build_client(addr);
    client.connect().await.expect("connect");
    await_subscribe_count(&state, 4).await;

    seed_open_order(
        &client,
        &cache,
        &state,
        &mut rx,
        "O-CXLALL-REJECT",
        "281476929510122",
    )
    .await;

    *state.next_send_tx_ack.lock().await = Some(json!({
        "type": "jsonapi/sendtx",
        "code": 21727,
        "message": "order is not cancelable",
    }));

    let baseline = state.send_txs().await.len();
    let cancel_all = CancelAllOrders::new(
        trader_id(),
        Some(client_id()),
        strategy_id(),
        eth_perp_id(),
        None,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );
    client
        .cancel_all_orders(cancel_all)
        .expect("cancel_all_orders");

    await_send_tx_count(&state, baseline + 1).await;
    assert!(
        next_order_event(&mut rx, Duration::from_millis(250))
            .await
            .is_none(),
        "cancel-all venue rejection for an open order must not emit an invalid cancel rejection",
    );

    client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn test_batch_cancel_orders_fans_out_correlated_cancel_orders() {
    let (addr, state) = start_server().await;
    let (mut client, mut rx, cache) = build_client(addr);
    client.connect().await.expect("connect");

    let cancels = (1..=3)
        .map(|i| {
            let order_id = format!("O-BATCH-{i}");
            let order = make_limit_order(
                order_id.as_str(),
                OrderSide::Buy,
                Quantity::from("0.0050"),
                Price::from("2361.31"),
                TimeInForce::Gtc,
                false,
                false,
            );
            let client_order_id = order.client_order_id();
            cache_order(&cache, order);

            CancelOrder::new(
                trader_id(),
                Some(client_id()),
                strategy_id(),
                eth_perp_id(),
                client_order_id,
                Some(VenueOrderId::from(format!("28147692951030{i}").as_str())),
                UUID4::new(),
                UnixNanos::default(),
                None,
                None,
            )
        })
        .collect::<Vec<_>>();

    let batch = BatchCancelOrders::new(
        trader_id(),
        Some(client_id()),
        strategy_id(),
        eth_perp_id(),
        cancels,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );
    client.batch_cancel_orders(batch).expect("batch_cancel");
    await_send_tx_count(&state, 3).await;
    let frames = state.send_txs().await;
    assert_eq!(frames.len(), 3);
    assert!(frames.iter().all(|frame| frame["type"] == "jsonapi/sendtx"));
    assert!(frames.iter().all(|frame| send_tx_type(frame) == 15));
    let infos = frames.iter().map(send_tx_info).collect::<Vec<_>>();
    let first_nonce = infos[0]["Nonce"].as_i64().expect("first nonce");
    assert_eq!(infos[1]["Nonce"].as_i64(), Some(first_nonce + 1));
    assert_eq!(infos[2]["Nonce"].as_i64(), Some(first_nonce + 2));
    let mut cancelled_indices: Vec<i64> = infos
        .iter()
        .map(|info| info["Index"].as_i64().expect("CancelOrder tx_info.Index"))
        .collect();
    cancelled_indices.sort_unstable();
    assert_eq!(
        cancelled_indices,
        vec![
            281_476_929_510_301_i64,
            281_476_929_510_302_i64,
            281_476_929_510_303_i64,
        ],
    );
    assert!(
        next_order_event(&mut rx, Duration::from_millis(100))
            .await
            .is_none(),
        "sendTx handoff must wait for account stream cancel outcomes",
    );
    client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn test_reconnect_replays_and_immediately_refreshes_authenticated_subscriptions() {
    // The WS layer auto-reconnects on a server-initiated close. After the
    // reconnect the 5 account-stream subscribes must replay with their
    // auth token; otherwise the typed execution stream would silently
    // drop. The data-client variant of this test pins the public-channel
    // replay; this pins the authenticated path.
    let (addr, state) = start_server().await;
    let (mut client, _rx, cache) = build_client(addr);
    client.connect().await.expect("connect");
    await_subscribe_count(&state, 5).await;
    state.subscribe_ack_delay_ms.store(100, Ordering::Relaxed);

    // Arm the server-side close. The next inbound frame from the client
    // closes the socket; we then send a no-op cancel to fire that frame.
    // Reconnect first replays the five tracked subscriptions, then notifies
    // the auth task to mint a fresh token and re-subscribe immediately.
    state.close_after_next_frame.store(true, Ordering::Relaxed);
    let order = make_limit_order(
        "O-RECONNECT-TICKLE",
        OrderSide::Buy,
        Quantity::from("0.0050"),
        Price::from("2361.31"),
        TimeInForce::Gtc,
        false,
        false,
    );
    let client_order_id = order.client_order_id();
    cache_order(&cache, order);
    let _ = client.cancel_order(CancelOrder::new(
        trader_id(),
        Some(client_id()),
        strategy_id(),
        eth_perp_id(),
        client_order_id,
        Some(VenueOrderId::from("1")),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    ));

    wait_until_async(
        || {
            let state = Arc::clone(&state);
            async move {
                let subs = state.subscribes.lock().await;
                [
                    "account_all_orders",
                    "account_all_trades",
                    "account_all_positions",
                    "account_all_assets",
                    "user_stats",
                ]
                .iter()
                .all(|prefix| {
                    subs.iter()
                        .filter(|s| s["channel"].as_str().unwrap_or("").starts_with(prefix))
                        .count()
                        >= 3
                })
            }
        },
        Duration::from_secs(10),
    )
    .await;

    let subs = state.subscribes().await;

    for prefix in [
        "account_all_orders",
        "account_all_trades",
        "account_all_positions",
        "account_all_assets",
        "user_stats",
    ] {
        let channel_subs = subs
            .iter()
            .filter(|sub| {
                sub["channel"]
                    .as_str()
                    .is_some_and(|channel| channel.starts_with(prefix))
            })
            .collect::<Vec<_>>();
        assert!(
            channel_subs.len() >= 3,
            "expected three subscribes for {prefix}, received {channel_subs:?}",
        );
        let initial_auth = channel_subs[0]["auth"]
            .as_str()
            .expect("initial account subscribe must carry auth");
        let replay_auth = channel_subs[1]["auth"]
            .as_str()
            .expect("replayed account subscribe must carry auth");
        let refreshed_auth = channel_subs[2]["auth"]
            .as_str()
            .expect("refreshed account subscribe must carry auth");
        assert_eq!(
            replay_auth, initial_auth,
            "{prefix} reconnect replay must use the stored token",
        );
        assert_ne!(
            refreshed_auth, replay_auth,
            "{prefix} auth refresh must be venue-visible",
        );
    }

    client.disconnect().await.expect("disconnect");
}

fn http_orders_payload(orders: &[Value], next_cursor: Option<&str>) -> Value {
    json!({
        "code": 200,
        "next_cursor": next_cursor,
        "orders": orders,
    })
}

fn http_order_fixture(
    order_id: &str,
    client_order_id: &str,
    status: &str,
    filled_base: &str,
) -> Value {
    json!({
        "order_index": order_id.parse::<i64>().unwrap(),
        "client_order_index": client_order_id.parse::<i64>().unwrap_or(0),
        "order_id": order_id,
        "client_order_id": client_order_id,
        "market_index": 0,
        "owner_account_index": TEST_ACCOUNT_INDEX as i64,
        "initial_base_amount": "0.0050",
        "price": "2361.31",
        "nonce": 100,
        "remaining_base_amount": "0.0050",
        "is_ask": false,
        "base_size": 50,
        "base_price": 236_131,
        "filled_base_amount": filled_base,
        "filled_quote_amount": "0.000000",
        "side": "buy",
        "type": "limit",
        "time_in_force": "good-till-time",
        "reduce_only": false,
        "trigger_price": "0.00",
        "order_expiry": 1_780_360_584_479_i64,
        "status": status,
        "trigger_status": "na",
        "trigger_time": 0,
        "parent_order_index": 0,
        "parent_order_id": "0",
        "to_trigger_order_id_0": "0",
        "to_trigger_order_id_1": "0",
        "to_cancel_order_id_0": "0",
        "integrator_fee_collector_index": "0",
        "integrator_taker_fee": "0",
        "integrator_maker_fee": "0",
        "block_height": 227_535_532,
        "timestamp": 1_777_941_383_576_i64,
        "created_at": 1_777_941_383_576_i64,
        "updated_at": 1_777_941_383_576_i64,
        "transaction_time": 1_777_941_383_576_735_i64,
    })
}

fn http_trade_fixture(trade_id: i64, bid_client_id: i64) -> Value {
    json!({
        "trade_id": trade_id,
        "trade_id_str": trade_id.to_string(),
        "tx_hash": "000000128b1ee814",
        "type": "trade",
        "market_id": 0,
        "size": "0.1336",
        "price": "2352.73",
        "usd_amount": "314.324728",
        "ask_id": 281_476_929_510_102_i64,
        "ask_id_str": "281476929510102",
        "bid_id": 562_947_905_631_053_i64,
        "bid_id_str": "562947905631053",
        "ask_client_id": 0,
        "ask_client_id_str": "0",
        "bid_client_id": bid_client_id,
        "bid_client_id_str": bid_client_id.to_string(),
        "ask_account_id": 91249,
        "bid_account_id": TEST_ACCOUNT_INDEX as i64,
        "is_maker_ask": true,
        "block_height": 227_535_535,
        "timestamp": 1_777_941_384_181_i64,
        "taker_fee": 196,
        "maker_fee": 28,
        "transaction_time": 1_777_941_384_181_586_i64,
    })
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn test_generate_mass_status_fans_out_active_inactive_position_and_trades() {
    let (addr, state) = start_server().await;
    let (mut client, _rx, _cache) = build_client(addr);
    client.connect().await.expect("connect");
    await_subscribe_count(&state, 4).await;

    // Drive the account-active market set so the fan-out actually hits the
    // active / inactive endpoints. The consumption loop notes a market whenever
    // an account_all_* frame mentions it; the position fixture exists in
    // test_data and carries market_id=0, matching our test instrument.
    state.push_frame(&load_json("ws_account_all_positions_update.json"));

    wait_until_async(
        || {
            let client_ptr = std::ptr::addr_of!(client);
            async move {
                // SAFETY: this test owns `client` exclusively.
                let client = unsafe { &*client_ptr };
                !client
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
                    .unwrap_or_default()
                    .is_empty()
            }
        },
        Duration::from_secs(5),
    )
    .await;

    // Install REST overrides for the fan-out.
    *state.active_orders_response.lock().await = Some(http_orders_payload(
        &[http_order_fixture(
            "281476929510200",
            "1001",
            "open",
            "0.0000",
        )],
        None,
    ));
    *state.inactive_orders_response.lock().await = Some(http_orders_payload(
        &[http_order_fixture(
            "281476929510201",
            "1002",
            "canceled",
            "0.0050",
        )],
        None,
    ));
    *state.trades_response.lock().await = Some(json!({"code":200,"trades":[]}));

    // `lookback_mins=None` so the inactive-orders timestamp filter is a
    // pass-through; otherwise the fixture's fixed `ts_last` could fall
    // outside the lookback window depending on wall-clock at test time.
    let mass = client
        .generate_mass_status(None)
        .await
        .expect("mass status")
        .expect("Some(mass_status)");

    assert!(
        state.active_orders_calls.load(Ordering::Relaxed) >= 1,
        "active orders endpoint should fan out",
    );
    assert!(
        state.inactive_orders_calls.load(Ordering::Relaxed) >= 1,
        "inactive orders endpoint should fan out",
    );
    assert!(
        state.trades_calls.load(Ordering::Relaxed) >= 1,
        "trades endpoint should fan out",
    );

    let order_reports = mass.order_reports();
    assert!(
        order_reports
            .values()
            .any(|r| r.order_status == OrderStatus::Accepted),
        "active orders should appear as Accepted (open) in mass status: {order_reports:?}",
    );
    assert!(
        order_reports
            .values()
            .any(|r| r.order_status == OrderStatus::Canceled),
        "inactive orders should include the canceled fixture: {order_reports:?}",
    );

    let positions = mass.position_reports();
    assert_eq!(positions.len(), 1);
    assert!(positions.contains_key(&eth_perp_id()));

    client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn test_generate_mass_status_seeds_market_fanout_from_inactive_orders() {
    let (addr, state) = start_server().await;
    let (mut client, _rx, _cache) = build_client(addr);
    client.connect().await.expect("connect");
    await_subscribe_count(&state, 4).await;

    *state.inactive_orders_response.lock().await = Some(http_orders_payload(
        &[http_order_fixture(
            "281476929510201",
            "1002",
            "canceled",
            "0.0050",
        )],
        None,
    ));
    *state.trades_response.lock().await = Some(json!({"code":200,"trades":[]}));

    let mass = client
        .generate_mass_status(None)
        .await
        .expect("mass status")
        .expect("Some(mass_status)");

    assert!(
        state.active_orders_calls.load(Ordering::Relaxed) >= 1,
        "active orders endpoint should fan out after active markets seeding",
    );
    assert!(
        state.inactive_orders_calls.load(Ordering::Relaxed) >= 2,
        "inactive orders should be used for seeding and per-market report fan-out",
    );

    let order_reports = mass.order_reports();
    assert!(
        order_reports
            .values()
            .any(|r| r.order_status == OrderStatus::Canceled),
        "inactive orders should seed active markets and appear in mass status: {order_reports:?}",
    );

    client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn test_generate_mass_status_restores_filled_orders_from_trade_market() {
    let (addr, state) = start_server().await;
    let (mut client, _rx, cache) = build_client(addr);
    client.connect().await.expect("connect");
    await_subscribe_count(&state, 4).await;

    let venue_order_id = VenueOrderId::from("562947905631053");
    let order = make_limit_order(
        "O-RESTORE-FILLED",
        OrderSide::Buy,
        Quantity::from("0.1336"),
        Price::from("2361.31"),
        TimeInForce::Gtc,
        false,
        false,
    );
    let (_, client_order_id) = cache_accepted_order(&cache, order, venue_order_id);
    let reused_venue_order_id = VenueOrderId::from("562947905631054");
    let reused_order = make_limit_order(
        "O-RESTORE-FILLED-REUSED",
        OrderSide::Buy,
        Quantity::from("0.1336"),
        Price::from("2361.31"),
        TimeInForce::Gtc,
        false,
        false,
    );
    let (_, reused_client_order_id) =
        cache_accepted_order(&cache, reused_order, reused_venue_order_id);

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time after epoch");
    let now_ms = now.as_millis() as i64;
    let now_secs = now.as_secs();
    let client_order_index = 42_i64;
    let mut filled_order = http_order_fixture(venue_order_id.as_str(), "42", "filled", "0.1336");
    filled_order["initial_base_amount"] = json!("0.1336");
    filled_order["remaining_base_amount"] = json!("0.0000");
    filled_order["timestamp"] = json!(now_secs as i64);
    filled_order["created_at"] = json!(now_secs as i64);
    filled_order["updated_at"] = json!(now_secs as i64);
    let mut reused_filled_order =
        http_order_fixture(reused_venue_order_id.as_str(), "42", "filled", "0.1336");
    reused_filled_order["initial_base_amount"] = json!("0.1336");
    reused_filled_order["remaining_base_amount"] = json!("0.0000");
    reused_filled_order["timestamp"] = json!(now_secs as i64);
    reused_filled_order["created_at"] = json!(now_secs as i64);
    reused_filled_order["updated_at"] = json!(now_secs as i64);
    let mut trade = http_trade_fixture(19_209_006_905, client_order_index);
    trade["timestamp"] = json!(now_ms);
    trade["transaction_time"] = json!(now_ms * 1_000);
    let mut reused_trade = http_trade_fixture(19_209_006_906, client_order_index);
    reused_trade["bid_id"] = json!(reused_venue_order_id.as_str().parse::<i64>().unwrap());
    reused_trade["bid_id_str"] = json!(reused_venue_order_id.as_str());
    reused_trade["timestamp"] = json!(now_ms);
    reused_trade["transaction_time"] = json!(now_ms * 1_000);

    *state.inactive_orders_unscoped_response.lock().await = Some(http_orders_payload(&[], None));
    *state.inactive_orders_response.lock().await = Some(http_orders_payload(
        &[filled_order, reused_filled_order],
        None,
    ));
    *state.trades_response.lock().await = Some(json!({"code":200,"trades":[trade, reused_trade]}));

    let mass = client
        .generate_mass_status(Some(60))
        .await
        .expect("mass status")
        .expect("Some(mass_status)");
    let order_reports = mass.order_reports();
    let order_report = order_reports
        .get(&venue_order_id)
        .expect("terminal order report");
    let fill_reports = mass.fill_reports();
    let fill_report = fill_reports
        .get(&venue_order_id)
        .and_then(|reports| reports.first())
        .expect("historical fill report");
    let reused_order_report = order_reports
        .get(&reused_venue_order_id)
        .expect("reused-index terminal order report");
    let reused_fill_report = fill_reports
        .get(&reused_venue_order_id)
        .and_then(|reports| reports.first())
        .expect("reused-index historical fill report");

    assert_eq!(state.active_orders_calls.load(Ordering::Relaxed), 1);
    assert_eq!(state.inactive_orders_calls.load(Ordering::Relaxed), 2);
    assert_eq!(state.trades_calls.load(Ordering::Relaxed), 1);
    assert_eq!(order_report.client_order_id, Some(client_order_id));
    assert_eq!(order_report.venue_order_id, venue_order_id);
    assert_eq!(order_report.order_side, Some(OrderSide::Buy));
    assert_eq!(order_report.order_type, OrderType::Limit);
    assert_eq!(order_report.order_status, OrderStatus::Filled);
    assert_eq!(order_report.quantity, Quantity::from("0.1336"));
    assert_eq!(order_report.filled_qty, Quantity::from("0.1336"));
    assert_eq!(order_report.price, Some(Price::from("2361.31")));
    assert_eq!(
        order_report.ts_accepted,
        UnixNanos::from(now_secs * 1_000_000_000),
    );
    assert_eq!(
        order_report.ts_last,
        UnixNanos::from(now_secs * 1_000_000_000),
    );
    assert_eq!(fill_report.client_order_id, Some(client_order_id));
    assert_eq!(fill_report.venue_order_id, venue_order_id);
    assert_eq!(fill_report.order_side, OrderSide::Buy);
    assert_eq!(fill_report.last_qty, Quantity::from("0.1336"));
    assert_eq!(fill_report.last_px, Price::from("2352.73"));
    assert_eq!(fill_report.commission, Money::from("0.000196 USDC"));
    assert_eq!(
        reused_order_report.client_order_id,
        Some(reused_client_order_id),
    );
    assert_eq!(reused_order_report.venue_order_id, reused_venue_order_id);
    assert_eq!(
        reused_fill_report.client_order_id,
        Some(reused_client_order_id),
    );
    assert_eq!(reused_fill_report.venue_order_id, reused_venue_order_id);

    client.disconnect().await.expect("disconnect");
}

#[rstest]
#[case::complete(true)]
#[case::missing_terminal_order(false)]
#[tokio::test(flavor = "multi_thread")]
async fn test_generate_bounded_mass_status_reports_snapshot_contract(
    #[case] terminal_order_available: bool,
) {
    let (addr, state) = start_server().await;
    let (mut client, _rx, _cache) = build_client(addr);
    client.connect().await.expect("connect");

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time after epoch");
    let now_ms = now.as_millis() as i64;
    let now_secs = now.as_secs() as i64;
    let venue_order_id = VenueOrderId::from("562947905631059");
    let trade_id = TradeId::from("19209006929");
    let mut closing_order = http_order_fixture(venue_order_id.as_str(), "49", "filled", "0.1336");
    closing_order["initial_base_amount"] = json!("0.1336");
    closing_order["remaining_base_amount"] = json!("0.0000");
    closing_order["is_ask"] = json!(true);
    closing_order["side"] = json!("sell");
    closing_order["reduce_only"] = json!(true);
    closing_order["timestamp"] = json!(now_secs);
    closing_order["created_at"] = json!(now_secs);
    closing_order["updated_at"] = json!(now_secs);
    let terminal_orders = if terminal_order_available {
        vec![closing_order]
    } else {
        vec![]
    };
    *state.inactive_orders_unscoped_response.lock().await =
        Some(http_orders_payload(&terminal_orders, None));
    *state.inactive_orders_response.lock().await =
        Some(http_orders_payload(&terminal_orders, None));

    let mut closing_trade = http_trade_fixture(19_209_006_929, 49);
    closing_trade["ask_id"] = json!(venue_order_id.as_str().parse::<i64>().unwrap());
    closing_trade["ask_id_str"] = json!(venue_order_id.as_str());
    closing_trade["ask_client_id"] = json!(49);
    closing_trade["ask_client_id_str"] = json!("49");
    closing_trade["ask_account_id"] = json!(TEST_ACCOUNT_INDEX as i64);
    closing_trade["bid_account_id"] = json!(TEST_ACCOUNT_INDEX as i64 + 1);
    closing_trade["timestamp"] = json!(now_ms);
    closing_trade["transaction_time"] = json!(now_ms * 1_000);

    // A trade older than the lookback start proves the venue served the whole
    // requested window; the client filters it out of the report set.
    let pre_window_ms = now_ms - 2 * 60 * 60 * 1_000;
    let mut pre_window_trade = closing_trade.clone();
    pre_window_trade["trade_id"] = json!(19_209_006_928_i64);
    pre_window_trade["trade_id_str"] = json!("19209006928");
    pre_window_trade["timestamp"] = json!(pre_window_ms);
    pre_window_trade["transaction_time"] = json!(pre_window_ms * 1_000);
    *state.trades_response.lock().await =
        Some(json!({"code":200,"trades":[closing_trade, pre_window_trade]}));

    let mass_status = client
        .generate_mass_status(Some(60))
        .await
        .expect("mass status")
        .expect("mass status available");
    let expected_start = UnixNanos::from(
        mass_status
            .ts_init
            .as_u64()
            .saturating_sub(60 * 60 * 1_000_000_000),
    );
    let order_reports = mass_status.order_reports();
    let fill_reports = mass_status.fill_reports();
    let position_reports = mass_status.position_reports();

    assert_eq!(mass_status.lookback_start(), Some(expected_start));
    assert_eq!(mass_status.reports_complete(), terminal_order_available);
    assert_eq!(order_reports.len(), usize::from(terminal_order_available));
    assert_eq!(fill_reports.len(), 1);
    assert_eq!(position_reports.len(), 1);
    let fill_report = &fill_reports[&venue_order_id][0];
    let position_report = &position_reports[&eth_perp_id()][0];

    if terminal_order_available {
        let order_report = order_reports
            .get(&venue_order_id)
            .expect("closing order report");
        assert_eq!(order_report.order_status, OrderStatus::Filled);
        assert_eq!(order_report.order_side, Some(OrderSide::Sell));
        assert!(order_report.reduce_only);
    }
    assert_eq!(fill_report.trade_id, trade_id);
    assert_eq!(fill_report.order_side, OrderSide::Sell);
    assert_eq!(position_report.position_side, PositionSide::Flat);
    assert_eq!(position_report.quantity, Quantity::zero(4));
    assert_eq!(position_report.signed_decimal_qty, Decimal::ZERO);
    assert_eq!(position_report.venue_position_id, None);

    client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn test_generate_bounded_mass_status_marks_truncated_trade_history_incomplete() {
    let (addr, state) = start_server().await;
    let (mut client, _rx, _cache) = build_client(addr);
    client.connect().await.expect("connect");

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time after epoch");
    let now_ms = now.as_millis() as i64;
    let now_secs = now.as_secs() as i64;
    let venue_order_id = VenueOrderId::from("562947905631061");
    let mut closing_order = http_order_fixture(venue_order_id.as_str(), "51", "filled", "0.1336");
    closing_order["initial_base_amount"] = json!("0.1336");
    closing_order["remaining_base_amount"] = json!("0.0000");
    closing_order["is_ask"] = json!(true);
    closing_order["side"] = json!("sell");
    closing_order["reduce_only"] = json!(true);
    closing_order["timestamp"] = json!(now_secs);
    closing_order["created_at"] = json!(now_secs);
    closing_order["updated_at"] = json!(now_secs);
    *state.inactive_orders_unscoped_response.lock().await =
        Some(http_orders_payload(&[closing_order.clone()], None));
    *state.inactive_orders_response.lock().await =
        Some(http_orders_payload(&[closing_order], None));

    let mut closing_trade = http_trade_fixture(19_209_006_931, 51);
    closing_trade["ask_id"] = json!(venue_order_id.as_str().parse::<i64>().unwrap());
    closing_trade["ask_id_str"] = json!(venue_order_id.as_str());
    closing_trade["ask_client_id"] = json!(51);
    closing_trade["ask_client_id_str"] = json!("51");
    closing_trade["ask_account_id"] = json!(TEST_ACCOUNT_INDEX as i64);
    closing_trade["bid_account_id"] = json!(TEST_ACCOUNT_INDEX as i64 + 1);
    closing_trade["timestamp"] = json!(now_ms);
    closing_trade["transaction_time"] = json!(now_ms * 1_000);

    // Retained trade history ends inside the lookback: the venue offers no
    // further cursor while its oldest served trade is still newer than the
    // requested start.
    let retained_edge_ms = now_ms - 30 * 60 * 1_000;
    let mut retained_edge_trade = closing_trade.clone();
    retained_edge_trade["trade_id"] = json!(19_209_006_932_i64);
    retained_edge_trade["trade_id_str"] = json!("19209006932");
    retained_edge_trade["timestamp"] = json!(retained_edge_ms);
    retained_edge_trade["transaction_time"] = json!(retained_edge_ms * 1_000);
    state.trades_responses.lock().await.extend([
        json!({"code":200,"trades":[closing_trade],"next_cursor":"retained-tail"}),
        json!({"code":200,"trades":[retained_edge_trade]}),
    ]);

    let mass_status = client
        .generate_mass_status(Some(60))
        .await
        .expect("mass status")
        .expect("mass status available");
    let order_reports = mass_status.order_reports();
    let fill_reports = mass_status.fill_reports();
    let venue_fills = &fill_reports[&venue_order_id];

    assert!(mass_status.lookback_start().is_some());
    assert!(!mass_status.reports_complete());
    assert_eq!(state.trades_calls.load(Ordering::Relaxed), 2);
    assert_eq!(order_reports.len(), 1);
    assert_eq!(
        order_reports[&venue_order_id].order_status,
        OrderStatus::Filled,
    );
    assert_eq!(fill_reports.len(), 1);
    assert_eq!(venue_fills.len(), 2);
    assert_eq!(venue_fills[0].trade_id, TradeId::from("19209006931"));
    assert_eq!(venue_fills[1].trade_id, TradeId::from("19209006932"));
    assert_eq!(mass_status.position_reports().len(), 1);

    client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn test_generate_unbounded_mass_status_stays_complete_when_trade_cursor_exhausts() {
    let (addr, state) = start_server().await;
    let (mut client, _rx, _cache) = build_client(addr);
    client.connect().await.expect("connect");

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time after epoch");
    let now_ms = now.as_millis() as i64;
    let now_secs = now.as_secs() as i64;
    let venue_order_id = VenueOrderId::from("562947905631062");
    let mut closing_order = http_order_fixture(venue_order_id.as_str(), "52", "filled", "0.1336");
    closing_order["initial_base_amount"] = json!("0.1336");
    closing_order["remaining_base_amount"] = json!("0.0000");
    closing_order["is_ask"] = json!(true);
    closing_order["side"] = json!("sell");
    closing_order["reduce_only"] = json!(true);
    closing_order["timestamp"] = json!(now_secs);
    closing_order["created_at"] = json!(now_secs);
    closing_order["updated_at"] = json!(now_secs);
    *state.inactive_orders_unscoped_response.lock().await =
        Some(http_orders_payload(&[closing_order.clone()], None));
    *state.inactive_orders_response.lock().await =
        Some(http_orders_payload(&[closing_order], None));

    let mut closing_trade = http_trade_fixture(19_209_006_933, 52);
    closing_trade["ask_id"] = json!(venue_order_id.as_str().parse::<i64>().unwrap());
    closing_trade["ask_id_str"] = json!(venue_order_id.as_str());
    closing_trade["ask_client_id"] = json!(52);
    closing_trade["ask_client_id_str"] = json!("52");
    closing_trade["ask_account_id"] = json!(TEST_ACCOUNT_INDEX as i64);
    closing_trade["bid_account_id"] = json!(TEST_ACCOUNT_INDEX as i64 + 1);
    closing_trade["timestamp"] = json!(now_ms);
    closing_trade["transaction_time"] = json!(now_ms * 1_000);

    // An unbounded request asks for whatever the venue retains, so exhausting
    // the trade cursor leaves no window uncovered.
    *state.trades_response.lock().await = Some(json!({"code":200,"trades":[closing_trade]}));

    let mass_status = client
        .generate_mass_status(None)
        .await
        .expect("mass status")
        .expect("mass status available");

    assert_eq!(mass_status.lookback_start(), None);
    assert!(mass_status.reports_complete());
    assert_eq!(state.trades_calls.load(Ordering::Relaxed), 1);
    assert_eq!(mass_status.order_reports().len(), 1);
    assert_eq!(
        mass_status.order_reports()[&venue_order_id].order_status,
        OrderStatus::Filled,
    );
    assert_eq!(mass_status.fill_reports().len(), 1);
    assert_eq!(
        mass_status.fill_reports()[&venue_order_id][0].trade_id,
        TradeId::from("19209006933"),
    );

    client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn test_generate_bounded_mass_status_stays_complete_when_no_trades_served() {
    let (addr, state) = start_server().await;
    let (mut client, _rx, _cache) = build_client(addr);
    client.connect().await.expect("connect");

    // An account with no retained trades has no history the venue could have
    // truncated, so the bounded window is covered.
    *state.inactive_orders_unscoped_response.lock().await = Some(http_orders_payload(&[], None));
    *state.inactive_orders_response.lock().await = Some(http_orders_payload(&[], None));
    *state.trades_response.lock().await = Some(json!({"code":200,"trades":[]}));

    let mass_status = client
        .generate_mass_status(Some(60))
        .await
        .expect("mass status")
        .expect("mass status available");

    assert!(mass_status.lookback_start().is_some());
    assert!(mass_status.reports_complete());
    assert_eq!(state.trades_calls.load(Ordering::Relaxed), 1);
    assert_eq!(mass_status.order_reports().len(), 0);
    assert_eq!(mass_status.fill_reports().len(), 0);
    assert_eq!(mass_status.position_reports().len(), 0);

    client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn test_generate_bounded_mass_status_rejects_skipped_position_row() {
    let (addr, state) = start_server().await;
    let (mut client, mut rx, _cache) = build_client(addr);
    client.connect().await.expect("connect");
    await_subscribe_count(&state, 4).await;

    let mut invalid_snapshot = load_json("ws_account_all_positions_update.json");
    invalid_snapshot["type"] = json!("subscribed/account_all_positions");
    invalid_snapshot["positions"]["0"]["position"] = json!("-1.5000");
    state.push_frame(&invalid_snapshot);

    let unexpected_position = next_event_matching(&mut rx, Duration::from_millis(250), |event| {
        matches!(
            event,
            ExecutionEvent::Report(ExecutionReport::Position(report))
                if report.instrument_id == eth_perp_id()
        )
    })
    .await;
    assert!(
        unexpected_position.is_none(),
        "invalid snapshot row must not emit a position report: {unexpected_position:?}",
    );

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time after epoch");
    let now_ms = now.as_millis() as i64;
    let now_secs = now.as_secs() as i64;
    let venue_order_id = VenueOrderId::from("562947905631060");
    let trade_id = TradeId::from("19209006930");
    let mut closing_order = http_order_fixture(venue_order_id.as_str(), "50", "filled", "0.1336");
    closing_order["initial_base_amount"] = json!("0.1336");
    closing_order["remaining_base_amount"] = json!("0.0000");
    closing_order["is_ask"] = json!(true);
    closing_order["side"] = json!("sell");
    closing_order["reduce_only"] = json!(true);
    closing_order["timestamp"] = json!(now_secs);
    closing_order["created_at"] = json!(now_secs);
    closing_order["updated_at"] = json!(now_secs);
    *state.inactive_orders_unscoped_response.lock().await =
        Some(http_orders_payload(&[closing_order.clone()], None));
    *state.inactive_orders_response.lock().await =
        Some(http_orders_payload(&[closing_order], None));

    let mut closing_trade = http_trade_fixture(19_209_006_930, 50);
    closing_trade["ask_id"] = json!(venue_order_id.as_str().parse::<i64>().unwrap());
    closing_trade["ask_id_str"] = json!(venue_order_id.as_str());
    closing_trade["ask_client_id"] = json!(50);
    closing_trade["ask_client_id_str"] = json!("50");
    closing_trade["ask_account_id"] = json!(TEST_ACCOUNT_INDEX as i64);
    closing_trade["bid_account_id"] = json!(TEST_ACCOUNT_INDEX as i64 + 1);
    closing_trade["timestamp"] = json!(now_ms);
    closing_trade["transaction_time"] = json!(now_ms * 1_000);
    *state.trades_response.lock().await = Some(json!({"code":200,"trades":[closing_trade]}));

    let mass_status = client
        .generate_mass_status(Some(60))
        .await
        .expect("mass status")
        .expect("mass status available");
    let order_reports = mass_status.order_reports();
    let fill_reports = mass_status.fill_reports();
    let position_reports = mass_status.position_reports();

    assert!(mass_status.lookback_start().is_some());
    assert!(!mass_status.reports_complete());
    assert_eq!(order_reports.len(), 1);
    assert_eq!(
        order_reports[&venue_order_id].order_status,
        OrderStatus::Filled
    );
    assert_eq!(fill_reports.len(), 1);
    assert_eq!(fill_reports[&venue_order_id][0].trade_id, trade_id);
    assert!(position_reports.is_empty());

    client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn test_generate_bounded_mass_status_keeps_active_orders_when_history_is_incomplete() {
    let (addr, state) = start_server().await;
    let (mut client, _rx, _cache) = build_client(addr);
    client.connect().await.expect("connect");
    await_subscribe_count(&state, 4).await;
    state.push_frame(&load_json("ws_account_all_positions_update.json"));
    wait_until_async(
        || {
            let client_ptr = std::ptr::addr_of!(client);
            async move {
                // SAFETY: this test owns `client` exclusively.
                let client = unsafe { &*client_ptr };
                client
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
                    .is_ok_and(|reports| reports.len() == 1)
            }
        },
        Duration::from_secs(5),
    )
    .await;

    let venue_order_id = VenueOrderId::from("281476929510200");
    *state.active_orders_response.lock().await = Some(http_orders_payload(
        &[http_order_fixture(
            venue_order_id.as_str(),
            "1001",
            "open",
            "0.0000",
        )],
        None,
    ));
    *state.inactive_orders_response.lock().await = Some(http_orders_payload(&[], Some("stuck")));
    *state.trades_response.lock().await = Some(json!({"code":200,"trades":[]}));

    let mass_status = client
        .generate_mass_status(Some(60))
        .await
        .expect("mass status")
        .expect("mass status available");
    let order_reports = mass_status.order_reports();
    let order_report = order_reports
        .get(&venue_order_id)
        .unwrap_or_else(|| panic!("active order report missing from {order_reports:?}"));

    assert!(mass_status.lookback_start().is_some());
    assert!(!mass_status.reports_complete());
    assert_eq!(order_reports.len(), 1);
    assert_eq!(order_report.venue_order_id, venue_order_id);
    assert_eq!(order_report.order_status, OrderStatus::Accepted);
    assert_eq!(order_report.filled_qty, Quantity::zero(4));
    assert_eq!(mass_status.fill_reports().len(), 0);
    assert_eq!(mass_status.position_reports().len(), 1);
    assert_eq!(state.active_orders_calls.load(Ordering::Relaxed), 1);
    assert_eq!(state.inactive_orders_calls.load(Ordering::Relaxed), 2);

    client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn test_generate_bounded_mass_status_keeps_active_orders_when_active_fetch_is_incomplete() {
    let (addr, state) = start_server().await;
    let (mut client, _rx, _cache) = build_client(addr);
    client.connect().await.expect("connect");
    await_subscribe_count(&state, 4).await;
    state.push_frame(&load_json("ws_account_all_positions_update.json"));
    wait_until_async(
        || {
            let client_ptr = std::ptr::addr_of!(client);
            async move {
                // SAFETY: this test owns `client` exclusively.
                let client = unsafe { &*client_ptr };
                client
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
                    .is_ok_and(|reports| reports.len() == 1)
            }
        },
        Duration::from_secs(5),
    )
    .await;

    let venue_order_id = VenueOrderId::from("281476929510201");
    let valid_order = http_order_fixture(venue_order_id.as_str(), "1002", "open", "0.0000");
    let mut unmapped_order = http_order_fixture("281476929510202", "1003", "open", "0.0000");
    unmapped_order["market_index"] = json!(999);
    state.active_orders_responses.lock().await.extend([
        json!("invalid active-orders response"),
        http_orders_payload(&[valid_order, unmapped_order], None),
    ]);
    *state.inactive_orders_response.lock().await = Some(http_orders_payload(&[], None));
    *state.trades_response.lock().await = Some(json!({"code":200,"trades":[]}));

    let mass_status = client
        .generate_mass_status(Some(60))
        .await
        .expect("mass status")
        .expect("mass status available");
    let order_reports = mass_status.order_reports();
    let order_report = order_reports
        .get(&venue_order_id)
        .expect("active order report");

    assert!(mass_status.lookback_start().is_some());
    assert!(!mass_status.reports_complete());
    assert_eq!(order_reports.len(), 1);
    assert_eq!(order_report.venue_order_id, venue_order_id);
    assert_eq!(order_report.order_status, OrderStatus::Accepted);
    assert_eq!(order_report.filled_qty, Quantity::zero(4));
    assert_eq!(mass_status.fill_reports().len(), 0);
    assert_eq!(mass_status.position_reports().len(), 1);
    assert_eq!(state.active_orders_calls.load(Ordering::Relaxed), 2);
    assert_eq!(state.inactive_orders_calls.load(Ordering::Relaxed), 0);

    client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn test_mass_status_keeps_fill_market_orders_when_history_is_incomplete() {
    let (addr, state) = start_server().await;
    let (mut client, _rx, _cache) = build_client(addr);
    client.connect().await.expect("connect");
    await_subscribe_count(&state, 4).await;

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time after epoch");
    let now_ms = now.as_millis() as i64;
    let venue_order_id = VenueOrderId::from("562947905631053");
    let order = http_order_fixture(venue_order_id.as_str(), "1004", "open", "0.0000");
    let mut trade = http_trade_fixture(19_209_006_934, 1004);
    trade["timestamp"] = json!(now_ms);
    trade["transaction_time"] = json!(now_ms * 1_000);

    *state.inactive_orders_unscoped_response.lock().await = Some(http_orders_payload(&[], None));
    *state.active_orders_response.lock().await = Some(http_orders_payload(&[order], None));
    *state.inactive_orders_response.lock().await = Some(http_orders_payload(&[], Some("stuck")));
    *state.trades_response.lock().await = Some(json!({"code":200,"trades":[trade]}));

    let mass_status = client
        .generate_mass_status(Some(60))
        .await
        .expect("mass status")
        .expect("mass status available");
    let order_reports = mass_status.order_reports();
    let fill_reports = mass_status.fill_reports();
    let order_report = order_reports
        .get(&venue_order_id)
        .expect("partial fill-market order report");
    let fill_report = &fill_reports[&venue_order_id][0];

    assert!(!mass_status.reports_complete());
    assert_eq!(order_reports.len(), 1);
    assert_eq!(order_report.venue_order_id, venue_order_id);
    assert_eq!(order_report.order_status, OrderStatus::Accepted);
    assert_eq!(fill_report.venue_order_id, venue_order_id);
    assert_eq!(fill_report.client_order_id, order_report.client_order_id);
    assert_eq!(state.active_orders_calls.load(Ordering::Relaxed), 1);
    assert_eq!(state.inactive_orders_calls.load(Ordering::Relaxed), 3);

    client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn test_generate_bounded_mass_status_marks_unmapped_fill_incomplete() {
    let (addr, state) = start_server().await;
    let (mut client, _rx, _cache) = build_client(addr);
    client.connect().await.expect("connect");

    let mut trade = http_trade_fixture(19_209_006_930, 50);
    trade["market_id"] = json!(999);
    *state.trades_response.lock().await = Some(json!({"code":200,"trades":[trade]}));

    let mass_status = client
        .generate_mass_status(Some(60))
        .await
        .expect("mass status")
        .expect("mass status available");

    assert!(mass_status.lookback_start().is_some());
    assert!(!mass_status.reports_complete());
    assert_eq!(mass_status.order_reports().len(), 0);
    assert_eq!(mass_status.fill_reports().len(), 0);
    assert_eq!(mass_status.position_reports().len(), 0);
    assert_eq!(state.trades_calls.load(Ordering::Relaxed), 1);

    client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn test_generate_mass_status_excludes_old_fill_without_poisoning_replay() {
    let (addr, state) = start_server().await;
    let (mut client, mut rx, _cache) = build_client(addr);
    client.connect().await.expect("connect");
    await_subscribe_count(&state, 4).await;

    let old_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time after epoch")
        .as_millis() as i64
        - 2 * 60 * 60 * 1_000;
    let mut trade = http_trade_fixture(19_209_006_906, 42);
    trade["timestamp"] = json!(old_ms);
    trade["transaction_time"] = json!(old_ms * 1_000);
    *state.inactive_orders_unscoped_response.lock().await = Some(http_orders_payload(&[], None));
    *state.trades_response.lock().await =
        Some(json!({"code":200,"trades":[trade.clone()],"next_cursor":"older"}));

    let mass = client
        .generate_mass_status(Some(60))
        .await
        .expect("mass status")
        .expect("Some(mass_status)");

    assert!(mass.order_reports().is_empty());
    assert!(mass.fill_reports().is_empty());
    assert_eq!(state.active_orders_calls.load(Ordering::Relaxed), 0);
    assert_eq!(state.inactive_orders_calls.load(Ordering::Relaxed), 1);
    assert_eq!(state.trades_calls.load(Ordering::Relaxed), 1);
    let trades_queries = state.trades_queries.lock().await;
    assert_eq!(trades_queries.len(), 1);
    assert!(!trades_queries[0].contains_key("from"));
    drop(trades_queries);

    state.push_frame(&json!({
        "type": "update/account_all_trades",
        "channel": format!("account_all_trades:{TEST_ACCOUNT_INDEX}"),
        "trades": {"0": [trade]},
    }));
    let replay = next_event_matching(&mut rx, Duration::from_secs(2), |event| {
        matches!(event, ExecutionEvent::Report(ExecutionReport::Fill(_)))
    })
    .await
    .expect("live replay of lookback-excluded fill");

    match replay {
        ExecutionEvent::Report(ExecutionReport::Fill(report)) => {
            assert_eq!(report.trade_id.to_string(), "19209006906");
        }
        other => panic!("expected FillReport, was {other:?}"),
    }

    client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn test_account_all_trades_dedupes_across_reconnect() {
    // The dispatcher keys fill dedup on `TradeId`; a duplicate fill on
    // reconnect must not produce two OrderFilled events. We push the
    // same trade frame twice and assert exactly one fill reaches the
    // event channel.
    //
    // Routing the fill through the tracked path requires a known cloid;
    // we register one synthetically by submitting an order first so the
    // venue echo's cloid number resolves to our ClientOrderId via the
    // cloid map.
    let (addr, state) = start_server().await;
    let (mut client, mut rx, cache) = build_client(addr);
    client.connect().await.expect("connect");
    await_subscribe_count(&state, 4).await;

    let order = make_limit_order(
        "O-FILL-DEDUP",
        OrderSide::Buy,
        Quantity::from("0.1336"),
        Price::from("2352.73"),
        TimeInForce::Gtc,
        false,
        false,
    );
    cache_order(&cache, order.clone());
    client
        .submit_order(submit_command(&order))
        .expect("submit_order");
    // The optimistic submitted event lands first.
    let _submitted = next_order_event(&mut rx, Duration::from_secs(2)).await;
    await_send_tx_count(&state, 1).await;

    // Resolve the venue-side cloid index the adapter actually picked
    // (collision probe may have bumped it forward). Read it back from the
    // sendTx payload we just observed.
    let info = send_tx_info(&state.send_txs().await[0]);
    let venue_cloid_index = info["ClientOrderIndex"]
        .as_i64()
        .expect("ClientOrderIndex in tx_info");

    // Build a trade frame with the matching bid_client_id so the dispatch
    // resolves the cloid through the cloid map and emits a typed
    // OrderFilled. Numeric values pinned to the venue's published
    // `account_all_trades` shape.
    let trade_frame = json!({
        "type": "update/account_all_trades",
        "channel": format!("account_all_trades:{TEST_ACCOUNT_INDEX}"),
        "trades": {
            "0": [http_trade_fixture(19_209_006_902, venue_cloid_index)]
        }
    });

    // Push the first fill on the live socket and wait for the typed
    // OrderFilled to drain through the consumption loop, then force a
    // reconnect via the server-side close primitive and push the same
    // frame again on the replayed connection. `seen_trade_ids` is owned
    // by `WsDispatchState` and intentionally NOT cleared on the
    // Reconnected arm of the consumption loop; a regression that wipes
    // it during reconnect would let the duplicate fill through.
    state.push_frame(&trade_frame);

    let mut fills = 0_usize;
    let first_fill = next_event_matching(&mut rx, Duration::from_secs(2), |e| {
        matches!(e, ExecutionEvent::Order(OrderEventAny::Filled(_)))
    })
    .await
    .expect("first OrderFilled");

    if matches!(first_fill, ExecutionEvent::Order(OrderEventAny::Filled(_))) {
        fills += 1;
    }

    // Arm the server-side close and tickle a sendTx so the next inbound
    // frame closes the socket; the WS layer reconnects and replays the
    // 4 account subscriptions.
    let subs_before_reconnect = state.subscribes().await.len();
    state.close_after_next_frame.store(true, Ordering::Relaxed);
    let reconnect_order = make_limit_order(
        "O-DEDUP-RECONNECT",
        OrderSide::Buy,
        Quantity::from("0.0050"),
        Price::from("2361.31"),
        TimeInForce::Gtc,
        false,
        false,
    );
    let reconnect_client_order_id = reconnect_order.client_order_id();
    cache_order(&cache, reconnect_order);
    let _ = client.cancel_order(CancelOrder::new(
        trader_id(),
        Some(client_id()),
        strategy_id(),
        eth_perp_id(),
        reconnect_client_order_id,
        Some(VenueOrderId::from("1")),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    ));
    await_subscribe_count(&state, subs_before_reconnect + 4).await;

    // Push the duplicate on the post-reconnect socket; the broadcast
    // inbox flushes to whichever socket is currently live.
    state.push_frame(&trade_frame);

    let mut other_events = 0_usize;
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while std::time::Instant::now() < deadline {
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        match tokio::time::timeout(remaining.min(Duration::from_millis(200)), rx.recv()).await {
            Ok(Some(ExecutionEvent::Order(OrderEventAny::Filled(_)))) => fills += 1,
            Ok(Some(_)) => other_events += 1,
            Ok(None) | Err(_) => {}
        }
    }

    assert_eq!(
        fills, 1,
        "TradeId dedup must survive reconnect and collapse the duplicate fill \
         to a single OrderFilled (other_events seen: {other_events})",
    );

    client.disconnect().await.expect("disconnect");
}

#[rstest]
#[case::inside(
    1_700_000_000_000,
    vec!["562947905631051", "562947905631052"],
    vec!["19209006921", "19209006922"],
)]
#[case::outside(1_700_000_003_000, vec![], vec![])]
#[case::split(
    1_700_000_001_500,
    vec!["562947905631052"],
    vec!["19209006922"],
)]
#[case::opening_on_boundary(
    1_700_000_001_000,
    vec!["562947905631051", "562947905631052"],
    vec!["19209006921", "19209006922"],
)]
#[tokio::test(flavor = "multi_thread")]
async fn test_generate_reports_fixed_lifecycle_cutoff(
    #[case] start_ms: u64,
    #[case] expected_order_ids: Vec<&str>,
    #[case] expected_trade_ids: Vec<&str>,
) {
    const OPEN_MS: i64 = 1_700_000_001_000;
    const CLOSE_MS: i64 = 1_700_000_002_000;
    const OPEN_ORDER_ID: &str = "562947905631051";
    const CLOSE_ORDER_ID: &str = "562947905631052";

    let (addr, state) = start_server().await;
    let (mut client, _rx, _cache) = build_client(addr);
    client.connect().await.expect("connect");

    let mut opening_order = http_order_fixture(OPEN_ORDER_ID, "41", "filled", "0.0050");
    opening_order["remaining_base_amount"] = json!("0.0000");
    opening_order["timestamp"] = json!(OPEN_MS / 1_000);
    opening_order["created_at"] = json!(OPEN_MS / 1_000);
    opening_order["updated_at"] = json!(OPEN_MS / 1_000);
    let mut closing_order = http_order_fixture(CLOSE_ORDER_ID, "42", "filled", "0.0050");
    closing_order["remaining_base_amount"] = json!("0.0000");
    closing_order["is_ask"] = json!(true);
    closing_order["side"] = json!("sell");
    closing_order["reduce_only"] = json!(true);
    closing_order["timestamp"] = json!(CLOSE_MS / 1_000);
    closing_order["created_at"] = json!(CLOSE_MS / 1_000);
    closing_order["updated_at"] = json!(CLOSE_MS / 1_000);
    *state.inactive_orders_response.lock().await =
        Some(http_orders_payload(&[opening_order, closing_order], None));

    let mut opening_trade = http_trade_fixture(19_209_006_921, 41);
    opening_trade["bid_id"] = json!(OPEN_ORDER_ID.parse::<i64>().unwrap());
    opening_trade["bid_id_str"] = json!(OPEN_ORDER_ID);
    opening_trade["timestamp"] = json!(OPEN_MS);
    opening_trade["transaction_time"] = json!(OPEN_MS * 1_000);
    let mut closing_trade = http_trade_fixture(19_209_006_922, 42);
    closing_trade["ask_id"] = json!(CLOSE_ORDER_ID.parse::<i64>().unwrap());
    closing_trade["ask_id_str"] = json!(CLOSE_ORDER_ID);
    closing_trade["ask_client_id"] = json!(42);
    closing_trade["ask_client_id_str"] = json!("42");
    closing_trade["ask_account_id"] = json!(TEST_ACCOUNT_INDEX as i64);
    closing_trade["bid_account_id"] = json!(TEST_ACCOUNT_INDEX as i64 + 1);
    closing_trade["timestamp"] = json!(CLOSE_MS);
    closing_trade["transaction_time"] = json!(CLOSE_MS * 1_000);
    *state.trades_response.lock().await =
        Some(json!({"code":200,"trades":[opening_trade, closing_trade]}));

    let start = Some(UnixNanos::from(start_ms * 1_000_000));
    let order_reports = client
        .generate_order_status_reports(&GenerateOrderStatusReports::new(
            UUID4::new(),
            UnixNanos::from(CLOSE_MS as u64 * 1_000_000),
            false,
            Some(eth_perp_id()),
            start,
            None,
            None,
            None,
        ))
        .await
        .expect("order reports");
    let fill_reports = client
        .generate_fill_reports(GenerateFillReports::new(
            UUID4::new(),
            UnixNanos::from(CLOSE_MS as u64 * 1_000_000),
            Some(eth_perp_id()),
            None,
            start,
            None,
            None,
            None,
        ))
        .await
        .expect("fill reports");

    let actual_order_ids = order_reports
        .iter()
        .map(|report| report.venue_order_id.as_str())
        .collect::<Vec<_>>();
    let actual_trade_ids = fill_reports
        .iter()
        .map(|report| report.trade_id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(actual_order_ids, expected_order_ids);
    assert_eq!(actual_trade_ids, expected_trade_ids);

    if let Some(close_report) = order_reports
        .iter()
        .find(|report| report.venue_order_id == VenueOrderId::from(CLOSE_ORDER_ID))
    {
        assert_eq!(close_report.order_status, OrderStatus::Filled);
        assert_eq!(close_report.order_side, Some(OrderSide::Sell));
        assert!(close_report.reduce_only);
    }

    client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn test_generate_fill_reports_keeps_recent_fill_on_start_boundary_page() {
    let (addr, state) = start_server().await;
    let (mut client, _rx, _cache) = build_client(addr);
    client.connect().await.expect("connect");

    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time after epoch")
        .as_millis() as i64;
    let start_ms = now_ms - 60 * 60 * 1_000;
    let mut recent_trade = http_trade_fixture(19_209_006_907, 42);
    recent_trade["timestamp"] = json!(now_ms);
    recent_trade["transaction_time"] = json!(now_ms * 1_000);
    let mut boundary_trade = http_trade_fixture(19_209_006_908, 42);
    boundary_trade["timestamp"] = json!(start_ms);
    boundary_trade["transaction_time"] = json!(start_ms * 1_000);
    let mut old_trade = http_trade_fixture(19_209_006_906, 42);
    old_trade["timestamp"] = json!(start_ms - 1);
    old_trade["transaction_time"] = json!((start_ms - 1) * 1_000);
    *state.trades_response.lock().await = Some(json!({
        "code": 200,
        "trades": [recent_trade, boundary_trade, old_trade],
        "next_cursor": "older",
    }));

    let reports = client
        .generate_fill_reports(GenerateFillReports::new(
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
            Some(UnixNanos::from(start_ms as u64 * 1_000_000)),
            None,
            None,
            None,
        ))
        .await
        .expect("fill reports");

    let trade_ids = reports
        .iter()
        .map(|report| report.trade_id.to_string())
        .collect::<Vec<_>>();
    assert_eq!(trade_ids, ["19209006907", "19209006908"]);
    assert_eq!(state.trades_calls.load(Ordering::Relaxed), 1);

    client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn test_generate_fill_reports_skips_trade_seen_on_websocket() {
    let (addr, state) = start_server().await;
    let (mut client, mut rx, _cache) = build_client(addr);
    client.connect().await.expect("connect");
    await_subscribe_count(&state, 4).await;

    let trade = http_trade_fixture(19_209_006_903, 42);
    let trade_frame = json!({
        "type": "update/account_all_trades",
        "channel": format!("account_all_trades:{TEST_ACCOUNT_INDEX}"),
        "trades": {
            "0": [trade.clone()]
        }
    });

    state.push_frame(&trade_frame);
    next_event_matching(&mut rx, Duration::from_secs(2), |e| {
        matches!(e, ExecutionEvent::Report(ExecutionReport::Fill(_)))
    })
    .await
    .expect("first fill report");

    *state.trades_response.lock().await = Some(json!({"code":200,"trades":[trade]}));

    let reports = client
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
        .expect("fill reports");

    assert!(
        reports.is_empty(),
        "HTTP fill reports should skip trades already routed from WebSocket: {reports:?}",
    );

    client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn test_generate_fill_reports_is_repeatable_for_reconciliation_source() {
    let (addr, state) = start_server().await;
    let (mut client, _rx, _cache) = build_client(addr);
    client.connect().await.expect("connect");

    let trade = http_trade_fixture(19_209_006_904, 42);
    *state.trades_response.lock().await = Some(json!({"code":200,"trades":[trade]}));

    let request = || {
        GenerateFillReports::new(
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
            None,
            None,
            None,
            None,
        )
    };
    let first = client
        .generate_fill_reports(request())
        .await
        .expect("first reconciliation");
    let second = client
        .generate_fill_reports(request())
        .await
        .expect("second reconciliation");

    assert_eq!(first.len(), 1);
    assert_eq!(second.len(), 1);
    assert_eq!(first[0].trade_id, second[0].trade_id);

    client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn test_generate_fill_reports_rejects_repeated_cursor() {
    let (addr, state) = start_server().await;
    let (mut client, _rx, _cache) = build_client(addr);
    client.connect().await.expect("connect");
    *state.trades_response.lock().await =
        Some(json!({"code":200,"trades":[],"next_cursor":"stuck"}));

    let err = client
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
        .unwrap_err();

    assert!(err.to_string().contains("repeated cursor `stuck`"));
    assert_eq!(state.trades_calls.load(Ordering::Relaxed), 2);

    client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn test_failed_fill_sweep_does_not_poison_live_replay() {
    let (addr, state) = start_server().await;
    let (mut client, mut rx, _cache) = build_client(addr);
    client.connect().await.expect("connect");
    await_subscribe_count(&state, 4).await;

    let valid_trade = http_trade_fixture(19_209_006_935, 55);
    let mut invalid_trade = http_trade_fixture(19_209_006_936, 56);
    invalid_trade["market_id"] = json!(999);
    *state.inactive_orders_unscoped_response.lock().await = Some(http_orders_payload(&[], None));
    *state.trades_response.lock().await = Some(json!({
        "code": 200,
        "trades": [valid_trade.clone(), invalid_trade],
    }));

    let mass = client
        .generate_mass_status(Some(60))
        .await
        .expect("mass status")
        .expect("mass status available");

    assert!(!mass.reports_complete());
    assert!(mass.fill_reports().is_empty());

    state.push_frame(&json!({
        "type": "update/account_all_trades",
        "channel": format!("account_all_trades:{TEST_ACCOUNT_INDEX}"),
        "trades": {"0": [valid_trade]},
    }));
    let replay = next_event_matching(&mut rx, Duration::from_secs(2), |event| {
        matches!(event, ExecutionEvent::Report(ExecutionReport::Fill(_)))
    })
    .await
    .expect("live replay after failed fill sweep");

    match replay {
        ExecutionEvent::Report(ExecutionReport::Fill(report)) => {
            assert_eq!(report.trade_id, TradeId::from("19209006935"));
        }
        other => panic!("expected FillReport, was {other:?}"),
    }

    client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn test_generate_order_status_reports_rejects_repeated_inactive_cursor() {
    let (addr, state) = start_server().await;
    let (mut client, _rx, _cache) = build_client(addr);
    client.connect().await.expect("connect");
    *state.active_orders_response.lock().await = Some(http_orders_payload(&[], None));
    *state.inactive_orders_response.lock().await = Some(http_orders_payload(&[], Some("stuck")));

    let err = client
        .generate_order_status_reports(&GenerateOrderStatusReports::new(
            UUID4::new(),
            UnixNanos::default(),
            false,
            Some(eth_perp_id()),
            None,
            None,
            None,
            None,
        ))
        .await
        .unwrap_err();

    assert!(err.to_string().contains("repeated cursor `stuck`"));
    assert_eq!(state.inactive_orders_calls.load(Ordering::Relaxed), 2);

    client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn test_report_commands_reject_unknown_explicit_instrument_without_http_fanout() {
    let (addr, state) = start_server().await;
    let (mut client, _rx, _cache) = build_client(addr);
    client.connect().await.expect("connect");
    let unknown = InstrumentId::from("UNKNOWN-PERP.LIGHTER");

    let order_error = client
        .generate_order_status_reports(&GenerateOrderStatusReports::new(
            UUID4::new(),
            UnixNanos::default(),
            true,
            Some(unknown),
            None,
            None,
            None,
            None,
        ))
        .await
        .expect_err("unknown order-report instrument must fail");
    let fill_error = client
        .generate_fill_reports(GenerateFillReports::new(
            UUID4::new(),
            UnixNanos::default(),
            Some(unknown),
            None,
            None,
            None,
            None,
            None,
        ))
        .await
        .expect_err("unknown fill-report instrument must fail");
    let position_error = client
        .generate_position_status_reports(&GeneratePositionStatusReports::new(
            UUID4::new(),
            UnixNanos::default(),
            Some(unknown),
            None,
            None,
            None,
            None,
        ))
        .await
        .expect_err("unknown position-report instrument must fail");

    assert!(order_error.to_string().contains("order report instrument"));
    assert!(fill_error.to_string().contains("fill instrument"));
    assert!(
        position_error
            .to_string()
            .contains("position report instrument")
    );
    assert_eq!(state.active_orders_calls.load(Ordering::Relaxed), 0);
    assert_eq!(state.inactive_orders_calls.load(Ordering::Relaxed), 0);
    assert_eq!(state.trades_calls.load(Ordering::Relaxed), 0);

    client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn test_open_order_reports_fail_when_an_in_scope_row_cannot_be_parsed() {
    let (addr, state) = start_server().await;
    let (mut client, _rx, _cache) = build_client(addr);
    client.connect().await.expect("connect");
    let mut unmapped_order = http_order_fixture("281476929510202", "1003", "open", "0.0000");
    unmapped_order["market_index"] = json!(999);
    *state.active_orders_response.lock().await = Some(http_orders_payload(&[unmapped_order], None));

    let error = client
        .generate_order_status_reports(&GenerateOrderStatusReports::new(
            UUID4::new(),
            UnixNanos::default(),
            true,
            Some(eth_perp_id()),
            None,
            None,
            None,
            None,
        ))
        .await
        .expect_err("unmapped active order must fail direct reconciliation");

    assert!(
        error
            .to_string()
            .contains("incomplete Lighter order reports"),
        "unexpected error: {error:#}",
    );
    assert_eq!(state.active_orders_calls.load(Ordering::Relaxed), 1);

    client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn test_single_order_report_fails_when_matching_row_cannot_be_parsed() {
    let (addr, state) = start_server().await;
    let (mut client, _rx, _cache) = build_client(addr);
    client.connect().await.expect("connect");
    let venue_order_id = VenueOrderId::from("281476929510202");
    let mut unmapped_order = http_order_fixture(venue_order_id.as_str(), "1003", "open", "0.0000");
    unmapped_order["market_index"] = json!(999);
    *state.active_orders_response.lock().await = Some(http_orders_payload(&[unmapped_order], None));

    let error = client
        .generate_order_status_report(&GenerateOrderStatusReport::new(
            UUID4::new(),
            UnixNanos::default(),
            Some(eth_perp_id()),
            None,
            Some(venue_order_id),
            None,
            None,
        ))
        .await
        .expect_err("unmapped matching order must fail direct reconciliation");

    assert!(
        error
            .to_string()
            .contains("failed to parse matching Lighter order"),
        "unexpected error: {error:#}",
    );
    assert_eq!(state.active_orders_calls.load(Ordering::Relaxed), 1);
    assert_eq!(state.inactive_orders_calls.load(Ordering::Relaxed), 0);

    client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn test_generate_order_status_reports_excludes_order_before_identity_restore() {
    let (addr, state) = start_server().await;
    let (mut client, mut rx, cache) = build_client(addr);
    client.connect().await.expect("connect");
    await_subscribe_count(&state, 4).await;

    let venue_order_id = VenueOrderId::from("562947905631055");
    let order = make_limit_order(
        "O-RECON-EXCLUDED",
        OrderSide::Buy,
        Quantity::from("0.1336"),
        Price::from("2361.31"),
        TimeInForce::Gtc,
        false,
        false,
    );
    cache_accepted_order(&cache, order, venue_order_id);

    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time after epoch")
        .as_millis() as i64;
    let second_ms = now_ms / 1_000 * 1_000;
    let start_ms = second_ms + 500;
    let excluded_ms = start_ms - 1;
    let mut excluded_order = http_order_fixture(venue_order_id.as_str(), "42", "filled", "0.1336");
    excluded_order["timestamp"] = json!(excluded_ms);
    excluded_order["created_at"] = json!(excluded_ms);
    excluded_order["updated_at"] = json!(excluded_ms);
    *state.inactive_orders_response.lock().await =
        Some(http_orders_payload(&[excluded_order], None));

    let reports = client
        .generate_order_status_reports(&GenerateOrderStatusReports::new(
            UUID4::new(),
            UnixNanos::default(),
            false,
            Some(eth_perp_id()),
            Some(UnixNanos::from(start_ms as u64 * 1_000_000)),
            None,
            None,
            None,
        ))
        .await
        .expect("order status reports");

    assert!(reports.is_empty());
    assert_eq!(state.active_orders_calls.load(Ordering::Relaxed), 1);
    assert_eq!(state.inactive_orders_calls.load(Ordering::Relaxed), 1);

    let mut replay = http_trade_fixture(19_209_006_907, 42);
    replay["bid_id"] = json!(venue_order_id.as_str().parse::<i64>().unwrap());
    replay["bid_id_str"] = json!(venue_order_id.as_str());
    replay["timestamp"] = json!(now_ms);
    replay["transaction_time"] = json!(now_ms * 1_000);
    state.push_frame(&json!({
        "type": "update/account_all_trades",
        "channel": format!("account_all_trades:{TEST_ACCOUNT_INDEX}"),
        "trades": {"0": [replay]},
    }));
    let report = next_event_matching(&mut rx, Duration::from_secs(2), |event| {
        matches!(event, ExecutionEvent::Report(ExecutionReport::Fill(_)))
    })
    .await
    .expect("live fill report");

    match report {
        ExecutionEvent::Report(ExecutionReport::Fill(report)) => {
            assert_eq!(
                report.client_order_id,
                Some(ClientOrderId::new(venue_order_id.as_str())),
            );
        }
        other => panic!("expected FillReport, was {other:?}"),
    }

    client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn test_generate_order_status_report_rejects_repeated_inactive_cursor() {
    let (addr, state) = start_server().await;
    let (mut client, _rx, _cache) = build_client(addr);
    client.connect().await.expect("connect");
    *state.active_orders_response.lock().await = Some(http_orders_payload(&[], None));
    *state.inactive_orders_response.lock().await = Some(http_orders_payload(&[], Some("stuck")));

    let err = client
        .generate_order_status_report(&GenerateOrderStatusReport::new(
            UUID4::new(),
            UnixNanos::default(),
            Some(eth_perp_id()),
            None,
            Some(VenueOrderId::from("281476929510999")),
            None,
            None,
        ))
        .await
        .unwrap_err();

    assert!(err.to_string().contains("repeated cursor `stuck`"));
    assert_eq!(state.inactive_orders_calls.load(Ordering::Relaxed), 2);

    client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn test_generate_order_status_report_client_index_does_not_search_inactive_history() {
    let (addr, state) = start_server().await;
    let (mut client, _rx, _cache) = build_client(addr);
    client.connect().await.expect("connect");
    *state.active_orders_response.lock().await = Some(http_orders_payload(&[], None));
    *state.inactive_orders_response.lock().await = Some(http_orders_payload(&[], Some("stuck")));

    let report = client
        .generate_order_status_report(&GenerateOrderStatusReport::new(
            UUID4::new(),
            UnixNanos::default(),
            Some(eth_perp_id()),
            Some(ClientOrderId::from("O-NOT-FOUND")),
            None,
            None,
            None,
        ))
        .await
        .expect("client-index lookup");

    assert!(report.is_none());
    assert_eq!(state.inactive_orders_calls.load(Ordering::Relaxed), 0);

    client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn test_generate_order_status_report_resolves_single_active_client_index() {
    let (addr, state) = start_server().await;
    let (mut client, _rx, cache) = build_client(addr);
    client.connect().await.expect("connect");

    let order = make_limit_order(
        "O-SINGLE-ACTIVE-INDEX",
        OrderSide::Buy,
        Quantity::from("0.0050"),
        Price::from("2361.31"),
        TimeInForce::Gtc,
        false,
        false,
    );
    cache_order(&cache, order.clone());
    client.submit_order(submit_command(&order)).expect("submit");
    await_send_tx_count(&state, 1).await;
    let client_order_index = send_tx_info(&state.send_txs().await[0])["ClientOrderIndex"]
        .as_i64()
        .expect("ClientOrderIndex")
        .to_string();
    let venue_order_id = VenueOrderId::from("281476929510300");

    *state.active_orders_response.lock().await = Some(http_orders_payload(
        &[http_order_fixture(
            venue_order_id.as_str(),
            &client_order_index,
            "open",
            "0.0000",
        )],
        None,
    ));

    let report = client
        .generate_order_status_report(&GenerateOrderStatusReport::new(
            UUID4::new(),
            UnixNanos::default(),
            Some(eth_perp_id()),
            Some(order.client_order_id()),
            None,
            None,
            None,
        ))
        .await
        .expect("single active client-index lookup")
        .expect("order report");

    assert_eq!(report.client_order_id, Some(order.client_order_id()));
    assert_eq!(report.venue_order_id, venue_order_id);
    assert_eq!(state.inactive_orders_calls.load(Ordering::Relaxed), 0);

    client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn test_generate_order_status_report_preserves_pending_modify() {
    let (addr, state) = start_server().await;
    let (mut client, mut rx, cache) = build_client(addr);
    client.connect().await.expect("connect");

    let order = make_limit_order(
        "O-RECONCILE-PENDING-MODIFY",
        OrderSide::Buy,
        Quantity::from("0.0050"),
        Price::from("2361.31"),
        TimeInForce::Gtc,
        false,
        false,
    );
    let client_order_id = order.client_order_id();
    let venue_order_id = VenueOrderId::from("281476929510301");
    cache_order(&cache, order);

    let accepted = OrderEventAny::Accepted(OrderAccepted::new(
        trader_id(),
        strategy_id(),
        eth_perp_id(),
        client_order_id,
        venue_order_id,
        account_id(),
        UUID4::new(),
        UnixNanos::from(1),
        UnixNanos::from(1),
        false,
    ));
    cache
        .borrow_mut()
        .update_order(&accepted)
        .expect("apply OrderAccepted");
    let pending = OrderEventAny::PendingUpdate(OrderPendingUpdate::new(
        trader_id(),
        strategy_id(),
        eth_perp_id(),
        client_order_id,
        Some(account_id()),
        UUID4::new(),
        UnixNanos::from(2),
        UnixNanos::from(2),
        false,
        Some(venue_order_id),
    ));
    cache
        .borrow_mut()
        .update_order(&pending)
        .expect("apply OrderPendingUpdate");

    *state.active_orders_response.lock().await = Some(http_orders_payload(
        &[http_order_fixture(
            venue_order_id.as_str(),
            client_order_id.as_str(),
            "open",
            "0.0000",
        )],
        None,
    ));

    client
        .modify_order(ModifyOrder::new(
            trader_id(),
            Some(client_id()),
            strategy_id(),
            eth_perp_id(),
            client_order_id,
            Some(venue_order_id),
            Some(Quantity::from("0.0100")),
            Some(Price::from("2400.00")),
            None,
            UUID4::new(),
            UnixNanos::from(3),
            None,
            None,
        ))
        .expect("modify_order");
    await_send_tx_count(&state, 1).await;

    let report = client
        .generate_order_status_report(&GenerateOrderStatusReport::new(
            UUID4::new(),
            UnixNanos::from(4),
            Some(eth_perp_id()),
            Some(client_order_id),
            Some(venue_order_id),
            None,
            None,
        ))
        .await
        .expect("pending modify lookup")
        .expect("order report");

    assert_eq!(report.client_order_id, Some(client_order_id));
    assert_eq!(report.venue_order_id, venue_order_id);
    assert_eq!(report.account_id, account_id());
    assert_eq!(report.instrument_id, eth_perp_id());
    assert_eq!(report.order_status, OrderStatus::PendingUpdate);
    assert_eq!(report.quantity, Quantity::from("0.0050"));
    assert_eq!(report.price, Some(Price::from("2361.31")));
    assert_eq!(report.trigger_price, None);
    assert_eq!(report.filled_qty, Quantity::from("0.0000"));

    let cached = cache
        .borrow()
        .order_owned(&client_order_id)
        .expect("cached order");
    assert_eq!(cached.status(), OrderStatus::PendingUpdate);
    assert_eq!(cached.quantity(), Quantity::from("0.0050"));
    assert_eq!(cached.price(), Some(Price::from("2361.31")));
    assert_eq!(cached.trigger_price(), None);
    assert!(
        next_order_event(&mut rx, Duration::from_millis(100))
            .await
            .is_none(),
        "reconciliation lookup must emit no typed order event",
    );

    client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn test_generate_order_status_report_rejects_ambiguous_active_client_index() {
    let (addr, state) = start_server().await;
    let (mut client, _rx, cache) = build_client(addr);
    client.connect().await.expect("connect");

    let order = make_limit_order(
        "O-AMBIGUOUS-INDEX",
        OrderSide::Buy,
        Quantity::from("0.0050"),
        Price::from("2361.31"),
        TimeInForce::Gtc,
        false,
        false,
    );
    cache_order(&cache, order.clone());
    client.submit_order(submit_command(&order)).expect("submit");
    await_send_tx_count(&state, 1).await;
    let client_order_index = send_tx_info(&state.send_txs().await[0])["ClientOrderIndex"]
        .as_i64()
        .expect("ClientOrderIndex");
    let client_order_index = client_order_index.to_string();

    *state.active_orders_response.lock().await = Some(http_orders_payload(
        &[
            http_order_fixture("281476929510300", &client_order_index, "open", "0.0000"),
            http_order_fixture("281476929510301", &client_order_index, "open", "0.0000"),
        ],
        None,
    ));

    let err = client
        .generate_order_status_report(&GenerateOrderStatusReport::new(
            UUID4::new(),
            UnixNanos::default(),
            Some(eth_perp_id()),
            Some(order.client_order_id()),
            None,
            None,
            None,
        ))
        .await
        .unwrap_err();

    assert!(
        err.to_string()
            .contains("ambiguous Lighter active-order lookup")
    );
    assert_eq!(state.inactive_orders_calls.load(Ordering::Relaxed), 0);

    client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn test_order_status_reports_stop_repeated_active_market_seed_cursor() {
    let (addr, state) = start_server().await;
    let (mut client, _rx, _cache) = build_client(addr);
    client.connect().await.expect("connect");
    *state.inactive_orders_response.lock().await = Some(http_orders_payload(&[], Some("stuck")));

    let error = client
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
        .expect_err("incomplete active-market seed must fail reconciliation");

    assert!(error.to_string().contains("repeated cursor `stuck`"));
    assert_eq!(state.inactive_orders_calls.load(Ordering::Relaxed), 2);

    client.disconnect().await.expect("disconnect");
}

#[rstest]
#[case::long(1, PositionSide::Long)]
#[case::short(-1, PositionSide::Short)]
#[tokio::test(flavor = "multi_thread")]
async fn test_account_all_positions_empty_update_retains_cached_position(
    #[case] sign: i8,
    #[case] expected_side: PositionSide,
) {
    let (addr, state) = start_server().await;
    let (mut client, mut rx, _cache) = build_client(addr);
    client.connect().await.expect("connect");
    await_subscribe_count(&state, 4).await;

    let mut snapshot = load_json("ws_account_all_positions_update.json");
    snapshot["type"] = json!("subscribed/account_all_positions");
    snapshot["positions"]["0"]["sign"] = json!(sign);
    state.push_frame(&snapshot);

    next_event_matching(&mut rx, Duration::from_secs(2), |e| {
        matches!(
            e,
            ExecutionEvent::Report(ExecutionReport::Position(report))
                if report.instrument_id == eth_perp_id()
                    && report.position_side == expected_side
                    && report.quantity == Quantity::from("1.5000")
        )
    })
    .await
    .expect("initial position report");

    state.push_frame(&json!({
        "type": "update/account_all_positions",
        "channel": format!("account_all_positions:{TEST_ACCOUNT_INDEX}"),
        "positions": {},
        "shares": [],
        "last_funding_round": null,
        "last_funding_discount": null,
    }));

    let unexpected_close = next_event_matching(&mut rx, Duration::from_millis(250), |e| {
        matches!(
            e,
            ExecutionEvent::Report(ExecutionReport::Position(report))
                if report.instrument_id == eth_perp_id()
                    && report.position_side == PositionSide::Flat
                    && report.quantity.is_zero()
        ) || matches!(e, ExecutionEvent::Order(OrderEventAny::Filled(_)))
    })
    .await;
    assert!(
        unexpected_close.is_none(),
        "incomplete position update must not emit a flat report or synthetic close: \
         {unexpected_close:?}",
    );

    let positions = client
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
        .expect("position reports");
    assert_eq!(positions.len(), 1);
    assert_eq!(positions[0].account_id, account_id());
    assert_eq!(positions[0].instrument_id, eth_perp_id());
    assert_eq!(positions[0].position_side, expected_side);
    assert_eq!(positions[0].quantity, Quantity::from("1.5000"));
    assert_eq!(
        positions[0].signed_decimal_qty,
        rust_decimal::Decimal::new(i64::from(sign) * 15, 1),
    );
    assert_eq!(
        positions[0].avg_px_open,
        Some(rust_decimal::Decimal::new(235010, 2)),
    );
    assert_eq!(positions[0].venue_position_id, None);

    client.disconnect().await.expect("disconnect");
}

#[rstest]
#[case::empty_map(false)]
#[case::zero_position_row(true)]
#[tokio::test(flavor = "multi_thread")]
async fn test_account_all_positions_flat_snapshot_clears_cache_and_emits_flat_report(
    #[case] zero_position_row: bool,
) {
    let (addr, state) = start_server().await;
    let (mut client, mut rx, _cache) = build_client(addr);
    client.connect().await.expect("connect");
    await_subscribe_count(&state, 4).await;

    state.push_frame(&load_json("ws_account_all_positions_update.json"));

    wait_until_async(
        || {
            let client_ptr = std::ptr::addr_of!(client);
            async move {
                let client = unsafe { &*client_ptr };
                !client
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
                    .unwrap_or_default()
                    .is_empty()
            }
        },
        Duration::from_secs(5),
    )
    .await;

    let flat_snapshot = if zero_position_row {
        let mut snapshot = load_json("ws_account_all_positions_update.json");
        snapshot["positions"]["0"]["position"] = json!("0.0000");
        snapshot
    } else {
        json!({
            "type": "subscribed/account_all_positions",
            "channel": format!("account_all_positions:{TEST_ACCOUNT_INDEX}"),
            "positions": {},
            "shares": [],
            "last_funding_round": null,
            "last_funding_discount": null,
        })
    };
    state.push_frame(&flat_snapshot);

    let flat_report = next_event_matching(&mut rx, Duration::from_secs(2), |e| {
        matches!(
            e,
            ExecutionEvent::Report(ExecutionReport::Position(report))
                if report.instrument_id == eth_perp_id()
                    && report.position_side == PositionSide::Flat
                    && report.quantity.is_zero()
        )
    })
    .await
    .expect("flat position report");

    let ExecutionEvent::Report(ExecutionReport::Position(flat_report)) = flat_report else {
        unreachable!("predicate only accepts position reports");
    };
    assert_eq!(flat_report.account_id, account_id());
    assert_eq!(flat_report.instrument_id, eth_perp_id());
    assert_eq!(flat_report.position_side, PositionSide::Flat);
    assert_eq!(flat_report.quantity, Quantity::zero(0));
    assert!(flat_report.signed_decimal_qty.is_zero());
    assert_eq!(flat_report.ts_last, flat_report.ts_init);
    assert!(flat_report.ts_last > UnixNanos::default());
    assert_eq!(flat_report.venue_position_id, None);
    assert_eq!(flat_report.avg_px_open, None);

    let duplicate_flat = next_event_matching(&mut rx, Duration::from_millis(250), |e| {
        matches!(
            e,
            ExecutionEvent::Report(ExecutionReport::Position(report))
                if report.instrument_id == eth_perp_id()
                    && report.position_side == PositionSide::Flat
                    && report.quantity.is_zero()
        )
    })
    .await;
    assert!(
        duplicate_flat.is_none(),
        "flat snapshot must emit exactly one flat report: {duplicate_flat:?}",
    );

    let positions = client
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
        .expect("position reports");
    assert!(
        positions.is_empty(),
        "flat position snapshot must clear the prior cache, was {positions:?}",
    );

    client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn test_account_all_positions_invalid_known_market_does_not_flatten_cached_position() {
    let (addr, state) = start_server().await;
    let (mut client, mut rx, _cache) = build_client(addr);
    client.connect().await.expect("connect");
    await_subscribe_count(&state, 4).await;

    state.push_frame(&load_json("ws_account_all_positions_update.json"));

    next_event_matching(&mut rx, Duration::from_secs(2), |e| {
        matches!(
            e,
            ExecutionEvent::Report(ExecutionReport::Position(report))
                if report.instrument_id == eth_perp_id()
                    && report.quantity == Quantity::from("1.5000")
        )
    })
    .await
    .expect("initial position report");

    let mut invalid_position = load_json("ws_account_all_positions_update.json");
    invalid_position["positions"]["0"]["position"] = json!("-1.5000");
    state.push_frame(&invalid_position);

    let unexpected_flat = next_event_matching(&mut rx, Duration::from_millis(250), |e| {
        matches!(
            e,
            ExecutionEvent::Report(ExecutionReport::Position(report))
                if report.instrument_id == eth_perp_id()
                    && report.position_side == PositionSide::Flat
                    && report.quantity.is_zero()
        )
    })
    .await;

    assert!(
        unexpected_flat.is_none(),
        "invalid position row must not flatten cached positions: {unexpected_flat:?}",
    );

    wait_until_async(
        || {
            let client_ptr = std::ptr::addr_of!(client);
            async move {
                // SAFETY: this test owns `client` exclusively.
                let client = unsafe { &*client_ptr };
                client
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
                    .is_err()
            }
        },
        Duration::from_secs(2),
    )
    .await;

    let error = client
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
        .expect_err("incomplete position snapshot must fail direct reconciliation");

    assert!(
        error
            .to_string()
            .contains("position snapshot does not cover"),
        "unexpected error: {error:#}",
    );

    client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn test_bounded_mass_status_rejects_stale_position_coverage_after_reconnect() {
    let (addr, state) = start_server().await;
    let (mut client, _rx, cache) = build_client(addr);
    client.connect().await.expect("connect");
    await_subscribe_count(&state, 4).await;

    state.push_frame(&load_json("ws_account_all_positions_update.json"));
    wait_until_async(
        || {
            let client_ptr = std::ptr::addr_of!(client);
            async move {
                // SAFETY: this test owns `client` exclusively.
                let client = unsafe { &*client_ptr };
                client
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
                    .is_ok_and(|reports| reports.len() == 1)
            }
        },
        Duration::from_secs(5),
    )
    .await;

    state
        .auto_emit_account_subscribed_frames
        .store(false, Ordering::Relaxed);
    let subs_before_reconnect = state.subscribes().await.len();
    state.close_after_next_frame.store(true, Ordering::Relaxed);
    let reconnect_order = make_limit_order(
        "O-POSITION-COVERAGE-RECONNECT",
        OrderSide::Buy,
        Quantity::from("0.0050"),
        Price::from("2361.31"),
        TimeInForce::Gtc,
        false,
        false,
    );
    let reconnect_client_order_id = reconnect_order.client_order_id();
    cache_order(&cache, reconnect_order);
    let _ = client.cancel_order(CancelOrder::new(
        trader_id(),
        Some(client_id()),
        strategy_id(),
        eth_perp_id(),
        reconnect_client_order_id,
        Some(VenueOrderId::from("1")),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    ));
    await_subscribe_count(&state, subs_before_reconnect + 4).await;

    let venue_order_id = VenueOrderId::from("281476929510200");
    *state.active_orders_response.lock().await = Some(http_orders_payload(
        &[http_order_fixture(
            venue_order_id.as_str(),
            "1001",
            "open",
            "0.0000",
        )],
        None,
    ));
    *state.inactive_orders_response.lock().await = Some(http_orders_payload(&[], None));
    *state.trades_response.lock().await = Some(json!({"code":200,"trades":[]}));

    let mass_status = client
        .generate_mass_status(Some(60))
        .await
        .expect("mass status")
        .expect("mass status available");
    let order_reports = mass_status.order_reports();
    let position_reports = mass_status.position_reports();
    let order_report = order_reports
        .get(&venue_order_id)
        .expect("active order report");
    let position_report = &position_reports[&eth_perp_id()][0];

    assert!(mass_status.lookback_start().is_some());
    assert!(!mass_status.reports_complete());
    assert_eq!(order_reports.len(), 1);
    assert_eq!(order_report.order_status, OrderStatus::Accepted);
    assert_eq!(position_reports.len(), 1);
    assert_eq!(position_report.position_side, PositionSide::Long);
    assert_eq!(position_report.quantity, Quantity::from("1.5000"));

    client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn test_account_all_positions_empty_snapshot_after_reconnect_flattens_prior_position() {
    let (addr, state) = start_server().await;
    let (mut client, mut rx, cache) = build_client(addr);
    client.connect().await.expect("connect");
    await_subscribe_count(&state, 4).await;

    state.push_frame(&load_json("ws_account_all_positions_update.json"));

    wait_until_async(
        || {
            let client_ptr = std::ptr::addr_of!(client);
            async move {
                let client = unsafe { &*client_ptr };
                !client
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
                    .unwrap_or_default()
                    .is_empty()
            }
        },
        Duration::from_secs(5),
    )
    .await;

    // Force a transparent reconnect. The execution loop keeps the prior
    // position cache across this lifecycle event, then lets the next complete
    // venue snapshot drive the diff.
    let subs_before_reconnect = state.subscribes().await.len();
    state.close_after_next_frame.store(true, Ordering::Relaxed);
    let reconnect_order = make_limit_order(
        "O-POSITION-RECONNECT",
        OrderSide::Buy,
        Quantity::from("0.0050"),
        Price::from("2361.31"),
        TimeInForce::Gtc,
        false,
        false,
    );
    let reconnect_client_order_id = reconnect_order.client_order_id();
    cache_order(&cache, reconnect_order);
    let _ = client.cancel_order(CancelOrder::new(
        trader_id(),
        Some(client_id()),
        strategy_id(),
        eth_perp_id(),
        reconnect_client_order_id,
        Some(VenueOrderId::from("1")),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    ));
    await_subscribe_count(&state, subs_before_reconnect + 4).await;

    state.push_frame(&json!({
        "type": "subscribed/account_all_positions",
        "channel": format!("account_all_positions:{TEST_ACCOUNT_INDEX}"),
        "positions": {},
        "shares": [],
        "last_funding_round": null,
        "last_funding_discount": null,
    }));

    let flat_report = next_event_matching(&mut rx, Duration::from_secs(2), |e| {
        matches!(
            e,
            ExecutionEvent::Report(ExecutionReport::Position(report))
                if report.instrument_id == eth_perp_id()
                    && report.position_side == PositionSide::Flat
                    && report.quantity.is_zero()
        )
    })
    .await
    .expect("flat position report after reconnect");

    let ExecutionEvent::Report(ExecutionReport::Position(flat_report)) = flat_report else {
        unreachable!("predicate only accepts position reports");
    };
    assert_eq!(flat_report.instrument_id, eth_perp_id());
    assert_eq!(flat_report.position_side, PositionSide::Flat);
    assert!(flat_report.quantity.is_zero());

    let positions = client
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
        .expect("position reports");
    assert!(
        positions.is_empty(),
        "empty position snapshot after reconnect must clear the prior cache, was {positions:?}",
    );

    client.disconnect().await.expect("disconnect");
}
