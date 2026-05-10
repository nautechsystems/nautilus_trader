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

//! Integration tests for the Coinbase data client.

use std::{
    net::SocketAddr,
    num::NonZeroUsize,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};

use axum::{
    Router,
    extract::{
        Query, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    response::{IntoResponse, Json, Response},
    routing::get,
};
use futures_util::StreamExt;
use nautilus_coinbase::{CoinbaseDataClient, CoinbaseDataClientConfig};
use nautilus_common::{
    clients::DataClient,
    live::runner::set_data_event_sender,
    messages::{
        DataEvent, DataResponse,
        data::{
            RequestBars, RequestBookSnapshot, RequestInstrument, RequestInstruments, RequestTrades,
            SubscribeBars, SubscribeBookDeltas, SubscribeFundingRates, SubscribeIndexPrices,
            SubscribeQuotes, SubscribeTrades, UnsubscribeFundingRates, UnsubscribeIndexPrices,
            UnsubscribeInstrument,
        },
    },
    testing::wait_until_async,
};
use nautilus_core::{UUID4, UnixNanos};
use nautilus_model::{
    data::{BarType, Data},
    enums::BookType,
    identifiers::{ClientId, InstrumentId, Venue},
};
use rstest::rstest;
use serde::Deserialize;
use serde_json::{Value, json};

fn data_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test_data")
}

fn load_json(filename: &str) -> Value {
    let content = std::fs::read_to_string(data_path().join(filename))
        .unwrap_or_else(|_| panic!("failed to read {filename}"));
    serde_json::from_str(&content).expect("invalid json")
}

fn load_json_str(filename: &str) -> String {
    std::fs::read_to_string(data_path().join(filename))
        .unwrap_or_else(|_| panic!("failed to read {filename}"))
}

#[derive(Clone)]
struct TestServerState {
    // When true, `handle_product_book` returns 503 instead of the canned JSON.
    book_should_fail: Arc<AtomicBool>,
    book_hit_count: Arc<AtomicUsize>,
    // Counts single-product GETs so derivatives-poll tests can assert that
    // the REST path is reached at least once per poll tick.
    product_hits: Arc<AtomicUsize>,
    // When true, `handle_product` blocks on `product_release` before replying
    // so concurrency tests can deterministically hold a REST call "in flight"
    // while they fire unsubscribe commands. Each successful `add_permits(1)`
    // on the semaphore releases exactly one stalled handler.
    product_stall_enabled: Arc<AtomicBool>,
    product_release: Arc<tokio::sync::Semaphore>,
}

impl Default for TestServerState {
    fn default() -> Self {
        Self {
            book_should_fail: Arc::default(),
            book_hit_count: Arc::default(),
            product_hits: Arc::default(),
            product_stall_enabled: Arc::default(),
            product_release: Arc::new(tokio::sync::Semaphore::new(0)),
        }
    }
}

#[derive(Deserialize)]
struct ProductBookQuery {
    #[allow(dead_code)]
    product_id: Option<String>,
    limit: Option<u32>,
}

async fn handle_products(State(_state): State<TestServerState>) -> impl IntoResponse {
    let products = load_json("http_products.json");
    Json(products)
}

async fn handle_product(
    State(state): State<TestServerState>,
    axum::extract::Path(product_id): axum::extract::Path<String>,
) -> impl IntoResponse {
    state.product_hits.fetch_add(1, Ordering::SeqCst);

    if state.product_stall_enabled.load(Ordering::SeqCst) {
        // Block until the test adds a permit. `forget` keeps the permit
        // count at its released value so each `add_permits(1)` releases
        // exactly one stalled handler.
        if let Ok(permit) = state.product_release.clone().acquire_owned().await {
            permit.forget();
        }
    }

    if product_id == "BTC-USD" {
        let product = load_json("http_product.json");
        Json(product)
    } else if product_id == "BIP-20DEC30-CDE" {
        // Return a perpetual payload with the derivatives fields the poll
        // manager reads. The shared future fixture has two products and
        // doesn't populate funding fields on the PERP row, so splice in
        // explicit values the tests assert against.
        let payload = load_json("http_products_future.json");
        let mut product = payload["products"][0].clone();
        if let Some(details) = product.get_mut("future_product_details") {
            details["index_price"] = json!("79190.103206");
            details["funding_rate"] = json!("0.000004");
            details["funding_time"] = json!("2026-04-22T15:00:00Z");
            details["funding_interval"] = json!("3600s");
        }
        Json(product)
    } else {
        Json(json!({"error": "not found"}))
    }
}

async fn handle_candles(State(_state): State<TestServerState>) -> impl IntoResponse {
    let candles = load_json("http_candles.json");
    Json(candles)
}

async fn handle_ticker(State(_state): State<TestServerState>) -> impl IntoResponse {
    let ticker = load_json("http_ticker.json");
    Json(ticker)
}

