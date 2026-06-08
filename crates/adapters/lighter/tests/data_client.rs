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

//! Integration tests for the [`LighterDataClient`].
//!
//! These tests stand up a unified Axum mock that serves both the Lighter REST
//! endpoints and the WebSocket `/stream` socket, instantiate a
//! [`LighterDataClient`] against it, and drive the public [`DataClient`] trait
//! surface end to end. They assert on the [`DataEvent`] stream observed on
//! the data event channel and on the venue-side subscription/unsubscription
//! frames recorded by the mock.
//!
//! Lower-level WebSocket framing, retry, and HTTP fixture coverage lives in
//! `tests/websocket.rs` and `tests/http.rs`. These tests focus on the
//! end-to-end flow through bootstrap, the WS consumption task, and the
//! request-response paths.

use std::{
    net::SocketAddr,
    num::NonZeroUsize,
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
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
};
use chrono::{TimeZone, Utc};
use futures_util::{SinkExt, StreamExt};
use nautilus_common::{
    clients::DataClient,
    live::runner::replace_data_event_sender,
    messages::{
        DataEvent,
        data::{
            DataResponse, RequestBars, RequestBookDepth, RequestBookSnapshot, RequestFundingRates,
            RequestInstrument, RequestInstruments, RequestQuotes, RequestTrades, SubscribeBars,
            SubscribeBookDeltas, SubscribeBookDepth10, SubscribeFundingRates, SubscribeIndexPrices,
            SubscribeMarkPrices, SubscribeQuotes, SubscribeTrades, UnsubscribeBars,
            UnsubscribeBookDeltas, UnsubscribeBookDepth10, UnsubscribeIndexPrices,
            UnsubscribeInstrument, UnsubscribeMarkPrices, UnsubscribeQuotes, UnsubscribeTrades,
        },
    },
    testing::wait_until_async,
};
use nautilus_core::{UUID4, UnixNanos};
use nautilus_lighter::{
    common::consts::LIGHTER_VENUE, config::LighterDataClientConfig, data::LighterDataClient,
};
use nautilus_model::{
    data::{BarSpecification, BarType, Data},
    enums::{AggregationSource, BarAggregation, BookType, PriceType},
    identifiers::{ClientId, InstrumentId},
    instruments::Instrument,
};
use rstest::rstest;
use serde_json::{Value, json};
const ETH_PERP_SYMBOL: &str = "ETH-PERP";

fn data_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test_data")
}

fn load_json(filename: &str) -> Value {
    let content = std::fs::read_to_string(data_path().join(filename))
        .unwrap_or_else(|_| panic!("failed to read {filename}"));
    serde_json::from_str(&content).expect("invalid json")
}

fn eth_perp_id() -> InstrumentId {
    InstrumentId::from(format!("{ETH_PERP_SYMBOL}.LIGHTER").as_str())
}

fn client_id() -> ClientId {
    ClientId::new("LIGHTER")
}

