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

//! Integration tests for the Lighter WebSocket client using a mock Axum server.
//!
//! The harness mirrors the OKX / Bybit shape: a `TestServerState` records every
//! inbound message from the client, a `handle_socket` task replies with venue
//! acks and pre-arranged update frames, and each test drives the public
//! [`LighterWebSocketClient`] surface and asserts on the resulting
//! [`NautilusWsMessage`] stream and the recorded server-side state.

use std::{
    net::SocketAddr,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
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
use futures_util::{SinkExt, StreamExt};
use nautilus_common::testing::wait_until_async;
use nautilus_core::UnixNanos;
use nautilus_lighter::{
    common::{
        enums::{LighterCandleResolution, LighterEnvironment, LighterProductType, LighterTxType},
        symbol::MarketRegistry,
    },
    websocket::{
        NautilusWsMessage,
        client::LighterWebSocketClient,
        messages::{LighterMarketSelection, LighterWsChannel},
    },
};
use nautilus_model::{
    enums::{BookAction, RecordFlag},
    identifiers::{InstrumentId, Symbol},
    instruments::{CryptoPerpetual, CurrencyPair, InstrumentAny},
    types::{Currency, Price, Quantity},
};
use nautilus_network::websocket::TransportBackend;
use serde_json::{Value, json};

const PERP_MARKET_INDEX: i16 = 0;
const PERP_VENUE_SYMBOL: &str = "ETH";
const SECOND_MARKET_INDEX: i16 = 1;
const SECOND_VENUE_SYMBOL: &str = "BTC";
const SPOT_MARKET_INDEX: i16 = 2048;
const SPOT_VENUE_SYMBOL: &str = "ETH";

fn data_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test_data")
}

fn load_json(filename: &str) -> Value {
    let content = std::fs::read_to_string(data_path().join(filename))
        .unwrap_or_else(|_| panic!("failed to read {filename}"));
    serde_json::from_str(&content).expect("invalid json")
}

