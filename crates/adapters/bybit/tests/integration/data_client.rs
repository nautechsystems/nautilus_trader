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

//! Integration tests for `BybitDataClient`.
//!
//! These tests verify the full data flow from WebSocket messages through
//! parsing to event emission via the data event channel.

use std::{
    collections::HashMap,
    net::SocketAddr,
    num::NonZeroUsize,
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
use nautilus_bybit::{
    common::{
        consts::{BYBIT_CLIENT_ID, BYBIT_VENUE},
        enums::{BybitEnvironment, BybitProductType},
    },
    config::BybitDataClientConfig,
    data::BybitDataClient,
};
use nautilus_common::{
    clients::DataClient,
    live::runner::{replace_system_event_sender, set_data_event_sender},
    messages::{
        DataEvent, SystemEvent,
        data::{
            DataResponse, RequestBookSnapshot, RequestFundingRates, RequestInstrument,
            RequestInstruments, RequestOptionChainReferencePrice, SubscribeBookDeltas,
            SubscribeQuotes, SubscribeTrades, UnsubscribeBookDeltas, UnsubscribeQuotes,
        },
        system::SocketState,
    },
    testing::wait_until_async,
};
use nautilus_core::{Params, UUID4, UnixNanos, time::get_atomic_clock_realtime};
use nautilus_live::{SocketReconnectRegistry, SocketReconnectRequestOutcome};
use nautilus_model::{
    data::Data,
    enums::{BookAction, BookType},
    identifiers::{InstrumentId, OptionSeriesId},
    types::Price,
};
use nautilus_network::http::HttpClient;
use rstest::rstest;
use rust_decimal::Decimal;
use serde_json::{Value, json};
use ustr::Ustr;

#[derive(Clone)]
struct TestServerState {
    connection_count: Arc<tokio::sync::Mutex<usize>>,
    subscriptions: Arc<tokio::sync::Mutex<Vec<String>>>,
    subscription_events: Arc<tokio::sync::Mutex<Vec<(String, bool)>>>,
    disconnect_trigger: Arc<AtomicBool>,
    ping_count: Arc<AtomicUsize>,
    ticker_queries: Arc<tokio::sync::Mutex<Vec<HashMap<String, String>>>>,
    ticker_response: Arc<tokio::sync::Mutex<Option<Value>>>,
    book_updates: Arc<tokio::sync::Mutex<Vec<Value>>>,
}

impl Default for TestServerState {
    fn default() -> Self {
        Self {
            connection_count: Arc::new(tokio::sync::Mutex::new(0)),
            subscriptions: Arc::new(tokio::sync::Mutex::new(Vec::new())),
            subscription_events: Arc::new(tokio::sync::Mutex::new(Vec::new())),
            disconnect_trigger: Arc::new(AtomicBool::new(false)),
            ping_count: Arc::new(AtomicUsize::new(0)),
            ticker_queries: Arc::new(tokio::sync::Mutex::new(Vec::new())),
            ticker_response: Arc::new(tokio::sync::Mutex::new(None)),
            book_updates: Arc::new(tokio::sync::Mutex::new(Vec::new())),
        }
    }
}

fn load_test_data(filename: &str) -> Value {
    let path = format!("test_data/{filename}");
    let content = std::fs::read_to_string(path).expect("Failed to read test data");
    serde_json::from_str(&content).expect("Failed to parse test data")
}

async fn handle_get_instruments(query: Query<HashMap<String, String>>) -> impl IntoResponse {
    let category = query.get("category").map(String::as_str);
    let filename = match category {
        Some("linear") => "http_get_instruments_linear.json",
        Some("spot") => "http_get_instruments_spot.json",
        Some("inverse") => "http_get_instruments_inverse.json",
        Some("option") => "http_get_instruments_option.json",
        _ => {
            return (
                axum::http::StatusCode::BAD_REQUEST,
                Json(json!({
                    "retCode": 10001,
                    "retMsg": "Invalid category",
                    "result": {},
                    "time": 1704470400123i64
                })),
            )
                .into_response();
        }
    };

    let instruments = load_test_data(filename);
    Json(instruments).into_response()
}

async fn handle_get_fee_rate() -> impl IntoResponse {
    let fee_rate = load_test_data("http_get_fee_rate.json");
    Json(fee_rate).into_response()
}

async fn handle_get_server_time() -> impl IntoResponse {
    Json(json!({
        "retCode": 0,
        "retMsg": "OK",
        "result": {
            "timeSecond": "1704470400",
            "timeNano": "1704470400123456789"
        },
        "retExtInfo": {},
        "time": 1704470400123i64
    }))
}

async fn handle_get_orderbook() -> impl IntoResponse {
    let orderbook = load_test_data("http_get_orderbook.json");
    Json(orderbook).into_response()
}

async fn handle_get_funding_history() -> impl IntoResponse {
    let funding = load_test_data("http_get_funding_history.json");
    Json(funding).into_response()
}

async fn handle_get_tickers(
    State(state): State<TestServerState>,
    Query(query): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    state.ticker_queries.lock().await.push(query);
    Json(
        state
            .ticker_response
            .lock()
            .await
            .clone()
            .expect("ticker response must be configured"),
    )
}

async fn handle_websocket(ws: WebSocketUpgrade, State(state): State<TestServerState>) -> Response {
    ws.on_upgrade(|socket| handle_socket(socket, state))
}

async fn handle_socket(mut socket: WebSocket, state: TestServerState) {
    {
        let mut count = state.connection_count.lock().await;
        *count += 1;
    }

    loop {
        if state.disconnect_trigger.load(Ordering::Relaxed) {
            break;
        }

        for update in std::mem::take(&mut *state.book_updates.lock().await) {
            let topic = update["topic"].as_str().unwrap();
            if state.subscriptions.lock().await.iter().any(|s| s == topic) {
                socket
                    .send(Message::Text(update.to_string().into()))
                    .await
                    .unwrap();
            }
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
                let Ok(value) = serde_json::from_str::<Value>(&text) else {
                    continue;
                };

                let op = value.get("op").and_then(|v| v.as_str());

                match op {
                    Some("ping") => {
                        state.ping_count.fetch_add(1, Ordering::Relaxed);
                        let pong_response = json!({
                            "success": true,
                            "ret_msg": "pong",
                            "conn_id": "test-conn-id",
                            "req_id": value.get("req_id").and_then(|v| v.as_str()).unwrap_or(""),
                            "op": "pong"
                        });

                        if socket
                            .send(Message::Text(pong_response.to_string().into()))
                            .await
                            .is_err()
                        {
                            break;
                        }
                    }
                    Some("subscribe") => {
                        let args = value.get("args").and_then(|a| a.as_array());
                        if let Some(topics) = args {
                            for topic in topics {
                                if let Some(topic_str) = topic.as_str() {
                                    state
                                        .subscription_events
                                        .lock()
                                        .await
                                        .push((topic_str.to_string(), true));

                                    let mut subs = state.subscriptions.lock().await;
                                    if !subs.contains(&topic_str.to_string()) {
                                        subs.push(topic_str.to_string());
                                    }
                                }
                            }
                        }

                        let sub_response = json!({
                            "success": true,
                            "ret_msg": "",
                            "conn_id": "test-conn-id",
                            "req_id": value.get("req_id").and_then(|v| v.as_str()).unwrap_or(""),
                            "op": "subscribe"
                        });

                        if socket
                            .send(Message::Text(sub_response.to_string().into()))
                            .await
                            .is_err()
                        {
                            break;
                        }

                        if let Some(topics) = args
                            && let Some(first_topic) = topics.first().and_then(|t| t.as_str())
                        {
                            if first_topic.contains("publicTrade") {
                                let trade_msg = load_test_data("ws_public_trade.json");

                                if socket
                                    .send(Message::Text(trade_msg.to_string().into()))
                                    .await
                                    .is_err()
                                {
                                    break;
                                }
                            } else if first_topic.contains("orderbook") {
                                let mut orderbook_msg =
                                    load_test_data("ws_orderbook_snapshot.json");
                                orderbook_msg["topic"] = first_topic.into();
                                orderbook_msg["data"]["s"] =
                                    first_topic.rsplit('.').next().unwrap().into();

                                if socket
                                    .send(Message::Text(orderbook_msg.to_string().into()))
                                    .await
                                    .is_err()
                                {
                                    break;
                                }
                            } else if first_topic.contains("tickers") {
                                let ticker_msg = load_test_data("ws_ticker_linear.json");

                                if socket
                                    .send(Message::Text(ticker_msg.to_string().into()))
                                    .await
                                    .is_err()
                                {
                                    break;
                                }
                            } else if first_topic.contains("kline") {
                                let kline_msg = load_test_data("ws_kline.json");

                                if socket
                                    .send(Message::Text(kline_msg.to_string().into()))
                                    .await
                                    .is_err()
                                {
                                    break;
                                }
                            }
                        }
                    }
                    Some("unsubscribe") => {
                        let args = value.get("args").and_then(|a| a.as_array());
                        if let Some(topics) = args {
                            for topic in topics {
                                if let Some(topic_str) = topic.as_str() {
                                    let mut events = state.subscription_events.lock().await;
                                    events.retain(|(t, _)| t != topic_str);
                                    drop(events);

                                    let mut subs = state.subscriptions.lock().await;
                                    subs.retain(|s| s != topic_str);
                                }
                            }
                        }

                        let unsub_response = json!({
                            "success": true,
                            "ret_msg": "",
                            "conn_id": "test-conn-id",
                            "req_id": value.get("req_id").and_then(|v| v.as_str()).unwrap_or(""),
                            "op": "unsubscribe"
                        });

                        if socket
                            .send(Message::Text(unsub_response.to_string().into()))
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
            Message::Close(_) => {
                break;
            }
            _ => {}
        }
    }

    let mut count = state.connection_count.lock().await;
    *count = count.saturating_sub(1);
}

fn create_test_router(state: TestServerState) -> Router {
    Router::new()
        .route("/v5/market/instruments-info", get(handle_get_instruments))
        .route("/v5/market/orderbook", get(handle_get_orderbook))
        .route(
            "/v5/market/funding/history",
            get(handle_get_funding_history),
        )
        .route("/v5/market/tickers", get(handle_get_tickers))
        .route("/v5/account/fee-rate", get(handle_get_fee_rate))
        .route("/v3/public/time", get(handle_get_server_time))
        .route("/v5/public/linear", get(handle_websocket))
        .route("/v5/public/spot", get(handle_websocket))
        .with_state(state)
}

async fn start_test_server()
-> Result<(SocketAddr, TestServerState), Box<dyn std::error::Error + Send + Sync>> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let state = TestServerState::default();
    let router = create_test_router(state.clone());

    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });

    let health_url = format!("http://{addr}/v3/public/time");
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

    Ok((addr, state))
}