#[derive(Clone, Default)]
struct TestServerState {
    connection_count: Arc<tokio::sync::Mutex<usize>>,
    subscribes: Arc<tokio::sync::Mutex<Vec<Value>>>,
    unsubscribes: Arc<tokio::sync::Mutex<Vec<Value>>>,
    /// Frames queued by tests, drained one per `subscribe` ack in FIFO order.
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

async fn order_book_details() -> Response {
    (
        StatusCode::OK,
        std::fs::read_to_string(data_path().join("http_order_book_details.json")).unwrap(),
    )
        .into_response()
}

async fn order_book_orders() -> Response {
    (
        StatusCode::OK,
        std::fs::read_to_string(data_path().join("http_order_book_orders.json")).unwrap(),
    )
        .into_response()
}

async fn recent_trades() -> Response {
    (
        StatusCode::OK,
        std::fs::read_to_string(data_path().join("http_recent_trades.json")).unwrap(),
    )
        .into_response()
}

async fn fundings() -> Response {
    (
        StatusCode::OK,
        std::fs::read_to_string(data_path().join("http_fundings.json")).unwrap(),
    )
        .into_response()
}

async fn candles() -> Response {
    (
        StatusCode::OK,
        std::fs::read_to_string(data_path().join("http_candles.json")).unwrap(),
    )
        .into_response()
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

fn build_router(state: Arc<TestServerState>) -> Router {
    Router::new()
        .route("/api/v1/orderBookDetails", get(order_book_details))
        .route("/api/v1/orderBookOrders", get(order_book_orders))
        .route("/api/v1/recentTrades", get(recent_trades))
        .route("/api/v1/fundings", get(fundings))
        .route("/api/v1/candles", get(candles))
        .route("/stream", get(handle_ws_upgrade))
        .with_state(state)
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
    // Give axum a moment to start accepting connections before tests dial in.
    tokio::time::sleep(Duration::from_millis(50)).await;
    (addr, state)
}

fn build_config(addr: SocketAddr) -> LighterDataClientConfig {
    LighterDataClientConfig {
        base_url_http: Some(format!("http://{addr}")),
        base_url_ws: Some(format!("ws://{addr}/stream")),
        // Disable the periodic refresh loop; tests drive bootstrap directly
        // via `connect()` and request_instruments(). A nonzero interval would
        // leak a background task across the entire crate's test run.
        update_instruments_interval_mins: 0,
        ..LighterDataClientConfig::default()
    }
}

/// Installs a fresh data event sender and returns `(client, receiver)`.
///
/// The runner sender lives in a thread-local and is cloned into the client
/// during `LighterDataClient::new`. Installing a fresh sender per test keeps
/// events from prior tests out of the channel.
fn build_client(
    config: LighterDataClientConfig,
) -> (
    LighterDataClient,
    tokio::sync::mpsc::UnboundedReceiver<DataEvent>,
) {
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    replace_data_event_sender(sender);
    let client = LighterDataClient::new(client_id(), config).expect("construct data client");
    (client, receiver)
}

/// Pulls every event currently sitting in the receiver, returning the count.
///
/// Tests use this to consume the instrument events emitted on `connect()`
/// before asserting on subscription-driven events.
fn drain_pending(rx: &mut tokio::sync::mpsc::UnboundedReceiver<DataEvent>) -> usize {
    let mut drained = 0;
    while rx.try_recv().is_ok() {
        drained += 1;
    }
    drained
}

/// Pulls events until one matches the predicate, returning it.
///
/// Used when the consumer loop emits intermediate `Instrument` / `Raw`
/// frames before the target event under the same drain.
async fn next_event_matching<F>(
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<DataEvent>,
    timeout: Duration,
    mut predicate: F,
) -> Option<DataEvent>
where
    F: FnMut(&DataEvent) -> bool,
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

#[rstest]
#[tokio::test]
async fn test_connect_disconnect_lifecycle() {
    let (addr, state) = start_server().await;
    let (mut client, _rx) = build_client(build_config(addr));

    assert!(!client.is_connected());

    client.connect().await.expect("connect");
    assert!(client.is_connected());

    wait_until_async(
        || {
            let state = Arc::clone(&state);
            async move { *state.connection_count.lock().await == 1 }
        },
        Duration::from_secs(2),
    )
    .await;

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
#[tokio::test]
async fn test_connect_emits_instrument_event() {
    let (addr, _state) = start_server().await;
    let (mut client, mut rx) = build_client(build_config(addr));

    client.connect().await.expect("connect");

    // The orderBookDetails fixture carries one perp (ETH); `connect()` fans it
    // out through the data event channel as a single `DataEvent::Instrument`.
    let event = tokio::time::timeout(Duration::from_secs(2), rx.recv())
        .await
        .expect("timeout waiting for instrument event")
        .expect("channel closed");

    match event {
        DataEvent::Instrument(instrument) => {
            assert_eq!(instrument.id(), eth_perp_id());
        }
        other => panic!("expected Instrument event, was {other:?}"),
    }

    client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test]
async fn test_unsubscribe_instrument_is_cache_replay_noop() {
    let (addr, state) = start_server().await;
    let (mut client, mut rx) = build_client(build_config(addr));

    client.connect().await.expect("connect");
    drain_pending(&mut rx);

    client
        .unsubscribe_instrument(&UnsubscribeInstrument::new(
            eth_perp_id(),
            Some(client_id()),
            None,
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ))
        .expect("unsubscribe_instrument");

    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(
        state.unsubscribes().await.is_empty(),
        "instrument unsubscribe is cache-local and must not hit the venue",
    );

    client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test]
async fn test_subscribe_book_deltas_emits_deltas() {
    let (addr, state) = start_server().await;
    let (mut client, mut rx) = build_client(build_config(addr));

    client.connect().await.expect("connect");
    drain_pending(&mut rx);

    state
        .enqueue_push(load_json("ws_order_book_subscribed.json"))
        .await;

    let instrument_id = eth_perp_id();
    client
        .subscribe_book_deltas(SubscribeBookDeltas::new(
            instrument_id,
            BookType::L2_MBP,
            Some(client_id()),
            None,
            UUID4::new(),
            UnixNanos::default(),
            None,
            false,
            None,
            None,
        ))
        .expect("subscribe_book_deltas");

    await_subscribe_count(&state, 1).await;
    let subs = state.subscribes().await;
    assert_eq!(subs[0]["channel"], "order_book/0");

    let event = next_event_matching(&mut rx, Duration::from_secs(2), |e| {
        matches!(e, DataEvent::Data(Data::Deltas(_)))
    })
    .await
    .expect("expected Deltas event");

    match event {
        DataEvent::Data(Data::Deltas(deltas)) => {
            assert_eq!(deltas.instrument_id, instrument_id);
            assert!(!deltas.deltas.is_empty());
        }
        other => panic!("expected Deltas event, was {other:?}"),
    }

    client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test]
async fn test_subscribe_book_deltas_rejects_wrong_book_type() {
    let (addr, _state) = start_server().await;
    let (mut client, _rx) = build_client(build_config(addr));

    let err = client
        .subscribe_book_deltas(SubscribeBookDeltas::new(
            eth_perp_id(),
            BookType::L1_MBP,
            Some(client_id()),
            None,
            UUID4::new(),
            UnixNanos::default(),
            None,
            false,
            None,
            None,
        ))
        .unwrap_err();

    assert!(err.to_string().contains("L2_MBP"));
}

#[rstest]
#[tokio::test]
async fn test_subscribe_book_depth10_emits_depth10_only() {
    let (addr, state) = start_server().await;
    let (mut client, mut rx) = build_client(build_config(addr));

    client.connect().await.expect("connect");
    drain_pending(&mut rx);

    state
        .enqueue_push(load_json("ws_order_book_subscribed.json"))
        .await;

    let instrument_id = eth_perp_id();
    client
        .subscribe_book_depth10(SubscribeBookDepth10::new(
            instrument_id,
            BookType::L2_MBP,
            Some(client_id()),
            None,
            UUID4::new(),
            UnixNanos::default(),
            None,
            false,
            None,
            None,
        ))
        .expect("subscribe_book_depth10");

    await_subscribe_count(&state, 1).await;
    assert_eq!(state.subscribes().await[0]["channel"], "order_book/0");

    let event = next_event_matching(&mut rx, Duration::from_secs(2), |e| {
        matches!(e, DataEvent::Data(Data::Depth10(_)))
    })
    .await
    .expect("expected Depth10 event");

    match event {
        DataEvent::Data(Data::Depth10(depth)) => {
            assert_eq!(depth.instrument_id, instrument_id);
        }
        other => panic!("expected Depth10 event, was {other:?}"),
    }

    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(
        rx.try_recv().is_err(),
        "depth-only subscription must not emit book deltas",
    );

    client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test]
async fn test_subscribe_quotes_emits_quote() {
    let (addr, state) = start_server().await;
    let (mut client, mut rx) = build_client(build_config(addr));

    client.connect().await.expect("connect");
    drain_pending(&mut rx);

    state.enqueue_push(load_json("ws_ticker_update.json")).await;

    let instrument_id = eth_perp_id();
    client
        .subscribe_quotes(SubscribeQuotes::new(
            instrument_id,
            Some(client_id()),
            None,
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ))
        .expect("subscribe_quotes");

    await_subscribe_count(&state, 1).await;
    assert_eq!(state.subscribes().await[0]["channel"], "ticker/0");

    let event = next_event_matching(&mut rx, Duration::from_secs(2), |e| {
        matches!(e, DataEvent::Data(Data::Quote(_)))
    })
    .await
    .expect("expected Quote event");

    if let DataEvent::Data(Data::Quote(quote)) = event {
        assert_eq!(quote.instrument_id, instrument_id);
    }

    client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test]
async fn test_subscribe_trades_emits_trade() {
    let (addr, state) = start_server().await;
    let (mut client, mut rx) = build_client(build_config(addr));

    client.connect().await.expect("connect");
    drain_pending(&mut rx);

    state.enqueue_push(load_json("ws_trade_update.json")).await;

    let instrument_id = eth_perp_id();
    client
        .subscribe_trades(SubscribeTrades::new(
            instrument_id,
            Some(client_id()),
            None,
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ))
        .expect("subscribe_trades");

    await_subscribe_count(&state, 1).await;
    assert_eq!(state.subscribes().await[0]["channel"], "trade/0");

    let event = next_event_matching(&mut rx, Duration::from_secs(2), |e| {
        matches!(e, DataEvent::Data(Data::Trade(_)))
    })
    .await
    .expect("expected Trade event");

    if let DataEvent::Data(Data::Trade(tick)) = event {
        assert_eq!(tick.instrument_id, instrument_id);
    }

    client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test]
async fn test_subscribe_mark_index_funding_share_one_ws_subscription() {
    // Mark price, index price, and funding rate are all served by the same
    // venue `market_stats` channel. The data client must coalesce them into a
    // single WS subscribe; otherwise three concurrent subscribes would surface
    // duplicate events on every fan-out frame.
    let (addr, state) = start_server().await;
    let (mut client, mut rx) = build_client(build_config(addr));

    client.connect().await.expect("connect");
    drain_pending(&mut rx);

    state
        .enqueue_push(load_json("ws_market_stats_update_single.json"))
        .await;

    let instrument_id = eth_perp_id();
    client
        .subscribe_mark_prices(SubscribeMarkPrices::new(
            instrument_id,
            Some(client_id()),
            None,
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ))
        .expect("subscribe_mark_prices");
    client
        .subscribe_index_prices(SubscribeIndexPrices::new(
            instrument_id,
            Some(client_id()),
            None,
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ))
        .expect("subscribe_index_prices");
    client
        .subscribe_funding_rates(SubscribeFundingRates::new(
            instrument_id,
            Some(client_id()),
            None,
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ))
        .expect("subscribe_funding_rates");

    await_subscribe_count(&state, 1).await;
    let subs = state.subscribes().await;
    assert_eq!(subs.len(), 1, "market_stats must subscribe exactly once");
    assert_eq!(subs[0]["channel"], "market_stats/0");

    let mut saw_mark = false;
    let mut saw_index = false;
    let mut saw_funding = false;

    for _ in 0..6 {
        let Some(event) = tokio::time::timeout(Duration::from_secs(2), rx.recv())
            .await
            .ok()
            .flatten()
        else {
            break;
        };

        match event {
            DataEvent::Data(Data::MarkPriceUpdate(update)) => {
                saw_mark = true;
                assert_eq!(update.instrument_id, instrument_id);
            }
            DataEvent::Data(Data::IndexPriceUpdate(update)) => {
                saw_index = true;
                assert_eq!(update.instrument_id, instrument_id);
            }
            DataEvent::FundingRate(update) => {
                saw_funding = true;
                assert_eq!(update.instrument_id, instrument_id);
            }
            _ => {}
        }

        if saw_mark && saw_index && saw_funding {
            break;
        }
    }

    assert!(saw_mark, "expected mark price event");
    assert!(saw_index, "expected index price event");
    assert!(saw_funding, "expected funding rate event");

    client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test]
async fn test_unsubscribe_market_stats_drops_last_share_triggers_unsubscribe() {
    let (addr, state) = start_server().await;
    let (mut client, mut rx) = build_client(build_config(addr));

    client.connect().await.expect("connect");
    drain_pending(&mut rx);

    let instrument_id = eth_perp_id();
    client
        .subscribe_mark_prices(SubscribeMarkPrices::new(
            instrument_id,
            Some(client_id()),
            None,
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ))
        .expect("subscribe_mark_prices");
    client
        .subscribe_index_prices(SubscribeIndexPrices::new(
            instrument_id,
            Some(client_id()),
            None,
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ))
        .expect("subscribe_index_prices");

    await_subscribe_count(&state, 1).await;

    // First unsubscribe leaves index price active; no venue unsubscribe yet.
    client
        .unsubscribe_mark_prices(&UnsubscribeMarkPrices::new(
            instrument_id,
            Some(client_id()),
            None,
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ))
        .expect("unsubscribe_mark_prices");

    // Pin no race: give the runtime a tick to confirm no unsubscribe is sent.
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(state.unsubscribes().await.is_empty());

    // Dropping the last share unsubscribes from the shared channel.
    client
        .unsubscribe_index_prices(&UnsubscribeIndexPrices::new(
            instrument_id,
            Some(client_id()),
            None,
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ))
        .expect("unsubscribe_index_prices");

    await_unsubscribe_count(&state, 1).await;
    let unsubs = state.unsubscribes().await;
    assert_eq!(unsubs[0]["channel"], "market_stats/0");

    client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test]
async fn test_unsubscribe_book_deltas_sends_venue_unsubscribe() {
    let (addr, state) = start_server().await;
    let (mut client, mut rx) = build_client(build_config(addr));

    client.connect().await.expect("connect");
    drain_pending(&mut rx);

    let instrument_id = eth_perp_id();
    client
        .subscribe_book_deltas(SubscribeBookDeltas::new(
            instrument_id,
            BookType::L2_MBP,
            Some(client_id()),
            None,
            UUID4::new(),
            UnixNanos::default(),
            None,
            false,
            None,
            None,
        ))
        .expect("subscribe_book_deltas");
    await_subscribe_count(&state, 1).await;

    client
        .unsubscribe_book_deltas(&UnsubscribeBookDeltas::new(
            instrument_id,
            Some(client_id()),
            None,
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ))
        .expect("unsubscribe_book_deltas");

    await_unsubscribe_count(&state, 1).await;
    assert_eq!(state.unsubscribes().await[0]["channel"], "order_book/0");

    client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test]
async fn test_unsubscribe_quotes_and_trades_send_venue_frames() {
    let (addr, state) = start_server().await;
    let (mut client, mut rx) = build_client(build_config(addr));

    client.connect().await.expect("connect");
    drain_pending(&mut rx);

    let instrument_id = eth_perp_id();
    client
        .subscribe_quotes(SubscribeQuotes::new(
            instrument_id,
            Some(client_id()),
            None,
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ))
        .expect("subscribe_quotes");
    client
        .subscribe_trades(SubscribeTrades::new(
            instrument_id,
            Some(client_id()),
            None,
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ))
        .expect("subscribe_trades");

    await_subscribe_count(&state, 2).await;

    client
        .unsubscribe_quotes(&UnsubscribeQuotes::new(
            instrument_id,
            Some(client_id()),
            None,
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ))
        .expect("unsubscribe_quotes");
    client
        .unsubscribe_trades(&UnsubscribeTrades::new(
            instrument_id,
            Some(client_id()),
            None,
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ))
        .expect("unsubscribe_trades");

    await_unsubscribe_count(&state, 2).await;
    let unsubs = state.unsubscribes().await;
    let channels: Vec<&str> = unsubs
        .iter()
        .map(|v| v["channel"].as_str().unwrap_or(""))
        .collect();
    assert!(channels.contains(&"ticker/0"));
    assert!(channels.contains(&"trade/0"));

    client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test]
async fn test_unsubscribe_book_depth10_without_deltas_sends_venue_unsubscribe() {
    let (addr, state) = start_server().await;
    let (mut client, mut rx) = build_client(build_config(addr));

    client.connect().await.expect("connect");
    drain_pending(&mut rx);

    let instrument_id = eth_perp_id();
    client
        .subscribe_book_depth10(SubscribeBookDepth10::new(
            instrument_id,
            BookType::L2_MBP,
            Some(client_id()),
            None,
            UUID4::new(),
            UnixNanos::default(),
            None,
            false,
            None,
            None,
        ))
        .expect("subscribe_book_depth10");
    await_subscribe_count(&state, 1).await;
    assert_eq!(state.subscribes().await[0]["channel"], "order_book/0");

    client
        .unsubscribe_book_depth10(&UnsubscribeBookDepth10::new(
            instrument_id,
            Some(client_id()),
            None,
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ))
        .expect("unsubscribe_book_depth10");

    await_unsubscribe_count(&state, 1).await;
    assert_eq!(state.unsubscribes().await[0]["channel"], "order_book/0");

    client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test]
async fn test_book_deltas_and_depth10_share_order_book_stream() {
    let (addr, state) = start_server().await;
    let (mut client, mut rx) = build_client(build_config(addr));

    client.connect().await.expect("connect");
    drain_pending(&mut rx);

    let instrument_id = eth_perp_id();
    state
        .enqueue_push(load_json("ws_order_book_subscribed.json"))
        .await;

    client
        .subscribe_book_deltas(SubscribeBookDeltas::new(
            instrument_id,
            BookType::L2_MBP,
            Some(client_id()),
            None,
            UUID4::new(),
            UnixNanos::default(),
            None,
            false,
            None,
            None,
        ))
        .expect("subscribe_book_deltas");
    await_subscribe_count(&state, 1).await;

    let event = next_event_matching(&mut rx, Duration::from_secs(2), |e| {
        matches!(e, DataEvent::Data(Data::Deltas(_)))
    })
    .await
    .expect("expected initial Deltas event");

    match event {
        DataEvent::Data(Data::Deltas(deltas)) => {
            assert_eq!(deltas.instrument_id, instrument_id);
        }
        other => panic!("expected Deltas event, was {other:?}"),
    }

    client
        .subscribe_book_depth10(SubscribeBookDepth10::new(
            instrument_id,
            BookType::L2_MBP,
            Some(client_id()),
            None,
            UUID4::new(),
            UnixNanos::default(),
            None,
            false,
            None,
            None,
        ))
        .expect("subscribe_book_depth10");

    tokio::time::sleep(Duration::from_millis(100)).await;
    let subs = state.subscribes().await;
    assert_eq!(
        subs.len(),
        1,
        "late local subscriber must reuse the venue order_book stream"
    );
    assert_eq!(subs[0]["channel"], "order_book/0");

    let event = next_event_matching(&mut rx, Duration::from_secs(2), |e| {
        matches!(e, DataEvent::Data(Data::Depth10(_)))
    })
    .await
    .expect("expected cached Depth10 event");

    match event {
        DataEvent::Data(Data::Depth10(depth)) => {
            assert_eq!(depth.instrument_id, instrument_id);
        }
        other => panic!("expected Depth10 event, was {other:?}"),
    }

    let next = tokio::time::timeout(Duration::from_millis(200), rx.recv()).await;
    assert!(
        !matches!(next, Ok(Some(DataEvent::Data(Data::Deltas(_))))),
        "late depth10 subscriber must not re-emit deltas",
    );

    client
        .unsubscribe_book_depth10(&UnsubscribeBookDepth10::new(
            instrument_id,
            Some(client_id()),
            None,
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ))
        .expect("unsubscribe_book_depth10");

    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(
        state.unsubscribes().await.is_empty(),
        "dropping depth10 must leave the deltas stream active",
    );

    client
        .unsubscribe_book_deltas(&UnsubscribeBookDeltas::new(
            instrument_id,
            Some(client_id()),
            None,
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ))
        .expect("unsubscribe_book_deltas");

    await_unsubscribe_count(&state, 1).await;
    assert_eq!(state.unsubscribes().await[0]["channel"], "order_book/0");

    client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test]
async fn test_book_depth10_and_deltas_share_order_book_stream() {
    let (addr, state) = start_server().await;
    let (mut client, mut rx) = build_client(build_config(addr));

    client.connect().await.expect("connect");
    drain_pending(&mut rx);

    let instrument_id = eth_perp_id();
    state
        .enqueue_push(load_json("ws_order_book_subscribed.json"))
        .await;

    client
        .subscribe_book_depth10(SubscribeBookDepth10::new(
            instrument_id,
            BookType::L2_MBP,
            Some(client_id()),
            None,
            UUID4::new(),
            UnixNanos::default(),
            None,
            false,
            None,
            None,
        ))
        .expect("subscribe_book_depth10");
    await_subscribe_count(&state, 1).await;

    let event = next_event_matching(&mut rx, Duration::from_secs(2), |e| {
        matches!(e, DataEvent::Data(Data::Depth10(_)))
    })
    .await
    .expect("expected initial Depth10 event");

    match event {
        DataEvent::Data(Data::Depth10(depth)) => {
            assert_eq!(depth.instrument_id, instrument_id);
        }
        other => panic!("expected Depth10 event, was {other:?}"),
    }

    client
        .subscribe_book_deltas(SubscribeBookDeltas::new(
            instrument_id,
            BookType::L2_MBP,
            Some(client_id()),
            None,
            UUID4::new(),
            UnixNanos::default(),
            None,
            false,
            None,
            None,
        ))
        .expect("subscribe_book_deltas");

    tokio::time::sleep(Duration::from_millis(100)).await;
    let subs = state.subscribes().await;
    assert_eq!(
        subs.len(),
        1,
        "late local subscriber must reuse the venue order_book stream"
    );
    assert_eq!(subs[0]["channel"], "order_book/0");

    let event = next_event_matching(&mut rx, Duration::from_secs(2), |e| {
        matches!(e, DataEvent::Data(Data::Deltas(_)))
    })
    .await
    .expect("expected cached Deltas event");

    match event {
        DataEvent::Data(Data::Deltas(deltas)) => {
            assert_eq!(deltas.instrument_id, instrument_id);
        }
        other => panic!("expected Deltas event, was {other:?}"),
    }

    let next = tokio::time::timeout(Duration::from_millis(200), rx.recv()).await;
    assert!(
        !matches!(next, Ok(Some(DataEvent::Data(Data::Depth10(_))))),
        "late deltas subscriber must not re-emit depth10",
    );

    client
        .unsubscribe_book_deltas(&UnsubscribeBookDeltas::new(
            instrument_id,
            Some(client_id()),
            None,
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ))
        .expect("unsubscribe_book_deltas");

    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(
        state.unsubscribes().await.is_empty(),
        "dropping deltas must leave the depth10 stream active",
    );

    client
        .unsubscribe_book_depth10(&UnsubscribeBookDepth10::new(
            instrument_id,
            Some(client_id()),
            None,
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ))
        .expect("unsubscribe_book_depth10");

    await_unsubscribe_count(&state, 1).await;
    assert_eq!(state.unsubscribes().await[0]["channel"], "order_book/0");

    client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test]
async fn test_subscribe_bars_accepts_ws_streamable_resolution() {
    let (addr, state) = start_server().await;
    let (mut client, mut rx) = build_client(build_config(addr));

    client.connect().await.expect("connect");
    drain_pending(&mut rx);

    let bar_type = BarType::new(
        eth_perp_id(),
        BarSpecification::new(1, BarAggregation::Minute, PriceType::Last),
        AggregationSource::External,
    );

    client
        .subscribe_bars(SubscribeBars::new(
            bar_type,
            Some(client_id()),
            None,
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ))
        .expect("subscribe_bars");

    await_subscribe_count(&state, 1).await;
    let subs = state.subscribes().await;
    assert_eq!(subs[0]["channel"], "candle/0/1m");

    client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test]
async fn test_request_instruments_emits_response() {
    let (addr, _state) = start_server().await;
    let (mut client, mut rx) = build_client(build_config(addr));

    client.connect().await.expect("connect");
    drain_pending(&mut rx);

    client
        .request_instruments(RequestInstruments::new(
            None,
            None,
            Some(client_id()),
            Some(*LIGHTER_VENUE),
            UUID4::new(),
            UnixNanos::default(),
            None,
        ))
        .expect("request_instruments");

    let event = next_event_matching(&mut rx, Duration::from_secs(2), |e| {
        matches!(e, DataEvent::Response(DataResponse::Instruments(_)))
    })
    .await
    .expect("expected Instruments response");

    if let DataEvent::Response(DataResponse::Instruments(response)) = event {
        assert_eq!(response.data.len(), 1);
        assert_eq!(response.data[0].id(), eth_perp_id());
    }

    client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test]
async fn test_request_instrument_emits_response() {
    let (addr, _state) = start_server().await;
    let (mut client, mut rx) = build_client(build_config(addr));

    client.connect().await.expect("connect");
    drain_pending(&mut rx);

    client
        .request_instrument(RequestInstrument::new(
            eth_perp_id(),
            None,
            None,
            Some(client_id()),
            UUID4::new(),
            UnixNanos::default(),
            None,
        ))
        .expect("request_instrument");

    let event = next_event_matching(&mut rx, Duration::from_secs(2), |e| {
        matches!(e, DataEvent::Response(DataResponse::Instrument(_)))
    })
    .await
    .expect("expected Instrument response");

    if let DataEvent::Response(DataResponse::Instrument(response)) = event {
        assert_eq!(response.instrument_id, eth_perp_id());
    }

    client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test]
async fn test_request_book_snapshot_emits_response() {
    let (addr, _state) = start_server().await;
    let (mut client, mut rx) = build_client(build_config(addr));

    client.connect().await.expect("connect");
    drain_pending(&mut rx);

    client
        .request_book_snapshot(RequestBookSnapshot::new(
            eth_perp_id(),
            NonZeroUsize::new(25),
            Some(client_id()),
            UUID4::new(),
            UnixNanos::default(),
            None,
        ))
        .expect("request_book_snapshot");

    let event = next_event_matching(&mut rx, Duration::from_secs(2), |e| {
        matches!(e, DataEvent::Response(DataResponse::Book(_)))
    })
    .await
    .expect("expected Book response");

    if let DataEvent::Response(DataResponse::Book(response)) = event {
        assert_eq!(response.instrument_id, eth_perp_id());
        let book = &response.data;
        assert!(book.best_bid_price().is_some(), "expected at least one bid");
        assert!(book.best_ask_price().is_some(), "expected at least one ask");
    }

    client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test]
async fn test_request_bars_emits_response() {
    let (addr, _state) = start_server().await;
    let (mut client, mut rx) = build_client(build_config(addr));

    client.connect().await.expect("connect");
    drain_pending(&mut rx);

    let bar_type = BarType::new(
        eth_perp_id(),
        BarSpecification::new(1, BarAggregation::Minute, PriceType::Last),
        AggregationSource::External,
    );
    let start = Utc
        .timestamp_millis_opt(1_700_000_000_000)
        .single()
        .unwrap();
    let end = Utc
        .timestamp_millis_opt(1_700_000_120_000)
        .single()
        .unwrap();

    client
        .request_bars(RequestBars::new(
            bar_type,
            Some(start),
            Some(end),
            NonZeroUsize::new(2),
            Some(client_id()),
            UUID4::new(),
            UnixNanos::default(),
            None,
        ))
        .expect("request_bars");

    let event = next_event_matching(&mut rx, Duration::from_secs(2), |e| {
        matches!(e, DataEvent::Response(DataResponse::Bars(_)))
    })
    .await
    .expect("expected Bars response");

    if let DataEvent::Response(DataResponse::Bars(response)) = event {
        assert_eq!(response.bar_type, bar_type);
        assert_eq!(response.data.len(), 2);
    }

    client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test]
async fn test_request_funding_rates_emits_response() {
    let (addr, _state) = start_server().await;
    let (mut client, mut rx) = build_client(build_config(addr));

    client.connect().await.expect("connect");
    drain_pending(&mut rx);

    let start = Utc
        .timestamp_millis_opt(1_778_702_400_000)
        .single()
        .unwrap();
    let end = Utc
        .timestamp_millis_opt(1_778_706_000_000)
        .single()
        .unwrap();

    client
        .request_funding_rates(RequestFundingRates::new(
            eth_perp_id(),
            Some(start),
            Some(end),
            NonZeroUsize::new(2),
            Some(client_id()),
            UUID4::new(),
            UnixNanos::default(),
            None,
        ))
        .expect("request_funding_rates");

    let event = next_event_matching(&mut rx, Duration::from_secs(2), |e| {
        matches!(e, DataEvent::Response(DataResponse::FundingRates(_)))
    })
    .await
    .expect("expected FundingRates response");

    if let DataEvent::Response(DataResponse::FundingRates(response)) = event {
        assert_eq!(response.instrument_id, eth_perp_id());
        assert_eq!(response.data.len(), 2);
    }

    client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test]
async fn test_request_trades_emits_response() {
    let (addr, _state) = start_server().await;
    let (mut client, mut rx) = build_client(build_config(addr));

    client.connect().await.expect("connect");
    drain_pending(&mut rx);

    client
        .request_trades(RequestTrades::new(
            eth_perp_id(),
            None,
            None,
            NonZeroUsize::new(50),
            Some(client_id()),
            UUID4::new(),
            UnixNanos::default(),
            None,
        ))
        .expect("request_trades");

    let event = next_event_matching(&mut rx, Duration::from_secs(2), |e| {
        matches!(e, DataEvent::Response(DataResponse::Trades(_)))
    })
    .await
    .expect("expected Trades response");

    if let DataEvent::Response(DataResponse::Trades(response)) = event {
        assert_eq!(response.instrument_id, eth_perp_id());
        assert_eq!(response.data.len(), 1);
    }

    client.disconnect().await.expect("disconnect");
}

#[rstest]
#[tokio::test]
async fn test_request_quotes_unsupported_bails() {
    let (addr, _state) = start_server().await;
    let (client, _rx) = build_client(build_config(addr));

    let err = client
        .request_quotes(RequestQuotes::new(
            eth_perp_id(),
            None,
            None,
            None,
            Some(client_id()),
            UUID4::new(),
            UnixNanos::default(),
            None,
        ))
        .unwrap_err();

    assert!(
        err.to_string()
            .contains("does not support historical quote requests"),
    );
}

#[rstest]
#[tokio::test]
async fn test_request_book_depth_unsupported_bails() {
    let (addr, _state) = start_server().await;
    let (client, _rx) = build_client(build_config(addr));

    let err = client
        .request_book_depth(RequestBookDepth::new(
            eth_perp_id(),
            None,
            None,
            None,
            NonZeroUsize::new(10),
            Some(client_id()),
            UUID4::new(),
            UnixNanos::default(),
            None,
        ))
        .unwrap_err();

    assert!(
        err.to_string()
            .contains("does not support historical order book depth requests"),
    );
}

#[rstest]
#[tokio::test]
async fn test_unsubscribe_bars_is_noop_for_unsupported_resolution() {
    let (addr, _state) = start_server().await;
    let (mut client, _rx) = build_client(build_config(addr));

    // A bar type that does not map to a Lighter resolution must be swallowed
    // by `unsubscribe_bars` rather than bubbled up; the engine routinely
    // tears down stale subscriptions on shutdown without knowing the venue's
    // supported set.
    let bar_type = BarType::new(
        eth_perp_id(),
        BarSpecification::new(3, BarAggregation::Minute, PriceType::Last),
        AggregationSource::External,
    );

    client
        .unsubscribe_bars(&UnsubscribeBars::new(
            bar_type,
            Some(client_id()),
            None,
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ))
        .expect("unsubscribe_bars must not error on unsupported resolution");
}

#[rstest]
#[tokio::test]
async fn test_reconnect_replays_active_subscriptions() {
    // Trigger a server-initiated close after the second subscribe ack and
    // confirm the WS layer reconnects and replays both topics. This pins the
    // reconnect contract through the full DataClient surface (the websocket
    // crate has the same test against the raw client).
    let (addr, state) = start_server().await;
    let (mut client, mut rx) = build_client(build_config(addr));

    client.connect().await.expect("connect");
    drain_pending(&mut rx);

    let instrument_id = eth_perp_id();
    client
        .subscribe_book_deltas(SubscribeBookDeltas::new(
            instrument_id,
            BookType::L2_MBP,
            Some(client_id()),
            None,
            UUID4::new(),
            UnixNanos::default(),
            None,
            false,
            None,
            None,
        ))
        .expect("subscribe_book_deltas");
    await_subscribe_count(&state, 1).await;

    state
        .drop_after_next_subscribe
        .store(true, Ordering::Relaxed);

    client
        .subscribe_trades(SubscribeTrades::new(
            instrument_id,
            Some(client_id()),
            None,
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ))
        .expect("subscribe_trades");
    await_subscribe_count(&state, 2).await;

    // After reconnect the WS layer replays both topics from subscription_args.
    // Total subscribes: original 2 + replay 2 = 4. The replay window is
    // RECONNECT_BASE_BACKOFF (250 ms) + jitter, so a generous timeout is fine.
    wait_until_async(
        || {
            let state = Arc::clone(&state);
            async move { state.subscribes.lock().await.len() >= 4 }
        },
        Duration::from_secs(5),
    )
    .await;

    let subs = state.subscribes().await;
    let book_count = subs
        .iter()
        .filter(|s| s["channel"] == "order_book/0")
        .count();
    let trade_count = subs.iter().filter(|s| s["channel"] == "trade/0").count();
    assert_eq!(book_count, 2, "book subscribe must replay");
    assert_eq!(trade_count, 2, "trade subscribe must replay");

    client.disconnect().await.expect("disconnect");
}