fn perp_instrument(
    market_index: i16,
    venue_symbol: &str,
    registry: &MarketRegistry,
) -> InstrumentAny {
    let instrument_id = registry.insert(market_index, venue_symbol, LighterProductType::Perp);
    InstrumentAny::CryptoPerpetual(CryptoPerpetual::new(
        instrument_id,
        Symbol::new(format!("{venue_symbol}-PERP")),
        Currency::from(venue_symbol),
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
        None,
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

fn spot_instrument(
    market_index: i16,
    venue_symbol: &str,
    registry: &MarketRegistry,
) -> InstrumentAny {
    let instrument_id = registry.insert(market_index, venue_symbol, LighterProductType::Spot);
    InstrumentAny::CurrencyPair(CurrencyPair::new(
        instrument_id,
        Symbol::new(format!("{venue_symbol}-SPOT")),
        Currency::from(venue_symbol),
        Currency::from("USDC"),
        2,
        4,
        Price::from("0.01"),
        Quantity::from("0.0001"),
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
        None,
        None,
        UnixNanos::default(),
        UnixNanos::default(),
    ))
}

#[derive(Clone, Default)]
struct TestServerState {
    connection_count: Arc<tokio::sync::Mutex<usize>>,
    subscribes: Arc<tokio::sync::Mutex<Vec<Value>>>,
    unsubscribes: Arc<tokio::sync::Mutex<Vec<Value>>>,
    send_txs: Arc<tokio::sync::Mutex<Vec<Value>>>,
    /// Optional callback queue sending raw text frames to push to the client
    /// after each handled subscribe. Drained in handler order.
    push_after_subscribe: Arc<tokio::sync::Mutex<Vec<String>>>,
    /// When set, the server closes the socket after sending the next subscribe ack.
    drop_after_next_subscribe: Arc<AtomicBool>,
}

impl TestServerState {
    async fn subscribes(&self) -> Vec<Value> {
        self.subscribes.lock().await.clone()
    }

    async fn unsubscribes(&self) -> Vec<Value> {
        self.unsubscribes.lock().await.clone()
    }

    async fn send_txs(&self) -> Vec<Value> {
        self.send_txs.lock().await.clone()
    }

    async fn enqueue_push(&self, frame: Value) {
        self.push_after_subscribe
            .lock()
            .await
            .push(frame.to_string());
    }

    async fn pop_push(&self) -> Option<String> {
        let mut q = self.push_after_subscribe.lock().await;
        if q.is_empty() {
            None
        } else {
            Some(q.remove(0))
        }
    }
}

async fn handle_ws_upgrade(
    ws: WebSocketUpgrade,
    State(state): State<Arc<TestServerState>>,
) -> Response {
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

async fn handle_socket(socket: WebSocket, state: Arc<TestServerState>) {
    {
        let mut count = state.connection_count.lock().await;
        *count += 1;
    }

    // Send the initial Lighter handshake frame so any future control-frame
    // tests have a representative server greeting.
    let (mut sink, mut stream) = socket.split();
    let _ = sink
        .send(Message::Text(
            json!({"type":"connected"}).to_string().into(),
        ))
        .await;

    while let Some(message) = stream.next().await {
        let Ok(message) = message else { break };
        match message {
            Message::Text(text) => {
                let Ok(value) = serde_json::from_str::<Value>(&text) else {
                    continue;
                };
                let kind = value.get("type").and_then(Value::as_str).unwrap_or("");
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

                        // Drain any pending push frame queued by the test.
                        if let Some(payload) = state.pop_push().await
                            && sink.send(Message::Text(payload.into())).await.is_err()
                        {
                            break;
                        }

                        if state
                            .drop_after_next_subscribe
                            .swap(false, Ordering::Relaxed)
                        {
                            let _ = sink.send(Message::Close(None)).await;
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
                    }
                    _ => {}
                }
            }
            Message::Ping(payload) if sink.send(Message::Pong(payload.clone())).await.is_err() => {
                break;
            }
            Message::Close(_) => break,
            _ => {}
        }
    }

    let mut count = state.connection_count.lock().await;
    *count = count.saturating_sub(1);
}

async fn start_ws_server(state: Arc<TestServerState>) -> SocketAddr {
    let router = Router::new()
        .route("/stream", get(handle_ws_upgrade))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind ws listener");
    let addr = listener.local_addr().expect("local addr");
    tokio::spawn(async move {
        axum::serve(listener, router).await.expect("ws server");
    });
    tokio::time::sleep(Duration::from_millis(50)).await;
    addr
}

struct ClientHarness {
    client: LighterWebSocketClient,
    registry: Arc<MarketRegistry>,
}

impl ClientHarness {
    async fn build(addr: SocketAddr) -> Self {
        let registry = Arc::new(MarketRegistry::new());
        let perp = perp_instrument(PERP_MARKET_INDEX, PERP_VENUE_SYMBOL, &registry);
        let second = perp_instrument(SECOND_MARKET_INDEX, SECOND_VENUE_SYMBOL, &registry);
        let spot = spot_instrument(SPOT_MARKET_INDEX, SPOT_VENUE_SYMBOL, &registry);

        let mut client = LighterWebSocketClient::new(
            Some(format!("ws://{addr}/stream")),
            LighterEnvironment::Testnet,
            Arc::clone(&registry),
            TransportBackend::default(),
            None,
        );
        client.cache_instruments(vec![
            (PERP_MARKET_INDEX, perp),
            (SECOND_MARKET_INDEX, second),
            (SPOT_MARKET_INDEX, spot),
        ]);
        client.connect().await.expect("connect");

        Self { client, registry }
    }

    fn instrument(&self, market_index: i16) -> InstrumentId {
        self.registry
            .instrument_id(market_index)
            .expect("registered")
    }
}

async fn next_event_within(
    client: &mut LighterWebSocketClient,
    timeout: Duration,
) -> Option<NautilusWsMessage> {
    tokio::time::timeout(timeout, client.next_event())
        .await
        .ok()
        .flatten()
}

async fn await_subscribe_count(state: &TestServerState, target: usize) {
    wait_until_async(
        || {
            let state = state.clone();
            async move { state.subscribes.lock().await.len() >= target }
        },
        Duration::from_secs(2),
    )
    .await;
}

async fn await_unsubscribe_count(state: &TestServerState, target: usize) {
    wait_until_async(
        || {
            let state = state.clone();
            async move { state.unsubscribes.lock().await.len() >= target }
        },
        Duration::from_secs(2),
    )
    .await;
}

async fn await_send_tx_count(state: &TestServerState, target: usize) {
    wait_until_async(
        || {
            let state = state.clone();
            async move { state.send_txs.lock().await.len() >= target }
        },
        Duration::from_secs(2),
    )
    .await;
}

async fn await_subscription_count(client: &LighterWebSocketClient, target: usize) {
    let started = std::time::Instant::now();
    while client.subscription_count() < target && started.elapsed() < Duration::from_secs(2) {
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

/// Returns a clone of the order_book fixture rewritten to target a specific
/// `market_index`.
fn book_snapshot_frame_for_market(market_index: i16) -> Value {
    let mut frame = load_json("ws_order_book_subscribed.json");
    frame["channel"] = json!(format!("order_book:{market_index}"));
    frame
}

/// Returns a clone of the incremental order_book fixture rewritten to target
/// a specific `market_index`.
fn book_update_frame_for_market(market_index: i16) -> Value {
    let mut frame = load_json("ws_order_book_update.json");
    frame["channel"] = json!(format!("order_book:{market_index}"));
    frame
}

#[tokio::test]
async fn test_websocket_connection_lifecycle() {
    let state = Arc::new(TestServerState::default());
    let addr = start_ws_server(state.clone()).await;

    let mut harness = ClientHarness::build(addr).await;
    wait_until_async(
        || {
            let state = state.clone();
            async move { *state.connection_count.lock().await == 1 }
        },
        Duration::from_secs(2),
    )
    .await;
    assert!(harness.client.is_active());

    harness.client.disconnect().await.expect("disconnect");
    wait_until_async(
        || {
            let state = state.clone();
            async move { *state.connection_count.lock().await == 0 }
        },
        Duration::from_secs(2),
    )
    .await;
}

#[tokio::test]
async fn test_subscribe_book_sends_outbound_slash_payload() {
    let state = Arc::new(TestServerState::default());
    let addr = start_ws_server(state.clone()).await;
    let mut harness = ClientHarness::build(addr).await;

    assert_eq!(harness.client.subscription_count(), 0, "initial state");

    let id = harness.instrument(PERP_MARKET_INDEX);
    harness
        .client
        .subscribe_book(id)
        .await
        .expect("subscribe_book");

    await_subscribe_count(&state, 1).await;
    let subs = state.subscribes().await;
    assert_eq!(subs[0]["type"], "subscribe");
    assert_eq!(subs[0]["channel"], "order_book/0");
    assert!(subs[0].get("auth").is_none());

    // The harness sends the `subscribed` ack synchronously; pin the
    // SubscriptionState transition to confirmed.
    await_subscription_count(&harness.client, 1).await;
    assert_eq!(harness.client.subscription_count(), 1);

    harness.client.disconnect().await.expect("disconnect");
}

#[tokio::test]
async fn test_unsubscribe_book_sends_outbound_payload() {
    let state = Arc::new(TestServerState::default());
    let addr = start_ws_server(state.clone()).await;
    let mut harness = ClientHarness::build(addr).await;

    let id = harness.instrument(PERP_MARKET_INDEX);
    harness.client.subscribe_book(id).await.expect("subscribe");
    await_subscribe_count(&state, 1).await;

    harness
        .client
        .unsubscribe_book(id)
        .await
        .expect("unsubscribe");
    await_unsubscribe_count(&state, 1).await;

    let unsubs = state.unsubscribes().await;
    assert_eq!(unsubs[0]["type"], "unsubscribe");
    assert_eq!(unsubs[0]["channel"], "order_book/0");

    harness.client.disconnect().await.expect("disconnect");
}

#[tokio::test]
async fn test_subscribe_candles_sends_outbound_slash_payload() {
    let state = Arc::new(TestServerState::default());
    let addr = start_ws_server(state.clone()).await;
    let mut harness = ClientHarness::build(addr).await;

    let id = harness.instrument(PERP_MARKET_INDEX);
    harness
        .client
        .subscribe_candles(id, LighterCandleResolution::OneMinute)
        .await
        .expect("subscribe_candles");

    await_subscribe_count(&state, 1).await;
    let subs = state.subscribes().await;
    assert_eq!(subs[0]["type"], "subscribe");
    assert_eq!(subs[0]["channel"], "candle/0/1m");
    assert!(subs[0].get("auth").is_none());

    harness.client.disconnect().await.expect("disconnect");
}

#[tokio::test]
async fn test_unsubscribe_candles_sends_outbound_payload() {
    let state = Arc::new(TestServerState::default());
    let addr = start_ws_server(state.clone()).await;
    let mut harness = ClientHarness::build(addr).await;

    let id = harness.instrument(PERP_MARKET_INDEX);
    harness
        .client
        .subscribe_candles(id, LighterCandleResolution::OneMinute)
        .await
        .expect("subscribe");
    await_subscribe_count(&state, 1).await;

    harness
        .client
        .unsubscribe_candles(id, LighterCandleResolution::OneMinute)
        .await
        .expect("unsubscribe");
    await_unsubscribe_count(&state, 1).await;

    let unsubs = state.unsubscribes().await;
    assert_eq!(unsubs[0]["type"], "unsubscribe");
    assert_eq!(unsubs[0]["channel"], "candle/0/1m");

    harness.client.disconnect().await.expect("disconnect");
}

#[tokio::test]
async fn test_subscribe_candles_rejects_one_week_before_queueing() {
    let state = Arc::new(TestServerState::default());
    let addr = start_ws_server(state.clone()).await;
    let mut harness = ClientHarness::build(addr).await;

    let id = harness.instrument(PERP_MARKET_INDEX);
    let err = harness
        .client
        .subscribe_candles(id, LighterCandleResolution::OneWeek)
        .await
        .unwrap_err();

    assert!(
        err.to_string()
            .contains("not offered on the Lighter candle WebSocket stream"),
        "expected WS-streamable rejection, was: {err}",
    );
    // No outbound payload should have been queued.
    assert!(state.subscribes().await.is_empty());

    harness.client.disconnect().await.expect("disconnect");
}

#[tokio::test]
async fn test_send_tx_sends_outbound_payload() {
    let state = Arc::new(TestServerState::default());
    let addr = start_ws_server(state.clone()).await;
    let mut harness = ClientHarness::build(addr).await;

    let tx_info_str = r#"{"example":"signed"}"#.to_string();
    let raw = serde_json::value::RawValue::from_string(tx_info_str.clone()).expect("valid JSON");
    harness
        .client
        .send_tx(LighterTxType::CreateOrder as u8, raw)
        .await
        .expect("send_tx");

    await_send_tx_count(&state, 1).await;
    let send_txs = state.send_txs().await;
    assert_eq!(send_txs.len(), 1, "sendTx must dispatch exactly once");
    assert_eq!(send_txs[0]["type"], "jsonapi/sendtx");
    assert_eq!(
        send_txs[0]["data"]["tx_type"],
        LighterTxType::CreateOrder as u8,
    );
    let expected: Value = serde_json::from_str(&tx_info_str).expect("parse expected");
    assert_eq!(send_txs[0]["data"]["tx_info"], expected);

    harness.client.disconnect().await.expect("disconnect");
}

#[tokio::test]
async fn test_send_tx_errors_when_handler_unavailable() {
    let registry = Arc::new(MarketRegistry::new());
    let client = LighterWebSocketClient::new(
        Some("ws://127.0.0.1:9/stream".to_string()),
        LighterEnvironment::Testnet,
        registry,
        TransportBackend::default(),
        None,
    );

    let raw = serde_json::value::RawValue::from_string("{}".to_string()).expect("valid JSON");
    let err = client
        .send_tx(LighterTxType::CreateOrder as u8, raw)
        .await
        .expect_err("send_tx should fail before connect");
    assert!(err.to_string().contains("handler unavailable"));
}

#[tokio::test]
async fn test_order_book_update_before_snapshot_is_dropped() {
    let state = Arc::new(TestServerState::default());
    let addr = start_ws_server(state.clone()).await;
    let mut harness = ClientHarness::build(addr).await;

    state
        .enqueue_push(load_json("ws_order_book_update.json"))
        .await;

    let id = harness.instrument(PERP_MARKET_INDEX);
    harness.client.subscribe_book(id).await.expect("subscribe");

    assert!(
        next_event_within(&mut harness.client, Duration::from_millis(300))
            .await
            .is_none(),
        "pre-snapshot update/order_book must not seed or clear the book",
    );

    state
        .enqueue_push(load_json("ws_order_book_subscribed.json"))
        .await;
    harness
        .client
        .subscribe_quotes(id)
        .await
        .expect("trigger snapshot push");

    let event = next_event_within(&mut harness.client, Duration::from_secs(2))
        .await
        .expect("expected snapshot after dropped update");
    let NautilusWsMessage::Deltas(deltas) = event else {
        panic!("expected Deltas, was {event:?}");
    };
    assert_eq!(deltas.instrument_id, id);
    let first = deltas.deltas.first().expect("at least one delta");
    assert_eq!(first.action, BookAction::Clear);
    assert!(
        deltas
            .deltas
            .iter()
            .any(|d| d.flags & RecordFlag::F_SNAPSHOT as u8 != 0),
        "later subscribed/order_book must still seed the book",
    );

    harness.client.disconnect().await.expect("disconnect");
}

#[tokio::test]
async fn test_order_book_second_frame_is_incremental_no_depth10() {
    let state = Arc::new(TestServerState::default());
    let addr = start_ws_server(state.clone()).await;
    let mut harness = ClientHarness::build(addr).await;

    // First subscribe drains one push (the snapshot frame); the second push
    // is delivered after the second subscribe ack. In production Lighter
    // sends incrementals on the same stream, so simulate by re-subscribing
    // is not realistic. Instead, queue snapshot first and then push an
    // incremental directly through a fresh subscribe-ack path below.
    state
        .enqueue_push(load_json("ws_order_book_subscribed.json"))
        .await;

    let id = harness.instrument(PERP_MARKET_INDEX);
    harness
        .client
        .subscribe_book(id)
        .await
        .expect("subscribe 1");
    let snapshot_event = next_event_within(&mut harness.client, Duration::from_secs(2))
        .await
        .expect("snapshot delta");
    assert!(matches!(snapshot_event, NautilusWsMessage::Deltas(_)));

    // Push a second order_book frame directly via a unsubscribe/subscribe
    // cycle so the test harness can deliver another payload. The handler
    // tracks book_snapshots_seen per market_index across the connection,
    // so the second frame parses as incremental even when delivered through
    // a fresh push.
    state
        .enqueue_push(load_json("ws_order_book_update.json"))
        .await;
    harness
        .client
        .subscribe_quotes(id)
        .await
        .expect("trigger second push");

    let second = next_event_within(&mut harness.client, Duration::from_secs(2))
        .await
        .expect("incremental delta");

    let NautilusWsMessage::Deltas(deltas) = second else {
        panic!("expected Deltas, was {second:?}");
    };
    assert!(
        !deltas
            .deltas
            .iter()
            .any(|d| d.flags & RecordFlag::F_SNAPSHOT as u8 != 0),
        "second frame must not carry F_SNAPSHOT",
    );

    // Depth10 is not subscribed, so no Depth10 message should follow.
    let next = next_event_within(&mut harness.client, Duration::from_millis(200)).await;
    assert!(
        !matches!(next, Some(NautilusWsMessage::Depth10(_))),
        "no Depth10 expected without subscribe_book_depth10",
    );

    harness.client.disconnect().await.expect("disconnect");
}

#[tokio::test]
async fn test_order_book_depth10_emits_on_snapshot() {
    let state = Arc::new(TestServerState::default());
    let addr = start_ws_server(state.clone()).await;
    let mut harness = ClientHarness::build(addr).await;

    state
        .enqueue_push(load_json("ws_order_book_subscribed.json"))
        .await;

    let id = harness.instrument(PERP_MARKET_INDEX);
    harness
        .client
        .subscribe_book_depth10(id)
        .await
        .expect("subscribe_book_depth10");

    // Drain emitted events until the snapshot Deltas + Depth10 pair appears.
    let mut saw_deltas = false;
    let mut saw_depth10 = false;

    for _ in 0..4 {
        let Some(event) = next_event_within(&mut harness.client, Duration::from_secs(2)).await
        else {
            break;
        };

        match event {
            NautilusWsMessage::Deltas(_) => saw_deltas = true,
            NautilusWsMessage::Depth10(_) => saw_depth10 = true,
            _ => {}
        }

        if saw_deltas && saw_depth10 {
            break;
        }
    }
    assert!(saw_deltas, "expected Deltas on snapshot");
    assert!(saw_depth10, "expected Depth10 on snapshot");

    harness.client.disconnect().await.expect("disconnect");
}

#[tokio::test]
async fn test_trade_frame_includes_liquidation_trades() {
    let state = Arc::new(TestServerState::default());
    let addr = start_ws_server(state.clone()).await;
    let mut harness = ClientHarness::build(addr).await;

    let mut frame = load_json("ws_trade_update.json");
    let trades_clone = frame["trades"].as_array().cloned().unwrap_or_default();
    frame["liquidation_trades"] = Value::Array(trades_clone);
    state.enqueue_push(frame).await;

    let id = harness.instrument(PERP_MARKET_INDEX);
    harness
        .client
        .subscribe_trades(id)
        .await
        .expect("subscribe_trades");

    let event = next_event_within(&mut harness.client, Duration::from_secs(2))
        .await
        .expect("trades event");
    let NautilusWsMessage::Trades(ticks) = event else {
        panic!("expected Trades, was {event:?}");
    };
    assert_eq!(ticks.len(), 2, "must merge `trades` + `liquidation_trades`");
    assert!(ticks.iter().all(|t| t.instrument_id == id));

    harness.client.disconnect().await.expect("disconnect");
}

#[tokio::test]
async fn test_ticker_frame_resolves_via_channel_index() {
    let state = Arc::new(TestServerState::default());
    let addr = start_ws_server(state.clone()).await;
    let mut harness = ClientHarness::build(addr).await;

    // Use the existing fixture but rewrite `s` to a symbol that does NOT
    // match any cached raw_symbol — verifies the handler resolves from the
    // channel field, not the payload symbol field.
    let mut frame = load_json("ws_ticker_update.json");
    frame["ticker"]["s"] = json!("UNRELATED");
    state.enqueue_push(frame).await;

    let id = harness.instrument(PERP_MARKET_INDEX);
    harness
        .client
        .subscribe_quotes(id)
        .await
        .expect("subscribe_quotes");

    let event = next_event_within(&mut harness.client, Duration::from_secs(2))
        .await
        .expect("quote event");
    let NautilusWsMessage::Quote(quote) = event else {
        panic!("expected Quote, was {event:?}");
    };
    assert_eq!(quote.instrument_id, id);

    harness.client.disconnect().await.expect("disconnect");
}

#[tokio::test]
async fn test_market_stats_frame_emits_mark_index_and_funding_updates() {
    let state = Arc::new(TestServerState::default());
    let addr = start_ws_server(state.clone()).await;
    let mut harness = ClientHarness::build(addr).await;

    state
        .enqueue_push(load_json("ws_market_stats_update_single.json"))
        .await;
    harness
        .client
        .subscribe_market_stats(LighterMarketSelection::Market(PERP_MARKET_INDEX))
        .await
        .expect("subscribe_market_stats");
    await_subscribe_count(&state, 1).await;

    let subs = state.subscribes().await;
    assert_eq!(subs[0]["channel"], "market_stats/0");

    let mut saw_mark = false;
    let mut saw_index = false;
    let mut saw_funding = false;

    for _ in 0..4 {
        let Some(event) = next_event_within(&mut harness.client, Duration::from_secs(2)).await
        else {
            break;
        };

        match event {
            NautilusWsMessage::MarkPrice(update) => {
                saw_mark = true;
                assert_eq!(update.instrument_id, harness.instrument(PERP_MARKET_INDEX));
                assert_eq!(update.value, Price::from("2064.47"));
            }
            NautilusWsMessage::IndexPrice(update) => {
                saw_index = true;
                assert_eq!(update.instrument_id, harness.instrument(PERP_MARKET_INDEX));
                assert_eq!(update.value, Price::from("2064.48"));
            }
            NautilusWsMessage::FundingRate(update) => {
                saw_funding = true;
                assert_eq!(update.instrument_id, harness.instrument(PERP_MARKET_INDEX));
                assert_eq!(update.rate.to_string(), "0.000001");
                assert_eq!(
                    update.next_funding_ns,
                    Some(UnixNanos::from(1_774_886_400_000_000_000))
                );
            }
            _ => {}
        }

        if saw_mark && saw_index && saw_funding {
            break;
        }
    }

    assert!(saw_mark, "expected mark price update");
    assert!(saw_index, "expected index price update");
    assert!(saw_funding, "expected funding rate update");

    harness.client.disconnect().await.expect("disconnect");
}

#[tokio::test]
async fn test_spot_market_stats_frame_emits_index_update() {
    let state = Arc::new(TestServerState::default());
    let addr = start_ws_server(state.clone()).await;
    let mut harness = ClientHarness::build(addr).await;

    let mut frame = load_json("ws_spot_market_stats_update_single.json");
    frame["spot_market_stats"]["symbol"] = json!(SPOT_VENUE_SYMBOL);
    state.enqueue_push(frame).await;
    harness
        .client
        .subscribe_spot_market_stats(LighterMarketSelection::Market(SPOT_MARKET_INDEX))
        .await
        .expect("subscribe_spot_market_stats");
    await_subscribe_count(&state, 1).await;

    let subs = state.subscribes().await;
    assert_eq!(subs[0]["channel"], "spot_market_stats/2048");

    let event = next_event_within(&mut harness.client, Duration::from_secs(2))
        .await
        .expect("spot index price");
    let NautilusWsMessage::IndexPrice(update) = event else {
        panic!("expected IndexPrice, was {event:?}");
    };
    assert_eq!(update.instrument_id, harness.instrument(SPOT_MARKET_INDEX));
    assert_eq!(update.value, Price::from("1.00"));

    harness.client.disconnect().await.expect("disconnect");
}

#[tokio::test]
async fn test_one_sided_ticker_frame_does_not_emit_quote() {
    let state = Arc::new(TestServerState::default());
    let addr = start_ws_server(state.clone()).await;
    let mut harness = ClientHarness::build(addr).await;

    // Lighter emits empty `price`/`size` strings on a side that currently has
    // no resting orders; the parser must skip the frame rather than error or
    // emit a malformed Quote.
    let mut frame = load_json("ws_ticker_update.json");
    frame["ticker"]["b"]["price"] = json!("");
    frame["ticker"]["b"]["size"] = json!("");
    state.enqueue_push(frame).await;

    let id = harness.instrument(PERP_MARKET_INDEX);
    harness
        .client
        .subscribe_quotes(id)
        .await
        .expect("subscribe_quotes");

    let event = next_event_within(&mut harness.client, Duration::from_millis(300)).await;
    assert!(
        event.is_none(),
        "one-sided ticker must not emit a Quote event, was {event:?}",
    );

    harness.client.disconnect().await.expect("disconnect");
}

#[tokio::test]
async fn test_typed_snapshot_then_update_marks_only_first_with_f_snapshot() {
    // Drive the production framing: `subscribed/order_book` is the initial
    // full book and must produce `F_SNAPSHOT`-tagged deltas; the following
    // `update/order_book` must NOT carry `F_SNAPSHOT`.
    let state = Arc::new(TestServerState::default());
    let addr = start_ws_server(state.clone()).await;
    let mut harness = ClientHarness::build(addr).await;

    state
        .enqueue_push(load_json("ws_order_book_subscribed.json"))
        .await;

    let id = harness.instrument(PERP_MARKET_INDEX);
    harness
        .client
        .subscribe_book(id)
        .await
        .expect("subscribe_book");

    let snapshot_event = next_event_within(&mut harness.client, Duration::from_secs(2))
        .await
        .expect("snapshot deltas");
    let NautilusWsMessage::Deltas(deltas) = snapshot_event else {
        panic!("expected Deltas on snapshot, was {snapshot_event:?}");
    };
    assert_eq!(deltas.instrument_id, id);
    let first = deltas.deltas.first().expect("at least one delta");
    assert_eq!(first.action, BookAction::Clear);
    assert!(
        deltas
            .deltas
            .iter()
            .any(|d| d.flags & RecordFlag::F_SNAPSHOT as u8 != 0),
        "subscribed/order_book frame must produce F_SNAPSHOT-flagged deltas",
    );

    // Now drive an incremental `update/order_book` through the same harness
    // by triggering another push via a sibling subscribe. The handler
    // tracks `book_snapshots_seen` per market_index, so the second frame
    // must parse as incremental.
    state
        .enqueue_push(load_json("ws_order_book_update.json"))
        .await;
    harness
        .client
        .subscribe_quotes(id)
        .await
        .expect("trigger second push");

    let incremental_event = next_event_within(&mut harness.client, Duration::from_secs(2))
        .await
        .expect("incremental deltas");
    let NautilusWsMessage::Deltas(deltas) = incremental_event else {
        panic!("expected Deltas on incremental, was {incremental_event:?}");
    };
    assert!(
        !deltas
            .deltas
            .iter()
            .any(|d| d.flags & RecordFlag::F_SNAPSHOT as u8 != 0),
        "update/order_book after a typed snapshot must not carry F_SNAPSHOT",
    );

    harness.client.disconnect().await.expect("disconnect");
}

#[tokio::test]
async fn test_unsubscribe_trade_does_not_clear_book_snapshot_state() {
    let state = Arc::new(TestServerState::default());
    let addr = start_ws_server(state.clone()).await;
    let mut harness = ClientHarness::build(addr).await;

    // Establish the order book snapshot for market_index 0.
    state
        .enqueue_push(load_json("ws_order_book_subscribed.json"))
        .await;
    let id = harness.instrument(PERP_MARKET_INDEX);
    harness
        .client
        .subscribe_book(id)
        .await
        .expect("subscribe_book");
    let _ = next_event_within(&mut harness.client, Duration::from_secs(2)).await;

    // Subscribe and unsubscribe trade for the same market; this must NOT
    // clear book_snapshots_seen because the predicate is now scoped to
    // `order_book:*` topics. After this, the next order_book frame must
    // still parse as incremental (no F_SNAPSHOT).
    harness
        .client
        .subscribe_trades(id)
        .await
        .expect("subscribe_trades");
    harness
        .client
        .unsubscribe_trades(id)
        .await
        .expect("unsubscribe_trades");
    await_unsubscribe_count(&state, 1).await;

    // Push a second order_book frame.
    state
        .enqueue_push(load_json("ws_order_book_update.json"))
        .await;
    harness
        .client
        .subscribe_quotes(id)
        .await
        .expect("trigger push");

    // Drain events until we see the order_book Deltas; assert it is incremental.
    let mut saw_book = false;

    for _ in 0..6 {
        let Some(event) = next_event_within(&mut harness.client, Duration::from_secs(2)).await
        else {
            break;
        };

        if let NautilusWsMessage::Deltas(deltas) = event {
            assert!(
                !deltas
                    .deltas
                    .iter()
                    .any(|d| d.flags & RecordFlag::F_SNAPSHOT as u8 != 0),
                "trade unsubscribe must not clear book snapshot state",
            );
            saw_book = true;
            break;
        }
    }
    assert!(saw_book, "expected a Deltas message after the second push");

    harness.client.disconnect().await.expect("disconnect");
}

#[tokio::test]
async fn test_unsubscribe_book_ack_resets_snapshot_state() {
    let state = Arc::new(TestServerState::default());
    let addr = start_ws_server(state.clone()).await;
    let mut harness = ClientHarness::build(addr).await;

    state
        .enqueue_push(load_json("ws_order_book_subscribed.json"))
        .await;
    let id = harness.instrument(PERP_MARKET_INDEX);
    harness
        .client
        .subscribe_book(id)
        .await
        .expect("subscribe_book");
    let _ = next_event_within(&mut harness.client, Duration::from_secs(2)).await;

    harness
        .client
        .unsubscribe_book(id)
        .await
        .expect("unsubscribe_book");
    await_unsubscribe_count(&state, 1).await;

    // Resubscribe; an update arriving before the next subscription snapshot
    // must be dropped because the unsubscribe ack cleared book_snapshots_seen.
    state
        .enqueue_push(load_json("ws_order_book_update.json"))
        .await;
    harness
        .client
        .subscribe_book(id)
        .await
        .expect("re-subscribe");
    assert!(
        next_event_within(&mut harness.client, Duration::from_millis(300))
            .await
            .is_none(),
        "update after book unsubscribe must not use the prior snapshot state",
    );

    state
        .enqueue_push(load_json("ws_order_book_subscribed.json"))
        .await;
    harness
        .client
        .subscribe_quotes(id)
        .await
        .expect("trigger snapshot push");

    let mut saw_snapshot = false;

    for _ in 0..6 {
        let Some(event) = next_event_within(&mut harness.client, Duration::from_secs(2)).await
        else {
            break;
        };

        if let NautilusWsMessage::Deltas(deltas) = event
            && deltas
                .deltas
                .iter()
                .any(|d| d.flags & RecordFlag::F_SNAPSHOT as u8 != 0)
        {
            saw_snapshot = true;
            break;
        }
    }
    assert!(
        saw_snapshot,
        "fresh subscribe after unsubscribe must yield snapshot deltas"
    );

    harness.client.disconnect().await.expect("disconnect");
}

#[tokio::test]
async fn test_subscribe_account_includes_auth_field() {
    let state = Arc::new(TestServerState::default());
    let addr = start_ws_server(state.clone()).await;
    let mut harness = ClientHarness::build(addr).await;

    let token = "schnorr-signature-bytes".to_string();
    harness
        .client
        .subscribe_account(LighterWsChannel::AccountAll(123_456), token.clone())
        .await
        .expect("subscribe_account");

    await_subscribe_count(&state, 1).await;
    let subs = state.subscribes().await;
    assert_eq!(subs[0]["type"], "subscribe");
    assert_eq!(subs[0]["channel"], "account_all/123456");
    assert_eq!(subs[0]["auth"].as_str(), Some(token.as_str()));

    // Pin the redacting Debug: the bearer token must not leak into the
    // formatted output even though it is held in `subscription_args` for
    // reconnect replay.
    let dbg = format!("{:?}", harness.client);
    assert!(
        !dbg.contains(&token),
        "Debug output must not contain the auth token, found: {dbg}",
    );

    harness.client.disconnect().await.expect("disconnect");
}

#[tokio::test]
async fn test_height_subscription_routes_through_raw() {
    let state = Arc::new(TestServerState::default());
    let addr = start_ws_server(state.clone()).await;
    let mut harness = ClientHarness::build(addr).await;

    state.enqueue_push(load_json("ws_height_update.json")).await;
    harness
        .client
        .subscribe_height()
        .await
        .expect("subscribe_height");

    let event = next_event_within(&mut harness.client, Duration::from_secs(2))
        .await
        .expect("height frame routed");
    let NautilusWsMessage::Raw(value) = event else {
        panic!("expected Raw, was {event:?}");
    };
    // Pin the payload contents so a regression that ships the wrong frame
    // through the Raw arm or corrupts fields gets caught.
    assert_eq!(value["type"].as_str(), Some("update/height"));
    assert_eq!(value["channel"].as_str(), Some("height"));
    assert_eq!(value["height"].as_i64(), Some(227_535_532));

    harness.client.disconnect().await.expect("disconnect");
}

#[tokio::test]
async fn test_subscription_count_tracks_subscribe_then_ack() {
    let state = Arc::new(TestServerState::default());
    let addr = start_ws_server(state.clone()).await;
    let mut harness = ClientHarness::build(addr).await;

    assert_eq!(harness.client.subscription_count(), 0, "initial");

    let id = harness.instrument(PERP_MARKET_INDEX);
    harness
        .client
        .subscribe_book(id)
        .await
        .expect("subscribe_book");
    await_subscribe_count(&state, 1).await;
    await_subscription_count(&harness.client, 1).await;
    assert_eq!(
        harness.client.subscription_count(),
        1,
        "after subscribe ack"
    );

    harness
        .client
        .unsubscribe_book(id)
        .await
        .expect("unsubscribe_book");
    await_unsubscribe_count(&state, 1).await;

    let started = std::time::Instant::now();
    while harness.client.subscription_count() > 0 && started.elapsed() < Duration::from_secs(2) {
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    assert_eq!(
        harness.client.subscription_count(),
        0,
        "after unsubscribe ack"
    );

    harness.client.disconnect().await.expect("disconnect");
}

#[tokio::test]
async fn test_multi_market_book_state_isolation() {
    let state = Arc::new(TestServerState::default());
    let addr = start_ws_server(state.clone()).await;
    let mut harness = ClientHarness::build(addr).await;

    let id0 = harness.instrument(PERP_MARKET_INDEX);
    let id1 = harness.instrument(SECOND_MARKET_INDEX);

    // First subscribe drains a snapshot for market 0; the second subscribe
    // drains a snapshot for market 1. Each market_index has its own
    // book_snapshots_seen entry, so both frames must carry F_SNAPSHOT.
    state
        .enqueue_push(book_snapshot_frame_for_market(PERP_MARKET_INDEX))
        .await;
    state
        .enqueue_push(book_snapshot_frame_for_market(SECOND_MARKET_INDEX))
        .await;

    harness
        .client
        .subscribe_book(id0)
        .await
        .expect("subscribe market 0");
    let market0_event = next_event_within(&mut harness.client, Duration::from_secs(2))
        .await
        .expect("market 0 deltas");
    let NautilusWsMessage::Deltas(deltas0) = market0_event else {
        panic!("expected Deltas for market 0, was {market0_event:?}");
    };
    assert_eq!(deltas0.instrument_id, id0);
    assert!(
        deltas0
            .deltas
            .iter()
            .any(|d| d.flags & RecordFlag::F_SNAPSHOT as u8 != 0),
        "market 0 first frame must carry F_SNAPSHOT",
    );

    harness
        .client
        .subscribe_book(id1)
        .await
        .expect("subscribe market 1");
    let market1_event = next_event_within(&mut harness.client, Duration::from_secs(2))
        .await
        .expect("market 1 deltas");
    let NautilusWsMessage::Deltas(deltas1) = market1_event else {
        panic!("expected Deltas for market 1, was {market1_event:?}");
    };
    assert_eq!(
        deltas1.instrument_id, id1,
        "market 1 frame must resolve to market 1 instrument, not market 0",
    );
    assert!(
        deltas1
            .deltas
            .iter()
            .any(|d| d.flags & RecordFlag::F_SNAPSHOT as u8 != 0),
        "market 1 first frame must carry F_SNAPSHOT independently of market 0",
    );

    // Push a second market 1 frame via a third subscribe (channel doesn't
    // matter for the queue drain). The second frame for market 1 must parse
    // as incremental, not as a fresh snapshot.
    state
        .enqueue_push(book_update_frame_for_market(SECOND_MARKET_INDEX))
        .await;
    harness
        .client
        .subscribe_quotes(id0)
        .await
        .expect("trigger third push");

    let mut saw_incremental_market1 = false;

    for _ in 0..6 {
        let Some(event) = next_event_within(&mut harness.client, Duration::from_secs(2)).await
        else {
            break;
        };

        if let NautilusWsMessage::Deltas(d) = event
            && d.instrument_id == id1
        {
            assert!(
                !d.deltas
                    .iter()
                    .any(|delta| delta.flags & RecordFlag::F_SNAPSHOT as u8 != 0),
                "second market 1 frame must be incremental",
            );
            saw_incremental_market1 = true;
            break;
        }
    }
    assert!(
        saw_incremental_market1,
        "expected an incremental market 1 frame after the snapshot",
    );

    harness.client.disconnect().await.expect("disconnect");
}

#[tokio::test]
async fn test_reconnect_replays_authenticated_and_public_subscriptions() {
    let state = Arc::new(TestServerState::default());
    let addr = start_ws_server(state.clone()).await;
    let mut harness = ClientHarness::build(addr).await;

    let id = harness.instrument(PERP_MARKET_INDEX);
    let token = "schnorr-token-replay-test".to_string();

    // First subscribe goes through normally; arm the drop AFTER it acks.
    harness
        .client
        .subscribe_book(id)
        .await
        .expect("subscribe_book");
    await_subscribe_count(&state, 1).await;

    state
        .drop_after_next_subscribe
        .store(true, Ordering::Relaxed);
    harness
        .client
        .subscribe_account(LighterWsChannel::AccountAll(789), token.clone())
        .await
        .expect("subscribe_account");

    // The server records the second subscribe, acks it, then closes.
    await_subscribe_count(&state, 2).await;

    // Drain events until Reconnected lands. The network layer reconnects
    // after `RECONNECT_BASE_BACKOFF` (250 ms) plus jitter, so a few seconds
    // is plenty of headroom.
    let mut saw_reconnected = false;

    for _ in 0..20 {
        let Some(event) = next_event_within(&mut harness.client, Duration::from_secs(3)).await
        else {
            break;
        };

        if matches!(event, NautilusWsMessage::Reconnected) {
            saw_reconnected = true;
            break;
        }
    }
    assert!(
        saw_reconnected,
        "expected Reconnected after server-driven close"
    );

    // The spawn loop replays both topics from `subscription_args`. Order is
    // non-deterministic (DashMap iter), so assert by content.
    await_subscribe_count(&state, 4).await;

    let subs = state.subscribes().await;
    let public_count = subs
        .iter()
        .filter(|s| s["channel"] == "order_book/0")
        .count();
    let account_subs: Vec<&Value> = subs
        .iter()
        .filter(|s| s["channel"] == "account_all/789")
        .collect();

    assert_eq!(public_count, 2, "public channel must replay on reconnect");
    assert_eq!(
        account_subs.len(),
        2,
        "auth channel must replay on reconnect",
    );

    for s in account_subs {
        assert_eq!(
            s["auth"].as_str(),
            Some(token.as_str()),
            "auth token must be carried on each replay",
        );
    }

    harness.client.disconnect().await.expect("disconnect");
}