async fn handle_product_book(
    State(state): State<TestServerState>,
    Query(query): Query<ProductBookQuery>,
) -> axum::response::Response {
    state.book_hit_count.fetch_add(1, Ordering::SeqCst);

    if state.book_should_fail.load(Ordering::SeqCst) {
        return (
            axum::http::StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": "unavailable"})),
        )
            .into_response();
    }

    let mut book = load_json("http_product_book.json");

    if let Some(limit) = query.limit {
        let limit = limit as usize;

        if let Some(pb) = book.get_mut("pricebook") {
            if let Some(bids) = pb.get_mut("bids").and_then(|v| v.as_array_mut()) {
                bids.truncate(limit);
            }

            if let Some(asks) = pb.get_mut("asks").and_then(|v| v.as_array_mut()) {
                asks.truncate(limit);
            }
        }
    }

    Json(book).into_response()
}

async fn handle_best_bid_ask(State(_state): State<TestServerState>) -> impl IntoResponse {
    Json(json!({
        "pricebooks": [{
            "product_id": "BTC-USD",
            "bids": [{"price": "68923.66", "size": "0.17189468"}],
            "asks": [{"price": "68923.67", "size": "0.16987193"}],
            "time": "2026-04-07T00:28:59.662782Z"
        }]
    }))
}

async fn handle_health() -> impl IntoResponse {
    axum::http::StatusCode::OK
}

async fn handle_ws_upgrade(
    ws: WebSocketUpgrade,
    State(_state): State<TestServerState>,
) -> Response {
    ws.on_upgrade(handle_ws_socket)
}

