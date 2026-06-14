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
//! harness mirrors the data-client scaffolding in `tests/data_client.rs`: the same
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
//! coverage lives in `tests/websocket.rs` and `tests/http.rs`.

use std::{
    cell::RefCell,
    net::SocketAddr,
    path::PathBuf,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicI64, AtomicUsize, Ordering},
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
    live::runner::replace_exec_event_sender,
    messages::{
        ExecutionEvent, ExecutionReport,
        execution::{
            BatchCancelOrders, CancelAllOrders, CancelOrder, GenerateFillReports,
            GeneratePositionStatusReports, ModifyOrder, SubmitOrder, SubmitOrderList,
        },
    },
    testing::wait_until_async,
};
use nautilus_core::{UUID4, UnixNanos};
use nautilus_lighter::{
    common::{
        consts::{LIGHTER_NAUTILUS_INTEGRATOR_ACCOUNT_INDEX, LIGHTER_VENUE},
        enums::LighterEnvironment,
    },
    config::LighterExecClientConfig,
    execution::LighterExecutionClient,
};
use nautilus_live::ExecutionClientCore;
use nautilus_model::{
    accounts::{AccountAny, MarginAccount},
    enums::{
        AccountType, OmsType, OrderSide, OrderStatus, OrderType, PositionSideSpecified,
        TimeInForce, TriggerType,
    },
    events::{AccountState, OrderAccepted, OrderEventAny, OrderPendingCancel},
    identifiers::{
        AccountId, ClientId, ClientOrderId, InstrumentId, OrderListId, StrategyId, Symbol,
        TraderId, VenueOrderId,
    },
    instruments::{CryptoPerpetual, CurrencyPair, InstrumentAny},
    orders::{
        LimitIfTouchedOrder, LimitOrder, MarketIfTouchedOrder, MarketOrder, Order, OrderAny,
        OrderList, StopLimitOrder, StopMarketOrder,
    },
    types::{AccountBalance, Currency, Money, Price, Quantity},
};
use rstest::rstest;
use serde_json::{Value, json};

const PRIVATE_KEY_HEX: &str =
    "0b8e0f63c24d8baacd9d29ad4e9a4b73c4a8d2bb8b16dc4fa9d7c2e1d3a8b1f0e8d3a4c5b6e7f001";
const TEST_ACCOUNT_INDEX: u64 = 12_345;
const TEST_API_KEY_INDEX: u8 = 5;
const ETH_PERP_SYMBOL: &str = "ETH-PERP";
const ETH_SPOT_SYMBOL: &str = "ETH/USDC-SPOT";
const TEST_MARKET_INDEX: i16 = 0;
const TEST_NEXT_NONCE: i64 = 9_999;
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
/// `send_txs`) and per-REST-endpoint call counts (`active_orders_calls`,
/// `inactive_orders_calls`, `trades_calls`). Tests inject venue responses
/// via the corresponding `*_response` and `next_send_tx_ack` overrides,
/// push synthetic frames through `inbox_tx` (consumed via
/// [`Self::push_frame`]), and arm a server-side close by toggling
/// `close_after_next_frame` so the WS layer's auto-reconnect path can be
/// exercised.
#[derive(Clone)]
struct TestServerState {
    connection_count: Arc<tokio::sync::Mutex<usize>>,
    subscribes: Arc<tokio::sync::Mutex<Vec<Value>>>,
    unsubscribes: Arc<tokio::sync::Mutex<Vec<Value>>>,
    send_txs: Arc<tokio::sync::Mutex<Vec<Value>>>,
    rest_send_txs: Arc<tokio::sync::Mutex<Vec<Value>>>,
    maker_only_calls: Arc<AtomicUsize>,
    maker_only_api_key_indexes: Arc<tokio::sync::Mutex<Vec<i64>>>,
    maker_only_authorizations: Arc<tokio::sync::Mutex<Vec<String>>>,
    active_orders_calls: Arc<AtomicUsize>,
    inactive_orders_calls: Arc<AtomicUsize>,
    trades_calls: Arc<AtomicUsize>,
    active_orders_response: Arc<tokio::sync::Mutex<Option<Value>>>,
    inactive_orders_response: Arc<tokio::sync::Mutex<Option<Value>>>,
    trades_response: Arc<tokio::sync::Mutex<Option<Value>>>,
    next_rest_send_tx_response: Arc<tokio::sync::Mutex<Option<Value>>>,
    next_send_tx_ack: Arc<tokio::sync::Mutex<Option<Value>>>,
    block_next_send_tx_batch_response: Arc<AtomicBool>,
    send_tx_batch_response_gate: Arc<tokio::sync::Notify>,
    inbox_tx: tokio::sync::broadcast::Sender<String>,
    close_after_next_frame: Arc<AtomicBool>,
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
            maker_only_calls: Arc::new(AtomicUsize::new(0)),
            maker_only_api_key_indexes: Arc::new(tokio::sync::Mutex::new(Vec::new())),
            maker_only_authorizations: Arc::new(tokio::sync::Mutex::new(Vec::new())),
            active_orders_calls: Arc::new(AtomicUsize::new(0)),
            inactive_orders_calls: Arc::new(AtomicUsize::new(0)),
            trades_calls: Arc::new(AtomicUsize::new(0)),
            active_orders_response: Arc::new(tokio::sync::Mutex::new(None)),
            inactive_orders_response: Arc::new(tokio::sync::Mutex::new(None)),
            trades_response: Arc::new(tokio::sync::Mutex::new(None)),
            next_rest_send_tx_response: Arc::new(tokio::sync::Mutex::new(None)),
            next_send_tx_ack: Arc::new(tokio::sync::Mutex::new(None)),
            block_next_send_tx_batch_response: Arc::new(AtomicBool::new(false)),
            send_tx_batch_response_gate: Arc::new(tokio::sync::Notify::new()),
            inbox_tx,
            close_after_next_frame: Arc::new(AtomicBool::new(false)),
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