fn create_test_config(addr: SocketAddr) -> BybitDataClientConfig {
    BybitDataClientConfig {
        api_key: None,
        api_secret: None,
        product_types: vec![BybitProductType::Linear],
        environment: BybitEnvironment::Mainnet,
        base_url_http: Some(format!("http://{addr}")),
        base_url_ws_public: Some(format!("ws://{addr}/v5/public/linear")),
        base_url_ws_private: None,
        proxy_url: None,
        http_timeout_secs: 10,
        max_retries: 1,
        retry_delay_initial_ms: 100,
        retry_delay_max_ms: 1000,
        heartbeat_interval_secs: 5,
        recv_window_ms: 5000,
        update_instruments_interval_mins: None,
        instrument_poll_interval_secs: None,
        transport_backend: Default::default(),
    }
}

#[rstest]
#[case::valid(0, "97000.125", Some("97000.125"))]
#[case::zero(0, "0", None)]
#[case::negative(0, "-1", None)]
#[case::invalid(0, "not_a_number", None)]
#[case::venue_error(10001, "97000.125", None)]
#[tokio::test]
async fn test_option_chain_reference_price_response(
    #[case] ret_code: i64,
    #[case] value: &str,
    #[case] expected: Option<&str>,
) {
    let (addr, state) = start_test_server().await.unwrap();
    *state.ticker_response.lock().await = Some(json!({
        "retCode": ret_code,
        "retMsg": if ret_code == 0 { "OK" } else { "Invalid request" },
        "result": {
            "category": "option",
            "nextPageCursor": "",
            "list": [{
                "symbol": "BTC-28MAR25-90000-C",
                "bid1Price": "1",
                "bid1Size": "2",
                "bid1Iv": "0.4",
                "ask1Price": "3",
                "ask1Size": "4",
                "ask1Iv": "0.5",
                "lastPrice": "2",
                "highPrice24h": "4",
                "lowPrice24h": "1",
                "markPrice": "2.5",
                "indexPrice": "96900",
                "markIv": "0.45",
                "underlyingPrice": value,
                "openInterest": "10",
                "turnover24h": "20",
                "volume24h": "30",
                "totalVolume": "40",
                "totalTurnover": "50",
                "delta": "0.5",
                "gamma": "0.01",
                "vega": "0.1",
                "theta": "-0.1",
                "predictedDeliveryPrice": "97100",
                "change24h": "0.01"
            }]
        },
        "retExtInfo": {},
        "time": 1704470400123_i64
    }));
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    set_data_event_sender(tx);
    let client = BybitDataClient::new(*BYBIT_CLIENT_ID, create_test_config(addr)).unwrap();
    let series_id = OptionSeriesId::new(
        *BYBIT_VENUE,
        Ustr::from("BTC"),
        Ustr::from("USDC"),
        UnixNanos::from(1_743_120_000_000_000_000),
    );
    let instrument_id = InstrumentId::from("BTC-28MAR25-90000-C-OPTION.BYBIT");
    let mut params = Params::new();
    params.insert("test-case".to_string(), json!(ret_code));
    let request = RequestOptionChainReferencePrice::new(
        series_id,
        instrument_id,
        Some(*BYBIT_CLIENT_ID),
        UUID4::new(),
        UnixNanos::from(42),
        Some(params.clone()),
    );
    let request_id = request.request_id;
    let clock = get_atomic_clock_realtime();
    let before_ns = clock.get_time_ns();

    client
        .request_option_chain_reference_price(request)
        .expect("request option-chain reference price");
    let event = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("timeout waiting for option-chain reference price")
        .expect("data event channel closed");
    let after_ns = clock.get_time_ns();
    let DataEvent::Response(DataResponse::OptionChainReferencePrice(response)) = event else {
        panic!("expected option-chain reference price response, received {event:?}");
    };
    let queries = state.ticker_queries.lock().await;

    assert_eq!(response.correlation_id, request_id);
    assert_eq!(response.client_id, *BYBIT_CLIENT_ID);
    assert_eq!(response.series_id, series_id);
    assert_eq!(response.price, expected.map(Price::from));
    assert!(response.ts_init >= before_ns && response.ts_init <= after_ns);
    assert_eq!(response.params, Some(params));
    assert_eq!(queries.len(), 1);
    assert_eq!(
        queries[0].get("category").map(String::as_str),
        Some("option")
    );
    assert_eq!(
        queries[0].get("symbol").map(String::as_str),
        Some("BTC-28MAR25-90000-C")
    );
}