async fn handle_ws_socket(mut socket: WebSocket) {
    while let Some(message) = socket.next().await {
        let Ok(message) = message else { break };

        match message {
            Message::Text(text) => {
                if let Ok(payload) = serde_json::from_str::<Value>(&text) {
                    let msg_type = payload.get("type").and_then(|t| t.as_str());

                    match msg_type {
                        Some("subscribe") => {
                            let channel = payload
                                .get("channel")
                                .and_then(|c| c.as_str())
                                .unwrap_or("");

                            let data_msg = match channel {
                                "market_trades" => load_json_str("ws_market_trades.json"),
                                "ticker" => load_json_str("ws_ticker.json"),
                                "level2" => load_json_str("ws_l2_data_snapshot.json"),
                                "candles" => load_json_str("ws_candles.json"),
                                _ => json!({"channel": channel}).to_string(),
                            };

                            if socket.send(Message::Text(data_msg.into())).await.is_err() {
                                break;
                            }
                        }
                        Some("unsubscribe") => {}
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

const API_PREFIX: &str = "/api/v3/brokerage";

fn create_test_router(state: TestServerState) -> Router {
    Router::new()
        .route(
            &format!("{API_PREFIX}/market/products"),
            get(handle_products),
        )
        .route(
            &format!("{API_PREFIX}/market/products/{{product_id}}"),
            get(handle_product),
        )
        .route(
            &format!("{API_PREFIX}/market/products/{{product_id}}/candles"),
            get(handle_candles),
        )
        .route(
            &format!("{API_PREFIX}/market/products/{{product_id}}/ticker"),
            get(handle_ticker),
        )
        .route(
            &format!("{API_PREFIX}/market/product_book"),
            get(handle_product_book),
        )
        .route(
            &format!("{API_PREFIX}/best_bid_ask"),
            get(handle_best_bid_ask),
        )
        .route("/health", get(handle_health))
        .route("/ws", get(handle_ws_upgrade))
        .with_state(state)
}

async fn start_mock_server(state: TestServerState) -> SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let router = create_test_router(state);

    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });

    // Wait for server to accept connections
    let start = std::time::Instant::now();

    loop {
        if tokio::net::TcpStream::connect(addr).await.is_ok() {
            break;
        }

        assert!(
            start.elapsed() <= Duration::from_secs(5),
            "Mock server did not start within timeout"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    addr
}

fn create_data_client_config(addr: SocketAddr) -> CoinbaseDataClientConfig {
    CoinbaseDataClientConfig {
        base_url_rest: Some(format!("http://{addr}")),
        base_url_ws: Some(format!("ws://{addr}/ws")),
        ..CoinbaseDataClientConfig::default()
    }
}

#[rstest]
#[tokio::test]
async fn test_data_client_connect_disconnect() {
    let state = TestServerState::default();
    let addr = start_mock_server(state).await;
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    set_data_event_sender(tx);

    let config = create_data_client_config(addr);
    let mut client = CoinbaseDataClient::new(ClientId::new("COINBASE"), config).unwrap();
    assert!(!client.is_connected());

    client.connect().await.unwrap();
    assert!(client.is_connected());

    client.disconnect().await.unwrap();
    assert!(!client.is_connected());
}

#[rstest]
#[tokio::test]
async fn test_data_client_connect_is_idempotent() {
    let state = TestServerState::default();
    let addr = start_mock_server(state).await;
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    set_data_event_sender(tx);

    let config = create_data_client_config(addr);
    let mut client = CoinbaseDataClient::new(ClientId::new("COINBASE"), config).unwrap();

    client.connect().await.unwrap();
    assert!(client.is_connected());

    client.connect().await.unwrap();
    assert!(client.is_connected());

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_data_client_emits_instruments_on_connect() {
    let state = TestServerState::default();
    let addr = start_mock_server(state).await;
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    set_data_event_sender(tx);

    let config = create_data_client_config(addr);
    let mut client = CoinbaseDataClient::new(ClientId::new("COINBASE"), config).unwrap();
    client.connect().await.unwrap();

    let mut instrument_count = 0;

    while let Ok(event) = rx.try_recv() {
        if matches!(event, DataEvent::Instrument(_)) {
            instrument_count += 1;
        }
    }

    assert!(
        instrument_count > 0,
        "Expected instrument events on connect"
    );

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_data_client_reset_clears_state() {
    let state = TestServerState::default();
    let addr = start_mock_server(state).await;
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    set_data_event_sender(tx);

    let config = create_data_client_config(addr);
    let mut client = CoinbaseDataClient::new(ClientId::new("COINBASE"), config).unwrap();

    client.reset().unwrap();
    assert!(!client.is_connected());

    client.connect().await.unwrap();
    assert!(client.is_connected());

    client.reset().unwrap();
    assert!(!client.is_connected());
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn test_data_client_subscribe_trades() {
    let state = TestServerState::default();
    let addr = start_mock_server(state).await;
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    set_data_event_sender(tx);

    let config = create_data_client_config(addr);
    let mut client = CoinbaseDataClient::new(ClientId::new("COINBASE"), config).unwrap();
    client.connect().await.unwrap();

    while rx.try_recv().is_ok() {}

    let instrument_id = InstrumentId::from("BTC-USD.COINBASE");
    let cmd = SubscribeTrades::new(
        instrument_id,
        Some(ClientId::new("COINBASE")),
        None,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );
    client.subscribe_trades(cmd).unwrap();

    wait_until_async(
        || {
            let found = loop {
                match rx.try_recv() {
                    Ok(DataEvent::Data(Data::Trade(_))) => break true,
                    Ok(_) => {}
                    Err(_) => break false,
                }
            };
            async move { found }
        },
        Duration::from_secs(5),
    )
    .await;

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn test_data_client_subscribe_quotes() {
    let state = TestServerState::default();
    let addr = start_mock_server(state).await;
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    set_data_event_sender(tx);

    let config = create_data_client_config(addr);
    let mut client = CoinbaseDataClient::new(ClientId::new("COINBASE"), config).unwrap();
    client.connect().await.unwrap();

    while rx.try_recv().is_ok() {}

    let instrument_id = InstrumentId::from("BTC-USD.COINBASE");
    let cmd = SubscribeQuotes::new(
        instrument_id,
        Some(ClientId::new("COINBASE")),
        None,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );
    client.subscribe_quotes(cmd).unwrap();

    let event = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("timeout waiting for quote event")
        .expect("channel closed");

    assert!(
        matches!(event, DataEvent::Data(Data::Quote(_))),
        "Expected Quote event, was: {event:?}"
    );

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn test_data_client_subscribe_book_deltas() {
    let state = TestServerState::default();
    let addr = start_mock_server(state).await;
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    set_data_event_sender(tx);

    let config = create_data_client_config(addr);
    let mut client = CoinbaseDataClient::new(ClientId::new("COINBASE"), config).unwrap();
    client.connect().await.unwrap();

    while rx.try_recv().is_ok() {}

    let instrument_id = InstrumentId::from("BTC-USD.COINBASE");
    let cmd = SubscribeBookDeltas::new(
        instrument_id,
        BookType::L2_MBP,
        Some(ClientId::new("COINBASE")),
        None,
        UUID4::new(),
        UnixNanos::default(),
        None,
        false,
        None,
        None,
    );
    client.subscribe_book_deltas(cmd).unwrap();

    let event = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("timeout waiting for book deltas event")
        .expect("channel closed");

    assert!(
        matches!(event, DataEvent::Data(Data::Deltas(_))),
        "Expected Deltas event, was: {event:?}"
    );

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn test_data_client_request_instruments() {
    let state = TestServerState::default();
    let addr = start_mock_server(state).await;
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    set_data_event_sender(tx);

    let config = create_data_client_config(addr);
    let mut client = CoinbaseDataClient::new(ClientId::new("COINBASE"), config).unwrap();
    client.connect().await.unwrap();

    tokio::time::sleep(Duration::from_millis(500)).await;

    while rx.try_recv().is_ok() {}

    let request = RequestInstruments::new(
        None,
        None,
        Some(ClientId::new("COINBASE")),
        Some(Venue::new("COINBASE")),
        UUID4::new(),
        UnixNanos::default(),
        None,
    );
    client.request_instruments(request).unwrap();

    let event = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("timeout waiting for instruments response")
        .expect("channel closed");

    assert!(
        matches!(event, DataEvent::Response(DataResponse::Instruments(_))),
        "Expected Instruments response, was: {event:?}"
    );

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn test_data_client_request_instrument() {
    let state = TestServerState::default();
    let addr = start_mock_server(state).await;
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    set_data_event_sender(tx);

    let config = create_data_client_config(addr);
    let mut client = CoinbaseDataClient::new(ClientId::new("COINBASE"), config).unwrap();
    client.connect().await.unwrap();

    tokio::time::sleep(Duration::from_millis(500)).await;

    while rx.try_recv().is_ok() {}

    let instrument_id = InstrumentId::from("BTC-USD.COINBASE");
    let request = RequestInstrument::new(
        instrument_id,
        None,
        None,
        Some(ClientId::new("COINBASE")),
        UUID4::new(),
        UnixNanos::default(),
        None,
    );
    client.request_instrument(request).unwrap();

    let event = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("timeout waiting for instrument response")
        .expect("channel closed");

    assert!(
        matches!(event, DataEvent::Response(DataResponse::Instrument(_))),
        "Expected Instrument response, was: {event:?}"
    );

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn test_data_client_request_book_snapshot() {
    let state = TestServerState::default();
    let addr = start_mock_server(state).await;
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    set_data_event_sender(tx);

    let config = create_data_client_config(addr);
    let mut client = CoinbaseDataClient::new(ClientId::new("COINBASE"), config).unwrap();
    client.connect().await.unwrap();

    tokio::time::sleep(Duration::from_millis(500)).await;

    while rx.try_recv().is_ok() {}

    let instrument_id = InstrumentId::from("BTC-USD.COINBASE");
    let request = RequestBookSnapshot::new(
        instrument_id,
        None,
        Some(ClientId::new("COINBASE")),
        UUID4::new(),
        UnixNanos::default(),
        None,
    );
    client.request_book_snapshot(request).unwrap();

    let event = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("timeout waiting for book snapshot response")
        .expect("channel closed");

    match event {
        DataEvent::Response(DataResponse::Book(book_response)) => {
            assert_eq!(book_response.instrument_id, instrument_id);
            let book = &book_response.data;
            assert!(book.best_bid_price().is_some(), "book should have bids");
            assert!(book.best_ask_price().is_some(), "book should have asks");
        }
        other => panic!("Expected Book response, was: {other:?}"),
    }

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn test_data_client_request_book_snapshot_with_depth() {
    let state = TestServerState::default();
    let addr = start_mock_server(state).await;
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    set_data_event_sender(tx);

    let config = create_data_client_config(addr);
    let mut client = CoinbaseDataClient::new(ClientId::new("COINBASE"), config).unwrap();
    client.connect().await.unwrap();

    tokio::time::sleep(Duration::from_millis(500)).await;

    while rx.try_recv().is_ok() {}

    let instrument_id = InstrumentId::from("BTC-USD.COINBASE");
    let request = RequestBookSnapshot::new(
        instrument_id,
        Some(NonZeroUsize::new(2).unwrap()),
        Some(ClientId::new("COINBASE")),
        UUID4::new(),
        UnixNanos::default(),
        None,
    );
    client.request_book_snapshot(request).unwrap();

    let event = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("timeout waiting for book snapshot response")
        .expect("channel closed");

    match event {
        DataEvent::Response(DataResponse::Book(book_response)) => {
            let book = &book_response.data;

            // The fixture has 5 levels per side; depth=2 should limit to 2
            let bid_count = book.bids(None).count();
            let ask_count = book.asks(None).count();
            assert_eq!(bid_count, 2, "should have 2 bid levels with depth=2");
            assert_eq!(ask_count, 2, "should have 2 ask levels with depth=2");
        }
        other => panic!("Expected Book response, was: {other:?}"),
    }

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn test_data_client_request_bars() {
    let state = TestServerState::default();
    let addr = start_mock_server(state).await;
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    set_data_event_sender(tx);

    let config = create_data_client_config(addr);
    let mut client = CoinbaseDataClient::new(ClientId::new("COINBASE"), config).unwrap();
    client.connect().await.unwrap();

    tokio::time::sleep(Duration::from_millis(500)).await;

    while rx.try_recv().is_ok() {}

    let bar_type = BarType::from("BTC-USD.COINBASE-1-HOUR-LAST-EXTERNAL");
    let request = RequestBars::new(
        bar_type,
        None,
        None,
        None,
        Some(ClientId::new("COINBASE")),
        UUID4::new(),
        UnixNanos::default(),
        None,
    );
    client.request_bars(request).unwrap();

    let event = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("timeout waiting for bars response")
        .expect("channel closed");

    match event {
        DataEvent::Response(DataResponse::Bars(bars_response)) => {
            assert_eq!(bars_response.bar_type, bar_type);
            assert!(!bars_response.data.is_empty(), "should have bars");

            // Bars should be sorted ascending by ts_event
            let events: Vec<_> = bars_response.data.iter().map(|b| b.ts_event).collect();

            for window in events.windows(2) {
                assert!(window[0] <= window[1], "bars not sorted ascending");
            }
        }
        other => panic!("Expected Bars response, was: {other:?}"),
    }

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn test_data_client_request_trades() {
    let state = TestServerState::default();
    let addr = start_mock_server(state).await;
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    set_data_event_sender(tx);

    let config = create_data_client_config(addr);
    let mut client = CoinbaseDataClient::new(ClientId::new("COINBASE"), config).unwrap();
    client.connect().await.unwrap();

    tokio::time::sleep(Duration::from_millis(500)).await;

    while rx.try_recv().is_ok() {}

    let instrument_id = InstrumentId::from("BTC-USD.COINBASE");
    let request = RequestTrades::new(
        instrument_id,
        None,
        None,
        None,
        Some(ClientId::new("COINBASE")),
        UUID4::new(),
        UnixNanos::default(),
        None,
    );
    client.request_trades(request).unwrap();

    let event = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("timeout waiting for trades response")
        .expect("channel closed");

    match event {
        DataEvent::Response(DataResponse::Trades(trades_response)) => {
            assert_eq!(trades_response.instrument_id, instrument_id);
            assert!(!trades_response.data.is_empty(), "should have trades");

            // Trades should be sorted ascending by ts_event
            let events: Vec<_> = trades_response.data.iter().map(|t| t.ts_event).collect();

            for window in events.windows(2) {
                assert!(window[0] <= window[1], "trades not sorted ascending");
            }
        }
        other => panic!("Expected Trades response, was: {other:?}"),
    }

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_data_client_unsubscribe_instrument_is_noop() {
    let state = TestServerState::default();
    let addr = start_mock_server(state).await;
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    set_data_event_sender(tx);

    let config = create_data_client_config(addr);
    let mut client = CoinbaseDataClient::new(ClientId::new("COINBASE"), config).unwrap();
    client.connect().await.unwrap();

    while rx.try_recv().is_ok() {}

    let cmd = UnsubscribeInstrument::new(
        InstrumentId::from("BTC-USD.COINBASE"),
        Some(ClientId::new("COINBASE")),
        None,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );

    client
        .unsubscribe_instrument(&cmd)
        .expect("unsubscribe_instrument should be a no-op Ok");

    assert!(
        rx.try_recv().is_err(),
        "unsubscribe_instrument should not emit any data events"
    );

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn test_data_client_subscribe_bars() {
    let state = TestServerState::default();
    let addr = start_mock_server(state).await;
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    set_data_event_sender(tx);

    let config = create_data_client_config(addr);
    let mut client = CoinbaseDataClient::new(ClientId::new("COINBASE"), config).unwrap();
    client.connect().await.unwrap();

    while rx.try_recv().is_ok() {}

    let bar_type = BarType::from("BTC-USD.COINBASE-5-MINUTE-LAST-EXTERNAL");
    let cmd = SubscribeBars::new(
        bar_type,
        Some(ClientId::new("COINBASE")),
        None,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );
    client.subscribe_bars(cmd).unwrap();

    wait_until_async(
        || {
            let found = loop {
                match rx.try_recv() {
                    Ok(DataEvent::Data(Data::Bar(_))) => break true,
                    Ok(_) => {}
                    Err(_) => break false,
                }
            };
            async move { found }
        },
        Duration::from_secs(5),
    )
    .await;

    client.disconnect().await.unwrap();
}

// Data-client historical requests spawn outside the cancellation token, so
// they must not retry. Force 503s and assert exactly one attempt.
#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn test_data_client_request_book_snapshot_does_not_retry_on_failure() {
    let state = TestServerState::default();
    state.book_should_fail.store(true, Ordering::SeqCst);
    let hit_count = state.book_hit_count.clone();

    let addr = start_mock_server(state).await;
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    set_data_event_sender(tx);

    let config = create_data_client_config(addr);
    let mut client = CoinbaseDataClient::new(ClientId::new("COINBASE"), config).unwrap();
    client.connect().await.unwrap();

    let instrument_id = InstrumentId::from("BTC-USD.COINBASE");
    let request = RequestBookSnapshot::new(
        instrument_id,
        None,
        Some(ClientId::new("COINBASE")),
        UUID4::new(),
        UnixNanos::default(),
        None,
    );
    client.request_book_snapshot(request).unwrap();

    tokio::time::sleep(Duration::from_millis(500)).await;

    let attempts = hit_count.load(Ordering::SeqCst);
    assert_eq!(
        attempts, 1,
        "data-client historical requests must not retry; saw {attempts}"
    );

    client.disconnect().await.unwrap();
}

// Helper: build the deriv-poll subscribe/unsubscribe commands for the
// lifecycle tests below. Keeps the individual tests readable.
fn subscribe_index_cmd(instrument_id: InstrumentId) -> SubscribeIndexPrices {
    SubscribeIndexPrices::new(
        instrument_id,
        Some(ClientId::new("COINBASE")),
        None,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    )
}

fn subscribe_funding_cmd(instrument_id: InstrumentId) -> SubscribeFundingRates {
    SubscribeFundingRates::new(
        instrument_id,
        Some(ClientId::new("COINBASE")),
        None,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    )
}

fn unsubscribe_index_cmd(instrument_id: InstrumentId) -> UnsubscribeIndexPrices {
    UnsubscribeIndexPrices::new(
        instrument_id,
        Some(ClientId::new("COINBASE")),
        None,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    )
}

fn unsubscribe_funding_cmd(instrument_id: InstrumentId) -> UnsubscribeFundingRates {
    UnsubscribeFundingRates::new(
        instrument_id,
        Some(ClientId::new("COINBASE")),
        None,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    )
}

// Use a short poll interval for the lifecycle tests so they don't have to
// wait 15 s (the client's default) to observe events.
fn create_deriv_data_client_config(addr: SocketAddr) -> CoinbaseDataClientConfig {
    CoinbaseDataClientConfig {
        base_url_rest: Some(format!("http://{addr}")),
        base_url_ws: Some(format!("ws://{addr}/ws")),
        derivatives_poll_interval_secs: 1,
        ..CoinbaseDataClientConfig::default()
    }
}

// End-to-end happy path: subscribe index + funding, observe both event
// kinds on the shared poll, unsubscribe both, observe the poll quiesces.
#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn test_data_client_subscribe_index_and_funding_emits_both_kinds() {
    use nautilus_model::data::{Data, FundingRateUpdate, IndexPriceUpdate};

    let state = TestServerState::default();
    let product_hits = state.product_hits.clone();
    let addr = start_mock_server(state).await;
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    set_data_event_sender(tx);

    let config = create_deriv_data_client_config(addr);
    let mut client = CoinbaseDataClient::new(ClientId::new("COINBASE"), config).unwrap();
    client.connect().await.unwrap();

    while rx.try_recv().is_ok() {}
    let baseline_hits = product_hits.load(Ordering::SeqCst);

    let instrument_id = InstrumentId::from("BIP-20DEC30-CDE.COINBASE");
    client
        .subscribe_index_prices(subscribe_index_cmd(instrument_id))
        .unwrap();
    client
        .subscribe_funding_rates(subscribe_funding_cmd(instrument_id))
        .unwrap();

    let mut got_index: Option<IndexPriceUpdate> = None;
    let mut got_funding: Option<FundingRateUpdate> = None;

    wait_until_async(
        || {
            while let Ok(evt) = rx.try_recv() {
                match evt {
                    DataEvent::Data(Data::IndexPriceUpdate(ip)) => got_index = Some(ip),
                    DataEvent::FundingRate(fr) => got_funding = Some(fr),
                    _ => {}
                }
            }
            let done = got_index.is_some() && got_funding.is_some();
            async move { done }
        },
        Duration::from_secs(5),
    )
    .await;

    let ip = got_index.expect("IndexPriceUpdate emitted from poll");
    assert_eq!(ip.instrument_id, instrument_id);
    assert_eq!(
        ip.value.as_decimal().to_string(),
        "79190.103206",
        "index price preserved at full precision"
    );

    let fr = got_funding.expect("FundingRateUpdate emitted from poll");
    assert_eq!(fr.instrument_id, instrument_id);
    assert_eq!(fr.rate.to_string(), "0.000004");
    assert_eq!(fr.interval, Some(60));
    assert!(fr.next_funding_ns.is_some());

    // At least one REST poll landed against the mock. The bootstrap emits
    // a few calls before we record the baseline, so use a relative check.
    let hits_after = product_hits.load(Ordering::SeqCst);
    assert!(
        hits_after > baseline_hits,
        "poll must hit /market/products/{{id}} at least once after subscribe"
    );

    // Unsubscribing both flags cancels the shared task; no more events
    // should arrive after a short drain.
    client
        .unsubscribe_index_prices(&unsubscribe_index_cmd(instrument_id))
        .unwrap();
    client
        .unsubscribe_funding_rates(&unsubscribe_funding_cmd(instrument_id))
        .unwrap();

    tokio::time::sleep(Duration::from_millis(1500)).await;

    while rx.try_recv().is_ok() {}

    let hits_before_idle = product_hits.load(Ordering::SeqCst);
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert_eq!(
        product_hits.load(Ordering::SeqCst),
        hits_before_idle,
        "poll must stop hitting the REST endpoint after last unsubscribe"
    );

    client.disconnect().await.unwrap();
}

// Lifecycle regression: disconnect() must stop poll tasks but preserve
// the subscription set, and a subsequent connect() must resume emissions.
// The data engine's adapter suppresses duplicate subscribe commands, so
// any silent drop of the state here would leave the stream dark.
#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn test_data_client_reconnect_resumes_derivatives_polls() {
    use nautilus_model::data::Data;

    let state = TestServerState::default();
    let addr = start_mock_server(state).await;
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    set_data_event_sender(tx);

    let config = create_deriv_data_client_config(addr);
    let mut client = CoinbaseDataClient::new(ClientId::new("COINBASE"), config).unwrap();
    client.connect().await.unwrap();

    while rx.try_recv().is_ok() {}

    let instrument_id = InstrumentId::from("BIP-20DEC30-CDE.COINBASE");
    client
        .subscribe_index_prices(subscribe_index_cmd(instrument_id))
        .unwrap();

    // Wait for the first index update so we know the initial subscribe is
    // fully live before toggling the connection.
    wait_until_async(
        || {
            let mut seen = false;

            while let Ok(evt) = rx.try_recv() {
                if matches!(evt, DataEvent::Data(Data::IndexPriceUpdate(_))) {
                    seen = true;
                }
            }
            async move { seen }
        },
        Duration::from_secs(5),
    )
    .await;

    client.disconnect().await.unwrap();

    while rx.try_recv().is_ok() {}

    client.connect().await.unwrap();

    while rx.try_recv().is_ok() {}

    let mut resumed = false;
    wait_until_async(
        || {
            while let Ok(evt) = rx.try_recv() {
                if matches!(evt, DataEvent::Data(Data::IndexPriceUpdate(_))) {
                    resumed = true;
                }
            }
            async move { resumed }
        },
        Duration::from_secs(5),
    )
    .await;
    assert!(
        resumed,
        "index-price poll must resume after disconnect + connect"
    );

    client.disconnect().await.unwrap();
}

// `stop()` must shut down the derivatives poll alongside the rest of the
// client so no REST calls continue after the stop signal.
#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn test_data_client_stop_halts_derivatives_poll() {
    let state = TestServerState::default();
    let product_hits = state.product_hits.clone();
    let addr = start_mock_server(state).await;
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    set_data_event_sender(tx);

    let config = create_deriv_data_client_config(addr);
    let mut client = CoinbaseDataClient::new(ClientId::new("COINBASE"), config).unwrap();
    client.connect().await.unwrap();

    let instrument_id = InstrumentId::from("BIP-20DEC30-CDE.COINBASE");
    client
        .subscribe_index_prices(subscribe_index_cmd(instrument_id))
        .unwrap();

    // Let at least one poll tick hit the mock server.
    tokio::time::sleep(Duration::from_millis(1500)).await;
    let hits_before_stop = product_hits.load(Ordering::SeqCst);
    assert!(
        hits_before_stop > 0,
        "poll must hit the endpoint at least once before stop"
    );

    client.stop().unwrap();
    let hits_at_stop = product_hits.load(Ordering::SeqCst);

    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert_eq!(
        product_hits.load(Ordering::SeqCst),
        hits_at_stop,
        "stop() must halt derivatives polling"
    );
}

// Concurrency fix regression: when a REST poll is in flight and the caller
// unsubscribes one of two active kinds, the post-await flag recheck must
// mask the dropped kind so only the remaining one emits. Pre-fix, the
// task re-entered emit_deriv_updates with the flags it read before the
// HTTP await started and still published both events.
#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn test_data_client_unsubscribe_during_inflight_poll_masks_dropped_kind() {
    use nautilus_model::data::Data;

    let state = TestServerState::default();
    let product_stall_enabled = state.product_stall_enabled.clone();
    let product_release = state.product_release.clone();
    let product_hits = state.product_hits.clone();
    let addr = start_mock_server(state).await;
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    set_data_event_sender(tx);

    let config = create_deriv_data_client_config(addr);
    let mut client = CoinbaseDataClient::new(ClientId::new("COINBASE"), config).unwrap();
    client.connect().await.unwrap();

    while rx.try_recv().is_ok() {}

    let instrument_id = InstrumentId::from("BIP-20DEC30-CDE.COINBASE");
    client
        .subscribe_index_prices(subscribe_index_cmd(instrument_id))
        .unwrap();
    client
        .subscribe_funding_rates(subscribe_funding_cmd(instrument_id))
        .unwrap();

    // Let the initial baseline poll drain through (stall is off here).
    wait_until_async(
        || {
            let mut saw_index = false;
            let mut saw_funding = false;

            while let Ok(evt) = rx.try_recv() {
                match evt {
                    DataEvent::Data(Data::IndexPriceUpdate(_)) => saw_index = true,
                    DataEvent::FundingRate(_) => saw_funding = true,
                    _ => {}
                }
            }
            let done = saw_index && saw_funding;
            async move { done }
        },
        Duration::from_secs(5),
    )
    .await;

    // Turn on the stall and wait for the next poll tick to get parked
    // inside handle_product. product_hits crossing its current value
    // signals the REST call has started and is blocking.
    let baseline_hits = product_hits.load(Ordering::SeqCst);
    product_stall_enabled.store(true, Ordering::SeqCst);
    wait_until_async(
        || {
            let done = product_hits.load(Ordering::SeqCst) > baseline_hits;
            async move { done }
        },
        Duration::from_secs(5),
    )
    .await;

    // The poll is now blocked inside request_raw_product. Unsubscribe
    // index; the funding subscription stays active so the task should
    // survive and only emit a funding update on this tick.
    client
        .unsubscribe_index_prices(&unsubscribe_index_cmd(instrument_id))
        .unwrap();

    // Release the stalled REST response.
    product_release.add_permits(1);

    // Expect a FundingRateUpdate from the released response; must never
    // see an IndexPriceUpdate because the recheck should have masked it.
    let mut saw_funding = false;
    let mut saw_index_after_unsubscribe = false;
    wait_until_async(
        || {
            while let Ok(evt) = rx.try_recv() {
                match evt {
                    DataEvent::FundingRate(_) => saw_funding = true,
                    DataEvent::Data(Data::IndexPriceUpdate(_)) => {
                        saw_index_after_unsubscribe = true;
                    }
                    _ => {}
                }
            }
            async move { saw_funding }
        },
        Duration::from_secs(5),
    )
    .await;

    assert!(saw_funding, "funding must emit after the stall releases");
    assert!(
        !saw_index_after_unsubscribe,
        "post-await recheck must mask the kind that was unsubscribed mid-poll"
    );

    // Turn stall off so disconnect can complete promptly.
    product_stall_enabled.store(false, Ordering::SeqCst);
    client.disconnect().await.unwrap();
}

// Concurrency fix regression: when the last remaining kind is
// unsubscribed while a poll is in flight, the task must exit cleanly via
// the `None` match on the post-await lookup (entry removed) and emit
// nothing. The inner select! also lets the cancel preempt, so either
// path is acceptable: the assertion is behavioural (no event, task
// gone) rather than path-specific.
#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn test_data_client_unsubscribe_last_kind_during_inflight_poll_emits_nothing() {
    use nautilus_model::data::Data;

    let state = TestServerState::default();
    let product_stall_enabled = state.product_stall_enabled.clone();
    let product_release = state.product_release.clone();
    let product_hits = state.product_hits.clone();
    let addr = start_mock_server(state).await;
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    set_data_event_sender(tx);

    let config = create_deriv_data_client_config(addr);
    let mut client = CoinbaseDataClient::new(ClientId::new("COINBASE"), config).unwrap();
    client.connect().await.unwrap();

    while rx.try_recv().is_ok() {}

    let instrument_id = InstrumentId::from("BIP-20DEC30-CDE.COINBASE");
    client
        .subscribe_index_prices(subscribe_index_cmd(instrument_id))
        .unwrap();

    // Wait for one successful poll so we know the task is alive.
    wait_until_async(
        || {
            let mut seen = false;

            while let Ok(evt) = rx.try_recv() {
                if matches!(evt, DataEvent::Data(Data::IndexPriceUpdate(_))) {
                    seen = true;
                }
            }
            async move { seen }
        },
        Duration::from_secs(5),
    )
    .await;

    // Stall the next poll response.
    let baseline_hits = product_hits.load(Ordering::SeqCst);
    product_stall_enabled.store(true, Ordering::SeqCst);
    wait_until_async(
        || {
            let done = product_hits.load(Ordering::SeqCst) > baseline_hits;
            async move { done }
        },
        Duration::from_secs(5),
    )
    .await;

    // Drop the last active flag while the REST call is parked.
    client
        .unsubscribe_index_prices(&unsubscribe_index_cmd(instrument_id))
        .unwrap();

    // Release the stalled response and give the task a moment to unwind.
    product_release.add_permits(1);
    tokio::time::sleep(Duration::from_millis(200)).await;

    // No IndexPriceUpdate and no FundingRateUpdate must land after the
    // unsubscribe.
    let mut stale_event = false;

    while let Ok(evt) = rx.try_recv() {
        match evt {
            DataEvent::Data(Data::IndexPriceUpdate(_)) | DataEvent::FundingRate(_) => {
                stale_event = true;
            }
            _ => {}
        }
    }
    assert!(
        !stale_event,
        "no event may be emitted for an instrument whose last flag was dropped mid-poll"
    );

    product_stall_enabled.store(false, Ordering::SeqCst);
    client.disconnect().await.unwrap();
}
