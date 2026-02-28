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
    common::enums::{BybitEnvironment, BybitProductType},
    config::BybitDataClientConfig,
    data::BybitDataClient,
};
use nautilus_common::{
    clients::DataClient,
    live::runner::set_data_event_sender,
    messages::{
        DataEvent,
        data::{
            DataResponse, RequestBookSnapshot, RequestFundingRates, SubscribeBookDeltas,
            SubscribeQuotes, SubscribeTrades,
        },
    },
    testing::wait_until_async,
};
use nautilus_core::{UUID4, UnixNanos};
use nautilus_model::{
    data::Data,
    enums::BookType,
    identifiers::{ClientId, InstrumentId},
};
use nautilus_network::http::HttpClient;
use rstest::rstest;
use serde_json::{Value, json};

#[derive(Clone)]
struct TestServerState {
    connection_count: Arc<tokio::sync::Mutex<usize>>,
    subscriptions: Arc<tokio::sync::Mutex<Vec<String>>>,
    subscription_events: Arc<tokio::sync::Mutex<Vec<(String, bool)>>>,
    disconnect_trigger: Arc<AtomicBool>,
    ping_count: Arc<AtomicUsize>,
}

impl Default for TestServerState {
    fn default() -> Self {
        Self {
            connection_count: Arc::new(tokio::sync::Mutex::new(0)),
            subscriptions: Arc::new(tokio::sync::Mutex::new(Vec::new())),
            subscription_events: Arc::new(tokio::sync::Mutex::new(Vec::new())),
            disconnect_trigger: Arc::new(AtomicBool::new(false)),
            ping_count: Arc::new(AtomicUsize::new(0)),
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
                                let orderbook_msg = load_test_data("ws_orderbook_snapshot.json");

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
        http_proxy_url: None,
        ws_proxy_url: None,
        http_timeout_secs: Some(10),
        max_retries: Some(1),
        retry_delay_initial_ms: Some(100),
        retry_delay_max_ms: Some(1000),
        heartbeat_interval_secs: Some(5),
        recv_window_ms: Some(5000),
        update_instruments_interval_mins: None,
    }
}

#[rstest]
#[tokio::test]
async fn test_data_client_connect_disconnect() {
    let (addr, state) = start_test_server().await.unwrap();
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    set_data_event_sender(tx);

    let config = create_test_config(addr);
    let mut client = BybitDataClient::new(ClientId::new("BYBIT"), config).unwrap();
    assert!(!client.is_connected());

    client.connect().await.unwrap();
    assert!(client.is_connected());

    wait_until_async(
        || async { *state.connection_count.lock().await > 0 },
        Duration::from_secs(5),
    )
    .await;
    assert_eq!(*state.connection_count.lock().await, 1);

    client.disconnect().await.unwrap();
    assert!(!client.is_connected());
}

#[rstest]
#[tokio::test]
async fn test_data_client_subscribe_trades() {
    let (addr, state) = start_test_server().await.unwrap();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    set_data_event_sender(tx);

    let config = create_test_config(addr);
    let mut client = BybitDataClient::new(ClientId::new("BYBIT"), config).unwrap();
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
        Some(ClientId::new("BYBIT")),
        None,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );
    client.subscribe_trades(&cmd).unwrap();

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
    let mut client = BybitDataClient::new(ClientId::new("BYBIT"), config).unwrap();
    client.connect().await.unwrap();

    wait_until_async(
        || async { *state.connection_count.lock().await > 0 },
        Duration::from_secs(5),
    )
    .await;

    while rx.try_recv().is_ok() {}

    // Quote subscription uses ticker topic for LINEAR products
    let instrument_id = InstrumentId::from("BTCUSDT-LINEAR.BYBIT");
    let cmd = SubscribeQuotes::new(
        instrument_id,
        Some(ClientId::new("BYBIT")),
        None,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );
    client.subscribe_quotes(&cmd).unwrap();

    wait_until_async(
        || async {
            state
                .subscription_events
                .lock()
                .await
                .iter()
                .any(|(topic, _)| topic.contains("tickers"))
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
    let mut client = BybitDataClient::new(ClientId::new("BYBIT"), config).unwrap();
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
        Some(ClientId::new("BYBIT")),
        None,
        UUID4::new(),
        UnixNanos::default(),
        None,
        false,
        None,
        None,
    );
    client.subscribe_book_deltas(&cmd).unwrap();

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
        matches!(event, DataEvent::Data(Data::Deltas(_))),
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
    let mut client = BybitDataClient::new(ClientId::new("BYBIT"), config).unwrap();

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
    let mut client = BybitDataClient::new(ClientId::new("BYBIT"), config).unwrap();

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
    let mut client = BybitDataClient::new(ClientId::new("BYBIT"), config).unwrap();
    client.connect().await.unwrap();

    // Drain instrument events from connect
    tokio::time::sleep(Duration::from_millis(500)).await;
    while rx.try_recv().is_ok() {}

    let instrument_id = InstrumentId::from("BTCUSDT-LINEAR.BYBIT");
    let request = RequestBookSnapshot::new(
        instrument_id,
        None,
        Some(ClientId::new("BYBIT")),
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
    let mut client = BybitDataClient::new(ClientId::new("BYBIT"), config).unwrap();
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
        Some(ClientId::new("BYBIT")),
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
    let mut client = BybitDataClient::new(ClientId::new("BYBIT"), config).unwrap();
    client.connect().await.unwrap();

    tokio::time::sleep(Duration::from_millis(500)).await;

    let instrument_id = InstrumentId::from("BTCUSDT-SPOT.BYBIT");
    let request = RequestFundingRates::new(
        instrument_id,
        None,
        None,
        None,
        Some(ClientId::new("BYBIT")),
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
    let mut client = BybitDataClient::new(ClientId::new("BYBIT"), config).unwrap();
    client.connect().await.unwrap();

    tokio::time::sleep(Duration::from_millis(500)).await;

    let instrument_id = InstrumentId::from("BTC-26DEC25-100000-C-OPTION.BYBIT");
    let request = RequestFundingRates::new(
        instrument_id,
        None,
        None,
        None,
        Some(ClientId::new("BYBIT")),
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