#[rstest]
#[tokio::test]
async fn test_data_client_connect_disconnect() {
    let (addr, state) = start_test_server().await.unwrap();
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    set_data_event_sender(tx);
    let (system_tx, mut system_rx) = tokio::sync::mpsc::unbounded_channel();
    replace_system_event_sender(system_tx);

    let config = create_test_config(addr);
    let registry = SocketReconnectRegistry::default();
    let mut client = registry
        .scope(|| BybitDataClient::new(*BYBIT_CLIENT_ID, config))
        .unwrap();
    assert!(!client.is_connected());

    client.connect().await.unwrap();
    assert!(client.is_connected());

    wait_until_async(
        || async { *state.connection_count.lock().await > 0 },
        Duration::from_secs(5),
    )
    .await;
    let event = tokio::time::timeout(Duration::from_secs(2), system_rx.recv())
        .await
        .unwrap()
        .unwrap();
    let SystemEvent::SocketState(change) = event;
    let endpoint = Ustr::from("bybit-linear-data-streams");
    let handle = registry.handle(*BYBIT_CLIENT_ID, endpoint).unwrap();

    assert_eq!(*state.connection_count.lock().await, 1);
    assert_eq!(change.client_id, *BYBIT_CLIENT_ID);
    assert_eq!(change.venue, Some(*BYBIT_VENUE));
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

    client.disconnect().await.unwrap();
    assert!(!client.is_connected());
    assert!(registry.handle(*BYBIT_CLIENT_ID, endpoint).is_none());
}

