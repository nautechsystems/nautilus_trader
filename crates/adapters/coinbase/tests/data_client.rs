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

use std::{net::SocketAddr, num::NonZeroUsize, path::PathBuf, time::Duration};

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
            SubscribeBars, SubscribeBookDeltas, SubscribeQuotes, SubscribeTrades,
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

#[derive(Clone, Default)]
struct TestServerState {}

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
    State(_state): State<TestServerState>,
    axum::extract::Path(product_id): axum::extract::Path<String>,
) -> impl IntoResponse {
    if product_id == "BTC-USD" {
        let product = load_json("http_product.json");
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
    State(_state): State<TestServerState>,
    Query(query): Query<ProductBookQuery>,
) -> impl IntoResponse {
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

    Json(book)
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
        .route(&format!("{API_PREFIX}/products"), get(handle_products))
        .route(
            &format!("{API_PREFIX}/products/{{product_id}}"),
            get(handle_product),
        )
        .route(
            &format!("{API_PREFIX}/products/{{product_id}}/candles"),
            get(handle_candles),
        )
        .route(
            &format!("{API_PREFIX}/products/{{product_id}}/ticker"),
            get(handle_ticker),
        )
        .route(
            &format!("{API_PREFIX}/product_book"),
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
    client.subscribe_trades(&cmd).unwrap();

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
    client.subscribe_quotes(&cmd).unwrap();

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
    client.subscribe_book_deltas(&cmd).unwrap();

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
    client.subscribe_bars(&cmd).unwrap();

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