    fn push_frame(&self, frame: &Value) {
        let _ = self.inbox_tx.send(frame.to_string());
    }

    fn block_next_send_tx_batch_response(&self) {
        self.block_next_send_tx_batch_response
            .store(true, Ordering::Release);
    }

    fn release_send_tx_batch_response(&self) {
        self.send_tx_batch_response_gate.notify_one();
    }
}

async fn order_book_details() -> Response {
    (StatusCode::OK, load_text("http_order_book_details.json")).into_response()
}

async fn account() -> Response {
    // Standard-tier account fixture; exercises tier detection on connect.
    (StatusCode::OK, load_text("http_account.json")).into_response()
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

async fn account_active_orders(
    State(state): State<Arc<TestServerState>>,
    Query(_query): Query<std::collections::HashMap<String, String>>,
) -> Response {
    state.active_orders_calls.fetch_add(1, Ordering::Relaxed);
    if let Some(body) = state.active_orders_response.lock().await.clone() {
        return (StatusCode::OK, body.to_string()).into_response();
    }
    (StatusCode::OK, json!({"code":200,"orders":[]}).to_string()).into_response()
}

async fn account_inactive_orders(
    State(state): State<Arc<TestServerState>>,
    Query(_query): Query<std::collections::HashMap<String, String>>,
) -> Response {
    state.inactive_orders_calls.fetch_add(1, Ordering::Relaxed);
    if let Some(body) = state.inactive_orders_response.lock().await.clone() {
        return (StatusCode::OK, body.to_string()).into_response();
    }
    (StatusCode::OK, json!({"code":200,"orders":[]}).to_string()).into_response()
}

async fn trades(
    State(state): State<Arc<TestServerState>>,
    Query(_query): Query<std::collections::HashMap<String, String>>,
) -> Response {
    state.trades_calls.fetch_add(1, Ordering::Relaxed);
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
        .route("/api/v1/accountActiveOrders", get(account_active_orders))
        .route(
            "/api/v1/accountInactiveOrders",
            get(account_inactive_orders),
        )
        .route("/api/v1/trades", get(trades))
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

    if state
        .block_next_send_tx_batch_response
        .swap(false, Ordering::AcqRel)
    {
        state.send_tx_batch_response_gate.notified().await;
    }

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
    // Let axum start accepting connections before tests dial in.
    tokio::time::sleep(Duration::from_millis(50)).await;
    (addr, state)
}

fn build_config(addr: SocketAddr) -> LighterExecClientConfig {
    // Pin every credential field explicitly so a stray `LIGHTER_*` env var
    // cannot leak into a test.
    LighterExecClientConfig {
        trader_id: trader_id(),
        account_id: account_id(),
        account_index: Some(TEST_ACCOUNT_INDEX),
        api_key_index: Some(TEST_API_KEY_INDEX),
        private_key: Some(PRIVATE_KEY_HEX.to_string()),
        base_url_http: Some(format!("http://{addr}")),
        base_url_ws: Some(format!("ws://{addr}/stream")),
        proxy_url: None,
        environment: LighterEnvironment::Testnet,
        http_timeout_secs: 5,
        ws_timeout_secs: 5,
        active_markets: Vec::new(),
        market_order_slippage_bps: 50,
        rest_quota_per_min: None,
        sendtx_quota_per_min: None,
        transport_backend: Default::default(),
    }
}

fn build_config_no_credentials(addr: SocketAddr) -> LighterExecClientConfig {
    LighterExecClientConfig {
        private_key: None,
        account_index: None,
        api_key_index: None,
        ..build_config(addr)
    }
}

fn test_perp_instrument() -> InstrumentAny {
    InstrumentAny::CryptoPerpetual(CryptoPerpetual::new(
        eth_perp_id(),
        Symbol::new(ETH_PERP_SYMBOL),
        Currency::from("ETH"),
        Currency::from("USDC"),
        Currency::from("USDC"),
        false,
        2,
        4,
        Price::from("0.01"),
        Quantity::from("0.0001"),
        None,
        None,
        None,
        None,
        None,
        Some(Money::from("10.000000 USDC")),
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        UnixNanos::default(),
        UnixNanos::default(),
    ))
}

fn test_spot_instrument() -> InstrumentAny {
    InstrumentAny::CurrencyPair(CurrencyPair::new(
        eth_spot_id(),
        Symbol::new("ETH/USDC"),
        Currency::from("ETH"),
        Currency::from("USDC"),
        4,
        2,
        Price::from("0.0001"),
        Quantity::from("0.01"),
        None,
        None,
        None,
        Some(Quantity::from("0.01")),
        None,
        Some(Money::from("1.0000 USDC")),
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        UnixNanos::default(),
        UnixNanos::default(),
    ))
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

fn build_client_with(
    config: LighterExecClientConfig,
) -> (
    LighterExecutionClient,
    tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
    Rc<RefCell<Cache>>,
) {
    let cache = build_cache_with_account_and_instrument();
    build_client_with_cache(config, cache)
}

fn build_client_with_cache(
    config: LighterExecClientConfig,
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

    let core = ExecutionClientCore::new(
        trader_id(),
        client_id(),
        *LIGHTER_VENUE,
        OmsType::Netting,
        account_id(),
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

async fn assert_local_order_denied_once(
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
    state: &TestServerState,
    reason_part: &str,
) {
    let event = next_order_event(rx, Duration::from_secs(2))
        .await
        .expect("expected denied event");
    match event {
        OrderEventAny::Denied(d) => assert!(
            d.reason.as_str().contains(reason_part),
            "expected reason containing `{reason_part}`, was {:?}",
            d.reason,
        ),
        other => panic!("expected OrderDenied, was {other:?}"),
    }

    assert!(
        next_order_event(rx, Duration::from_millis(100))
            .await
            .is_none(),
        "local denial should emit exactly one order event",
    );
    assert_eq!(state.send_txs().await.len(), 0);
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
    OrderAny::Limit(LimitOrder::new(
        trader_id(),
        strategy_id(),
        eth_perp_id(),
        ClientOrderId::from(id),
        side,
        qty,
        price,
        tif,
        None,
        post_only,
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

fn make_limit_order_with_quantity_options(
    id: &str,
    quote_quantity: bool,
    display_qty: Option<Quantity>,
) -> OrderAny {
    OrderAny::Limit(LimitOrder::new(
        trader_id(),
        strategy_id(),
        eth_perp_id(),
        ClientOrderId::from(id),
        OrderSide::Buy,
        Quantity::from("0.0050"),
        Price::from("2361.31"),
        TimeInForce::Gtc,
        None,
        false,
        false,
        quote_quantity,
        display_qty,
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

fn make_market_order(id: &str, side: OrderSide, qty: Quantity) -> OrderAny {
    OrderAny::Market(MarketOrder::new(
        trader_id(),
        strategy_id(),
        eth_perp_id(),
        ClientOrderId::from(id),
        side,
        qty,
        TimeInForce::Ioc,
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
    let price = Price::from("2401.00");

    match order_type {
        OrderType::StopMarket => OrderAny::StopMarket(StopMarketOrder::new(
            trader_id(),
            strategy_id(),
            instrument_id,
            ClientOrderId::from(id),
            side,
            qty,
            trigger,
            TriggerType::Default,
            tif,
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
        )),
        OrderType::StopLimit => OrderAny::StopLimit(StopLimitOrder::new(
            trader_id(),
            strategy_id(),
            instrument_id,
            ClientOrderId::from(id),
            side,
            qty,
            price,
            trigger,
            TriggerType::Default,
            tif,
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
        )),
        OrderType::MarketIfTouched => OrderAny::MarketIfTouched(MarketIfTouchedOrder::new(
            trader_id(),
            strategy_id(),
            instrument_id,
            ClientOrderId::from(id),
            side,
            qty,
            trigger,
            TriggerType::Default,
            tif,
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
            UUID4::new(),
            UnixNanos::default(),
        )),
        OrderType::LimitIfTouched => OrderAny::LimitIfTouched(LimitIfTouchedOrder::new(
            trader_id(),
            strategy_id(),
            instrument_id,
            ClientOrderId::from(id),
            side,
            qty,
            price,
            trigger,
            TriggerType::Default,
            tif,
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
        )),
        other => panic!("expected conditional order type, was {other:?}"),
    }
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

fn cache_pending_cancel_order(
    cache: &Rc<RefCell<Cache>>,
    order: OrderAny,
    venue_order_id: VenueOrderId,
) {
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

    let pending_cancel = OrderEventAny::PendingCancel(OrderPendingCancel::new(
        trader_id(),
        strategy_id(),
        instrument_id,
        client_order_id,
        account_id(),
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

fn send_tx_batch_types(send_tx_batch: &Value) -> Vec<u8> {
    send_tx_batch
        .get("data")
        .and_then(|d| d.get("tx_types"))
        .and_then(Value::as_array)
        .expect("missing tx_types")
        .iter()
        .map(|value| value.as_u64().expect("tx_type value") as u8)
        .collect()
}

fn send_tx_batch_infos(send_tx_batch: &Value) -> Vec<Value> {
    send_tx_batch
        .get("data")
        .and_then(|d| d.get("tx_infos"))
        .and_then(Value::as_array)
        .expect("missing tx_infos")
        .iter()
        .map(|inner| match inner {
            Value::String(s) => serde_json::from_str(s).expect("tx_info string is invalid json"),
            other => other.clone(),
        })
        .collect()
}

fn assert_send_tx_batch_infos_are_strings(send_tx_batch: &Value) {
    let infos = send_tx_batch
        .get("data")
        .and_then(|d| d.get("tx_infos"))
        .and_then(Value::as_array)
        .expect("missing tx_infos");
    assert!(
        infos.iter().all(Value::is_string),
        "sendTxBatch tx_infos must be a JSON array of signed tx_info strings",
    );
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

        // Race connect against the first four frames; connect must stay
        // pending after each push since the strict-await gate requires all
        // five streams to land.
        let push_four = {
            let state = Arc::clone(&state);
            let frames = [
                orders_frame.clone(),
                trades_frame.clone(),
                positions_frame.clone(),
                assets_frame.clone(),
            ];
            async move {
                await_subscribe_count(&state, 5).await;
                for frame in frames {
                    state.push_frame(&frame);
                    tokio::time::sleep(Duration::from_millis(80)).await;
                }
                // Settle so a buggy implementation that unblocks early has
                // time to surface here.
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
        };

        tokio::select! {
            result = &mut connect_fut => {
                panic!("connect returned with fewer than five account frames: {result:?}");
            }
            () = push_four => {}
        }

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
async fn connect_submits_l2_only_integrator_auto_approval() {
    let (addr, state) = start_server().await;
    let (mut client, _rx, _cache) = build_client(addr);

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

    client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn connect_skips_integrator_auto_approval_for_maker_only_api_key() {
    let (addr, state) = start_server().await;
    state
        .maker_only_api_key_indexes
        .lock()
        .await
        .push(i64::from(TEST_API_KEY_INDEX));
    let (mut client, _rx, _cache) = build_client(addr);

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
    *state.next_rest_send_tx_response.lock().await = Some(json!({
        "code": 21149,
        "message": "integrator is not approved",
    }));
    let (mut client, _rx, _cache) = build_client(addr);

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

        let push_first_four = {
            let state = Arc::clone(&state);
            async move {
                await_subscribe_count(&state, 5).await;
                for frame in first_four {
                    state.push_frame(&frame);
                    tokio::time::sleep(Duration::from_millis(80)).await;
                }
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
        };

        tokio::select! {
            result = &mut connect_fut => {
                panic!(
                    "connect returned before {last_stream} frame landed: {result:?}",
                );
            }
            () = push_first_four => {}
        }

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

/// Snapshots and clears the Lighter credential env vars, restoring the
/// originals on drop. Tests that exercise `Credential::resolve`'s
/// fall-back-to-env path must serialise on the workspace `serial_tests`
/// nextest group; see [`crate::common::credential::credential_env_vars`]
/// for the full list.
struct EnvGuard {
    saved: Vec<(&'static str, Option<String>)>,
}

const LIGHTER_ENV_VARS: &[&str] = &[
    "LIGHTER_API_KEY_INDEX",
    "LIGHTER_API_SECRET",
    "LIGHTER_ACCOUNT_INDEX",
    "LIGHTER_TESTNET_API_KEY_INDEX",
    "LIGHTER_TESTNET_API_SECRET",
    "LIGHTER_TESTNET_ACCOUNT_INDEX",
];

impl EnvGuard {
    fn clear_lighter() -> Self {
        let saved = LIGHTER_ENV_VARS
            .iter()
            .map(|&name| (name, std::env::var(name).ok()))
            .collect::<Vec<_>>();
        for &(name, _) in &saved {
            // SAFETY: tests in the `serial_tests` module run under the
            // workspace `serial-tests` nextest group, which serialises
            // them. No other Lighter test reads or writes these vars
            // concurrently.
            unsafe { std::env::remove_var(name) };
        }
        Self { saved }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        for (name, original) in &self.saved {
            match original {
                Some(value) => unsafe { std::env::set_var(name, value) },
                None => unsafe { std::env::remove_var(name) },
            }
        }
    }
}

// Tests in this module mutate process-global LIGHTER_* env vars while
// exercising the fail-fast credential path. The nextest filter
// `test(serial_tests)` (see `.config/nextest.toml`) pins them to the
// `serial-tests` group so they cannot race other tests that also read or
// write LIGHTER_* state.
mod serial_tests {
    use super::*;

    #[rstest]
    #[tokio::test(flavor = "multi_thread")]
    async fn test_connect_without_credentials_fails_fast() {
        let _guard = EnvGuard::clear_lighter();
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
    async fn test_submit_order_without_credentials_errors_synchronously() {
        let _guard = EnvGuard::clear_lighter();
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
#[tokio::test(flavor = "multi_thread")]
async fn test_submit_limit_order_emits_submitted_and_signs_sendtx() {
    let (addr, state) = start_server().await;
    let (mut client, mut rx, cache) = build_client(addr);
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

    client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn test_submit_order_list_sends_one_create_order_batch() {
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

    await_send_tx_count(&state, 1).await;
    let frames = state.send_txs().await;
    assert_eq!(frames.len(), 1, "single sendTxBatch expected");
    assert_eq!(frames[0]["type"], "jsonapi/sendtxbatch");
    assert_eq!(send_tx_batch_types(&frames[0]), vec![14, 14]);
    assert_send_tx_batch_infos_are_strings(&frames[0]);

    let infos = send_tx_batch_infos(&frames[0]);
    assert_eq!(infos.len(), 2);
    assert_eq!(infos[0]["MarketIndex"], TEST_MARKET_INDEX);
    assert_eq!(infos[0]["IsAsk"], 0);
    assert_eq!(infos[1]["MarketIndex"], TEST_MARKET_INDEX);
    assert_eq!(infos[1]["IsAsk"], 1);
    assert_eq!(infos[1]["TimeInForce"], 2);
    assert!(
        next_order_event(&mut rx, Duration::from_millis(100))
            .await
            .is_none(),
        "sendTxBatch success is not a per-order terminal outcome",
    );

    client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn test_http_batch_response_blocks_later_ws_sendtx() {
    let (addr, state) = start_server().await;
    state.block_next_send_tx_batch_response();
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

    await_send_tx_count(&state, 1).await;

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

    tokio::time::sleep(Duration::from_millis(100)).await;
    let frames = state.send_txs().await;
    assert_eq!(
        frames.len(),
        1,
        "later WS sendTx must wait for batch response"
    );
    assert_eq!(frames[0]["type"], "jsonapi/sendtxbatch");

    state.release_send_tx_batch_response();
    await_send_tx_count(&state, 2).await;

    let frames = state.send_txs().await;
    assert_eq!(frames.len(), 2);
    assert_eq!(frames[0]["type"], "jsonapi/sendtxbatch");
    assert_eq!(send_tx_batch_types(&frames[0]), vec![14, 14]);
    assert_eq!(frames[1]["type"], "jsonapi/sendtx");
    assert_eq!(send_tx_type(&frames[1]), 14);

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
    assert_local_order_denied_once(&mut rx, &state, "no cached quote").await;

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
    assert_local_order_denied_once(&mut rx, &state, "fill-or-kill").await;
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
            assert!(
                reason.contains("insufficient margin"),
                "rejection reason should include the venue message, was `{reason}`",
            );
            assert!(
                reason.contains("21029"),
                "rejection reason should include the venue code, was `{reason}`",
            );
        }
        other => panic!("expected OrderRejected, was {other:?}"),
    }

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
            assert!(
                reason.contains("code=21727"),
                "rejection reason should include the venue code, was `{reason}`",
            );
            assert!(
                reason.contains("order is not cancelable"),
                "rejection reason should include the venue message, was `{reason}`",
            );
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
#[tokio::test(flavor = "multi_thread")]
async fn test_modify_order_signs_modify_sendtx() {
    let (addr, state) = start_server().await;
    let (mut client, _rx, cache) = build_client(addr);
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
            assert!(
                reason.contains("code=21702"),
                "rejection reason should include the venue code, was `{reason}`",
            );
            assert!(
                reason.contains("modify rejected by venue"),
                "rejection reason should include the venue message, was `{reason}`",
            );
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
/// helper so the two stay in sync.
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
    // adapter signed in the sendTx — not the Nautilus cloid label.
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
        "nonce": 100,
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
        OrderSide::NoOrderSide,
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
        OrderSide::NoOrderSide,
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
async fn test_batch_cancel_orders_sends_one_cancel_order_batch() {
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
    await_send_tx_count(&state, 1).await;
    let frames = state.send_txs().await;
    assert_eq!(frames.len(), 1);
    assert_eq!(frames[0]["type"], "jsonapi/sendtxbatch");
    assert_eq!(send_tx_batch_types(&frames[0]), vec![15, 15, 15]);
    assert_send_tx_batch_infos_are_strings(&frames[0]);
    let infos = send_tx_batch_infos(&frames[0]);
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
        "sendTxBatch success must wait for account stream cancel outcomes",
    );
    client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn test_reconnect_replays_authenticated_account_subscriptions() {
    // The WS layer auto-reconnects on a server-initiated close. After the
    // reconnect the 5 account-stream subscribes must replay with their
    // auth token; otherwise the typed execution stream would silently
    // drop. The data-client variant of this test pins the public-channel
    // replay; this pins the authenticated path.
    let (addr, state) = start_server().await;
    let (mut client, _rx, cache) = build_client(addr);
    client.connect().await.expect("connect");
    await_subscribe_count(&state, 5).await;

    // Arm the server-side close. The next inbound frame from the client
    // closes the socket; we then send a no-op cancel to fire that frame.
    // Reconnect drives a full replay of the 5 tracked subscriptions.
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
                        >= 2
                })
            }
        },
        Duration::from_secs(10),
    )
    .await;

    // Sanity-check that every replayed subscribe still carries auth.
    let subs = state.subscribes().await;
    for sub in &subs {
        let channel = sub["channel"].as_str().unwrap_or("");
        if channel.starts_with("account_all_") || channel.starts_with("user_stats") {
            assert!(
                sub.get("auth").and_then(Value::as_str).is_some(),
                "account-stream subscribe missing auth: {sub:?}",
            );
        }
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

    // Drive `active_markets` so the fan-out actually hits the active /
    // inactive endpoints. The consumption loop notes a market whenever an
    // account_all_* frame mentions it; the position fixture exists in
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
async fn test_generate_mass_status_uses_configured_active_markets_on_cold_start() {
    let (addr, state) = start_server().await;
    let mut config = build_config(addr);
    config.active_markets = vec![TEST_MARKET_INDEX];
    let (mut client, _rx, _cache) = build_client_with(config);
    client.connect().await.expect("connect");
    await_subscribe_count(&state, 4).await;

    *state.active_orders_response.lock().await = Some(http_orders_payload(
        &[http_order_fixture(
            "281476929510200",
            "1001",
            "open",
            "0.0000",
        )],
        None,
    ));
    *state.trades_response.lock().await = Some(json!({"code":200,"trades":[]}));

    let mass = client
        .generate_mass_status(None)
        .await
        .expect("mass status")
        .expect("Some(mass_status)");

    assert_eq!(
        state.active_orders_calls.load(Ordering::Relaxed),
        1,
        "configured active market should drive one active-orders fetch",
    );
    assert_eq!(
        state.inactive_orders_calls.load(Ordering::Relaxed),
        1,
        "configured active market should skip inactive seeding and run one per-market fetch",
    );

    let order_reports = mass.order_reports();
    assert!(
        order_reports
            .values()
            .any(|r| r.order_status == OrderStatus::Accepted),
        "configured active market should surface open orders in mass status: {order_reports:?}",
    );

    client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn test_generate_mass_status_seeds_active_markets_from_inactive_orders() {
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
async fn test_account_all_positions_empty_snapshot_clears_cache_and_emits_flat_report() {
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

    // Push an empty positions snapshot. The dispatcher must treat it as
    // authoritative and flatten the prior cached position.
    state.push_frame(&json!({
        "type": "update/account_all_positions",
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
                    && report.position_side == PositionSideSpecified::Flat
                    && report.quantity.is_zero()
        )
    })
    .await
    .expect("flat position report");

    let ExecutionEvent::Report(ExecutionReport::Position(flat_report)) = flat_report else {
        unreachable!("predicate only accepts position reports");
    };
    assert_eq!(flat_report.instrument_id, eth_perp_id());
    assert_eq!(flat_report.position_side, PositionSideSpecified::Flat);
    assert!(flat_report.quantity.is_zero());

    // The empty snapshot also clears the cached position used by status reports.
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
        "empty position snapshot must clear the prior cache, was {positions:?}",
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
                    && report.position_side == PositionSideSpecified::Flat
                    && report.quantity.is_zero()
        )
    })
    .await;

    assert!(
        unexpected_flat.is_none(),
        "invalid position row must not flatten cached positions: {unexpected_flat:?}",
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
    assert_eq!(positions[0].instrument_id, eth_perp_id());
    assert_eq!(positions[0].quantity, Quantity::from("1.5000"));

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
        "type": "update/account_all_positions",
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
                    && report.position_side == PositionSideSpecified::Flat
                    && report.quantity.is_zero()
        )
    })
    .await
    .expect("flat position report after reconnect");

    let ExecutionEvent::Report(ExecutionReport::Position(flat_report)) = flat_report else {
        unreachable!("predicate only accepts position reports");
    };
    assert_eq!(flat_report.instrument_id, eth_perp_id());
    assert_eq!(flat_report.position_side, PositionSideSpecified::Flat);
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