#[rstest]
#[tokio::test]
async fn test_data_client_subscribe_trades() {
    let (addr, state) = start_test_server().await.unwrap();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    set_data_event_sender(tx);

    let config = create_test_config(addr);
    let mut client = BybitDataClient::new(*BYBIT_CLIENT_ID, config).unwrap();
    client.connect().await.unwrap();

    wait_until_async(
        || async { *state.connection_count.lock().await > 0 },
        Duration::from_secs(5),
    )
    .await;

    while rx.try_recv().is_ok() {}

    let instrument_id = InstrumentId::from("BTCUSDT-LINEAR.BYBIT");
    let cmd = SubscribeTrades::new(
        instrument_id,
        Some(*BYBIT_CLIENT_ID),
        None,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );
    client.subscribe_trades(cmd).unwrap();

    wait_until_async(
        || async { !state.subscription_events.lock().await.is_empty() },
        Duration::from_secs(5),
    )
    .await;

    let event = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("timeout waiting for event")
        .expect("channel closed");

    assert!(
        matches!(event, DataEvent::Data(Data::Trade(_))),
        "Expected Trade event, was: {event:?}"
    );

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_data_client_subscribe_quotes_linear() {
    let (addr, state) = start_test_server().await.unwrap();

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    set_data_event_sender(tx);

    let config = create_test_config(addr);
    let mut client = BybitDataClient::new(*BYBIT_CLIENT_ID, config).unwrap();
    client.connect().await.unwrap();

    wait_until_async(
        || async { *state.connection_count.lock().await > 0 },
        Duration::from_secs(5),
    )
    .await;

    while rx.try_recv().is_ok() {}

    let instrument_id = InstrumentId::from("BTCUSDT-LINEAR.BYBIT");
    let cmd = SubscribeQuotes::new(
        instrument_id,
        Some(*BYBIT_CLIENT_ID),
        None,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );
    client.subscribe_quotes(cmd).unwrap();

    wait_until_async(
        || async {
            state
                .subscription_events
                .lock()
                .await
                .iter()
                .any(|(topic, subscribed)| topic == "orderbook.1.BTCUSDT" && *subscribed)
        },
        Duration::from_secs(5),
    )
    .await;

    let event = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("timeout waiting for event")
        .expect("channel closed");

    assert!(
        matches!(event, DataEvent::Data(Data::Quote(_))),
        "Expected Quote event, was: {event:?}"
    );

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_data_client_subscribe_book_deltas() {
    let (addr, state) = start_test_server().await.unwrap();

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    set_data_event_sender(tx);

    let config = create_test_config(addr);
    let mut client = BybitDataClient::new(*BYBIT_CLIENT_ID, config).unwrap();
    client.connect().await.unwrap();

    wait_until_async(
        || async { *state.connection_count.lock().await > 0 },
        Duration::from_secs(5),
    )
    .await;

    while rx.try_recv().is_ok() {}

    let instrument_id = InstrumentId::from("BTCUSDT-LINEAR.BYBIT");
    let cmd = SubscribeBookDeltas::new(
        instrument_id,
        BookType::L2_MBP,
        Some(*BYBIT_CLIENT_ID),
        None,
        UUID4::new(),
        UnixNanos::default(),
        None,
        false,
        None,
        None,
    );
    client.subscribe_book_deltas(cmd).unwrap();

    wait_until_async(
        || async {
            state
                .subscription_events
                .lock()
                .await
                .iter()
                .any(|(topic, _)| topic.contains("orderbook"))
        },
        Duration::from_secs(5),
    )
    .await;

    let event = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("timeout waiting for event")
        .expect("channel closed");

    assert!(
        matches!(event, DataEvent::Data(Data::BookDeltas(_))),
        "Expected Deltas event, was: {event:?}"
    );

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_data_client_reset_clears_state() {
    let (addr, _state) = start_test_server().await.unwrap();

    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    set_data_event_sender(tx);

    let config = create_test_config(addr);
    let mut client = BybitDataClient::new(*BYBIT_CLIENT_ID, config).unwrap();

    client.reset().unwrap();
    assert!(!client.is_connected());

    client.connect().await.unwrap();
    assert!(client.is_connected());

    client.reset().unwrap();
    assert!(!client.is_connected());
}

#[rstest]
#[tokio::test]
async fn test_data_client_emits_instruments_on_connect() {
    let (addr, _state) = start_test_server().await.unwrap();

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    set_data_event_sender(tx);

    let config = create_test_config(addr);
    let mut client = BybitDataClient::new(*BYBIT_CLIENT_ID, config).unwrap();

    client.connect().await.unwrap();

    let instruments_received = Arc::new(AtomicUsize::new(0));
    let counter = instruments_received.clone();

    wait_until_async(
        || {
            while let Ok(event) = rx.try_recv() {
                if matches!(event, DataEvent::Instrument(_)) {
                    counter.fetch_add(1, Ordering::Relaxed);
                }
            }
            let count = counter.load(Ordering::Relaxed);
            async move { count > 0 }
        },
        Duration::from_secs(5),
    )
    .await;

    assert!(
        instruments_received.load(Ordering::Relaxed) > 0,
        "Expected to receive instrument events on connect"
    );

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_data_client_request_book_snapshot() {
    let (addr, _state) = start_test_server().await.unwrap();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    set_data_event_sender(tx);

    let config = create_test_config(addr);
    let mut client = BybitDataClient::new(*BYBIT_CLIENT_ID, config).unwrap();
    client.connect().await.unwrap();

    // Drain instrument events from connect
    tokio::time::sleep(Duration::from_millis(500)).await;

    while rx.try_recv().is_ok() {}

    let instrument_id = InstrumentId::from("BTCUSDT-LINEAR.BYBIT");
    let request = RequestBookSnapshot::new(
        instrument_id,
        None,
        Some(*BYBIT_CLIENT_ID),
        UUID4::new(),
        UnixNanos::default(),
        None,
    );
    client.request_book_snapshot(request).unwrap();

    let event = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("timeout waiting for book snapshot response")
        .expect("channel closed");

    assert!(
        matches!(event, DataEvent::Response(DataResponse::Book(_))),
        "Expected Book response, was: {event:?}"
    );

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_data_client_request_funding_rates() {
    let (addr, _state) = start_test_server().await.unwrap();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    set_data_event_sender(tx);

    let config = create_test_config(addr);
    let mut client = BybitDataClient::new(*BYBIT_CLIENT_ID, config).unwrap();
    client.connect().await.unwrap();

    // Drain instrument events from connect
    tokio::time::sleep(Duration::from_millis(500)).await;

    while rx.try_recv().is_ok() {}

    let instrument_id = InstrumentId::from("BTCUSDT-LINEAR.BYBIT");
    let request = RequestFundingRates::new(
        instrument_id,
        None,
        None,
        None,
        Some(*BYBIT_CLIENT_ID),
        UUID4::new(),
        UnixNanos::default(),
        None,
    );
    client.request_funding_rates(request).unwrap();

    let event = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("timeout waiting for funding rates response")
        .expect("channel closed");

    assert!(
        matches!(event, DataEvent::Response(DataResponse::FundingRates(_))),
        "Expected FundingRates response, was: {event:?}"
    );

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_data_client_request_funding_rates_rejects_spot() {
    let (addr, _state) = start_test_server().await.unwrap();
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    set_data_event_sender(tx);

    let config = create_test_config(addr);
    let mut client = BybitDataClient::new(*BYBIT_CLIENT_ID, config).unwrap();
    client.connect().await.unwrap();

    tokio::time::sleep(Duration::from_millis(500)).await;

    let instrument_id = InstrumentId::from("BTCUSDT-SPOT.BYBIT");
    let request = RequestFundingRates::new(
        instrument_id,
        None,
        None,
        None,
        Some(*BYBIT_CLIENT_ID),
        UUID4::new(),
        UnixNanos::default(),
        None,
    );
    let result = client.request_funding_rates(request);
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("Funding rates not available for Spot instruments"),
    );

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_data_client_request_funding_rates_rejects_option() {
    let (addr, _state) = start_test_server().await.unwrap();
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    set_data_event_sender(tx);

    let config = create_test_config(addr);
    let mut client = BybitDataClient::new(*BYBIT_CLIENT_ID, config).unwrap();
    client.connect().await.unwrap();

    tokio::time::sleep(Duration::from_millis(500)).await;

    let instrument_id = InstrumentId::from("BTC-26DEC25-100000-C-OPTION.BYBIT");
    let request = RequestFundingRates::new(
        instrument_id,
        None,
        None,
        None,
        Some(*BYBIT_CLIENT_ID),
        UUID4::new(),
        UnixNanos::default(),
        None,
    );
    let result = client.request_funding_rates(request);
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("Funding rates not available for Option instruments"),
    );

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn test_data_client_request_instruments() {
    let (addr, _state) = start_test_server().await.unwrap();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    set_data_event_sender(tx);

    let config = create_test_config(addr);
    let mut client = BybitDataClient::new(*BYBIT_CLIENT_ID, config).unwrap();
    client.connect().await.unwrap();

    // Drain instrument events from connect
    tokio::time::sleep(Duration::from_millis(500)).await;

    while rx.try_recv().is_ok() {}

    let request = RequestInstruments::new(
        None,
        None,
        Some(*BYBIT_CLIENT_ID),
        Some(*BYBIT_VENUE),
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
    let (addr, _state) = start_test_server().await.unwrap();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    set_data_event_sender(tx);

    let config = create_test_config(addr);
    let mut client = BybitDataClient::new(*BYBIT_CLIENT_ID, config).unwrap();
    client.connect().await.unwrap();

    // Drain instrument events from connect
    tokio::time::sleep(Duration::from_millis(500)).await;

    while rx.try_recv().is_ok() {}

    let instrument_id = InstrumentId::from("BTCUSDT-LINEAR.BYBIT");
    let request = RequestInstrument::new(
        instrument_id,
        None,
        None,
        Some(*BYBIT_CLIENT_ID),
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
#[case::shared_quotes_first(1, true, true)]
#[case::shared_book_first(1, false, false)]
#[case::shared_remove_book_first(1, true, false)]
#[case::shared_remove_quotes_first(1, false, true)]
#[case::deeper_remove_quotes_first(50, true, true)]
#[case::deeper_remove_book_first(50, false, false)]
#[tokio::test]
async fn test_data_client_book_quote_topic_lifetime(
    #[case] depth: usize,
    #[case] quotes_first: bool,
    #[case] remove_quotes_first: bool,
) {
    let (addr, state) = start_test_server().await.unwrap();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    set_data_event_sender(tx);
    let mut client = BybitDataClient::new(*BYBIT_CLIENT_ID, create_test_config(addr)).unwrap();
    client.connect().await.unwrap();
    let instrument_id = InstrumentId::from("BTCUSDT-LINEAR.BYBIT");
    let quotes = SubscribeQuotes::new(
        instrument_id,
        Some(*BYBIT_CLIENT_ID),
        None,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );
    let book = SubscribeBookDeltas::new(
        instrument_id,
        BookType::L2_MBP,
        Some(*BYBIT_CLIENT_ID),
        None,
        UUID4::new(),
        UnixNanos::default(),
        NonZeroUsize::new(depth),
        false,
        None,
        None,
    );

    for subscribe_quotes in [quotes_first, !quotes_first] {
        if subscribe_quotes {
            client.subscribe_quotes(quotes.clone()).unwrap();
        } else {
            client.subscribe_book_deltas(book.clone()).unwrap();
        }
    }
    let mut expected_topics = vec!["orderbook.1.BTCUSDT".to_string()];
    if depth != 1 {
        expected_topics.push(format!("orderbook.{depth}.BTCUSDT"));
    }
    wait_until_async(
        || async {
            let mut topics = state.subscriptions.lock().await.clone();
            topics.sort();
            topics == expected_topics
        },
        Duration::from_secs(5),
    )
    .await;

    let mut different_depth = book.clone();
    different_depth.depth = NonZeroUsize::new(if depth == 1 { 50 } else { 1 });
    assert_eq!(
        client
            .subscribe_book_deltas(different_depth)
            .unwrap_err()
            .to_string(),
        format!("Already subscribed to book depth {depth} for {instrument_id}"),
    );

    // Repeating either request must not acquire another transport reference
    client.subscribe_quotes(quotes).unwrap();
    client.subscribe_book_deltas(book).unwrap();
    let unsubscribe_quotes = UnsubscribeQuotes::new(
        instrument_id,
        Some(*BYBIT_CLIENT_ID),
        None,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );
    let unsubscribe_book = UnsubscribeBookDeltas::new(
        instrument_id,
        Some(*BYBIT_CLIENT_ID),
        None,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );

    if remove_quotes_first {
        client.unsubscribe_quotes(&unsubscribe_quotes).unwrap();
    } else {
        client.unsubscribe_book_deltas(&unsubscribe_book).unwrap();
    }
    let remaining_depth = if remove_quotes_first { depth } else { 1 };
    let mut snapshot = load_test_data("ws_orderbook_snapshot.json");
    snapshot["topic"] = format!("orderbook.{remaining_depth}.BTCUSDT").into();
    snapshot["ts"] = 1_709_891_700_000_u64.into();
    state.book_updates.lock().await.push(snapshot);
    let expected_ts = UnixNanos::new(1_709_891_700_000_000_000);
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            match rx.recv().await.unwrap() {
                DataEvent::Data(Data::Quote(quote)) if quote.ts_event == expected_ts => {
                    assert!(!remove_quotes_first);
                    assert_eq!(quote.instrument_id, instrument_id);
                    assert_eq!(quote.bid_price.as_decimal(), Decimal::from(27450));
                    assert_eq!(quote.ask_price.as_decimal(), Decimal::from(27451));
                    break;
                }
                DataEvent::Data(Data::BookDeltas(deltas)) if deltas.ts_event == expected_ts => {
                    assert!(remove_quotes_first);
                    assert_eq!(deltas.instrument_id, instrument_id);
                    break;
                }
                _ => {}
            }
        }
    })
    .await
    .expect("remaining subscriber must still receive fresh book data");

    if remove_quotes_first {
        client.unsubscribe_book_deltas(&unsubscribe_book).unwrap();
    } else {
        client.unsubscribe_quotes(&unsubscribe_quotes).unwrap();
    }
    wait_until_async(
        || async { state.subscriptions.lock().await.is_empty() },
        Duration::from_secs(5),
    )
    .await;
    assert!(state.subscriptions.lock().await.is_empty());
    client.disconnect().await.unwrap();
}

#[rstest]
#[case::linear(BybitProductType::Linear, "BTCUSDT-LINEAR.BYBIT")]
#[case::spot(BybitProductType::Spot, "BTCUSDT-SPOT.BYBIT")]
#[case::inverse(BybitProductType::Inverse, "BTCUSD-INVERSE.BYBIT")]
#[tokio::test]
#[ignore = "Connects to Bybit mainnet public market data"]
async fn test_live_book_and_quotes(#[case] product_type: BybitProductType, #[case] symbol: &str) {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    set_data_event_sender(tx);
    let config = BybitDataClientConfig {
        product_types: vec![product_type],
        environment: BybitEnvironment::Mainnet,
        api_key: None,
        api_secret: None,
        instrument_poll_interval_secs: None,
        ..Default::default()
    };
    let mut client = BybitDataClient::new(*BYBIT_CLIENT_ID, config).unwrap();
    tokio::time::timeout(Duration::from_secs(60), client.connect())
        .await
        .unwrap()
        .unwrap();
    let instrument_id = InstrumentId::from(symbol);
    client
        .subscribe_quotes(SubscribeQuotes::new(
            instrument_id,
            Some(*BYBIT_CLIENT_ID),
            None,
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ))
        .unwrap();
    client
        .subscribe_book_deltas(SubscribeBookDeltas::new(
            instrument_id,
            BookType::L2_MBP,
            Some(*BYBIT_CLIENT_ID),
            None,
            UUID4::new(),
            UnixNanos::default(),
            NonZeroUsize::new(50),
            false,
            None,
            None,
        ))
        .unwrap();
    let mut quotes = 0;
    let mut books = 0;
    let mut deletions = 0;
    let result = tokio::time::timeout(Duration::from_secs(30), async {
        while quotes < 20 || books < 20 || deletions == 0 {
            match rx.recv().await.unwrap() {
                DataEvent::Data(Data::Quote(quote)) => {
                    assert_eq!(quote.instrument_id, instrument_id);
                    assert!(quote.bid_size.is_positive());
                    assert!(quote.ask_size.is_positive());
                    assert!(quote.bid_price.is_positive());
                    assert!(quote.ask_price >= quote.bid_price);
                    quotes += 1;
                }
                DataEvent::Data(Data::BookDeltas(deltas)) => {
                    assert_eq!(deltas.instrument_id, instrument_id);
                    deletions += deltas
                        .deltas
                        .iter()
                        .filter(|delta| delta.action == BookAction::Delete)
                        .count();
                    books += 1;
                }
                _ => {}
            }
        }
    })
    .await;
    client.disconnect().await.unwrap();
    result.expect("expected quotes and depth-50 book updates with deletions");
    println!("{product_type}: {quotes} valid quotes, {books} books, {deletions} deletions");
}
