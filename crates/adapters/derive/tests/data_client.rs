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

//! Integration tests for `DeriveDataClient` against local REST and WS mocks.

use std::{
    collections::{HashMap, HashSet},
    net::SocketAddr,
    num::NonZeroUsize,
    path::PathBuf,
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
use chrono::{DateTime, TimeZone, Utc};
use futures_util::StreamExt;
use nautilus_common::{
    clients::DataClient,
    live::runner::replace_data_event_sender,
    messages::{
        DataEvent,
        data::{
            DataResponse, RequestBars, RequestForwardPrices, RequestFundingRates,
            RequestInstrument, RequestInstruments, RequestQuotes, RequestTrades,
            SubscribeBookDeltas, SubscribeBookDepth10, SubscribeQuotes, SubscribeTrades,
            UnsubscribeBookDeltas, UnsubscribeQuotes, UnsubscribeTrades,
        },
    },
    testing::wait_until_async,
};
use nautilus_core::{Params, UUID4, UnixNanos};
use nautilus_derive::{
    common::{
        consts::{DERIVE_CLIENT_ID, DERIVE_VENUE},
        enums::DeriveEnvironment,
    },
    config::DeriveDataClientConfig,
    data::DeriveDataClient,
};
use nautilus_model::{
    data::{BarType, Data},
    enums::BookType,
    identifiers::{InstrumentId, Venue},
    instruments::Instrument,
    types::{Price, Quantity},
};
use nautilus_network::{http::HttpClient, websocket::TransportBackend};
use rstest::rstest;
use serde_json::{Value, json};
use ustr::Ustr;

#[derive(Clone, Default)]
struct RestRequest {
    body: Value,
}

#[derive(Clone, Default)]
struct RestState {
    requests: Arc<tokio::sync::Mutex<Vec<RestRequest>>>,
    trade_history_pages: Arc<tokio::sync::Mutex<Vec<Value>>>,
    trade_history_calls: Arc<tokio::sync::Mutex<Vec<Value>>>,
    funding_rate_history_response: Arc<tokio::sync::Mutex<Value>>,
    funding_rate_history_calls: Arc<tokio::sync::Mutex<Vec<Value>>>,
    candles_response: Arc<tokio::sync::Mutex<Value>>,
    candles_pages: Arc<tokio::sync::Mutex<HashMap<i64, Value>>>,
    candles_generated_per_call: Arc<tokio::sync::Mutex<Option<usize>>>,
    candles_calls: Arc<tokio::sync::Mutex<Vec<Value>>>,
    ticker_response: Arc<tokio::sync::Mutex<Value>>,
    ticker_calls: Arc<tokio::sync::Mutex<Vec<Value>>>,
    instrument_response: Arc<tokio::sync::Mutex<Value>>,
    instrument_calls: Arc<tokio::sync::Mutex<Vec<Value>>>,
}

impl RestState {
    async fn requests(&self) -> Vec<RestRequest> {
        self.requests.lock().await.clone()
    }

    async fn trade_history_calls(&self) -> Vec<Value> {
        self.trade_history_calls.lock().await.clone()
    }

    async fn funding_rate_history_calls(&self) -> Vec<Value> {
        self.funding_rate_history_calls.lock().await.clone()
    }

    async fn candles_calls(&self) -> Vec<Value> {
        self.candles_calls.lock().await.clone()
    }

    async fn ticker_calls(&self) -> Vec<Value> {
        self.ticker_calls.lock().await.clone()
    }

    async fn instrument_calls(&self) -> Vec<Value> {
        self.instrument_calls.lock().await.clone()
    }
}

#[derive(Clone, Default)]
struct WsState {
    connection_count: Arc<AtomicUsize>,
    subscribe_frames: Arc<tokio::sync::Mutex<Vec<Value>>>,
    unsubscribe_frames: Arc<tokio::sync::Mutex<Vec<Value>>>,
}

impl WsState {
    async fn subscribes(&self) -> Vec<Value> {
        self.subscribe_frames.lock().await.clone()
    }

    async fn unsubscribes(&self) -> Vec<Value> {
        self.unsubscribe_frames.lock().await.clone()
    }
}

async fn handle_get_instruments(
    State(state): State<RestState>,
    body: axum::body::Bytes,
) -> Response {
    let parsed_body: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    state
        .requests
        .lock()
        .await
        .push(RestRequest { body: parsed_body });

    (
        StatusCode::OK,
        Json(load_json("common/http_get_instruments_eth_all.json")),
    )
        .into_response()
}

async fn handle_get_trade_history(
    State(state): State<RestState>,
    body: axum::body::Bytes,
) -> Response {
    let parsed: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    let page = parsed.get("page").and_then(Value::as_u64).unwrap_or(1) as usize;
    state.trade_history_calls.lock().await.push(parsed);

    let pages = state.trade_history_pages.lock().await;
    let result = pages
        .get(page.saturating_sub(1))
        .cloned()
        .unwrap_or_else(|| json!({"trades": [], "pagination": {"count": 0, "num_pages": 0}}));

    (StatusCode::OK, Json(json!({"id": 1, "result": result}))).into_response()
}

async fn handle_get_funding_rate_history(
    State(state): State<RestState>,
    body: axum::body::Bytes,
) -> Response {
    let parsed: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    state.funding_rate_history_calls.lock().await.push(parsed);

    let result = state.funding_rate_history_response.lock().await.clone();
    (StatusCode::OK, Json(json!({"id": 1, "result": result}))).into_response()
}

async fn handle_get_tradingview_chart_data(
    State(state): State<RestState>,
    body: axum::body::Bytes,
) -> Response {
    let parsed: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    let end_ts = parsed.get("end_timestamp").and_then(Value::as_i64);
    let period = parsed.get("period").and_then(Value::as_i64);
    state.candles_calls.lock().await.push(parsed);

    if let Some(per_call) = *state.candles_generated_per_call.lock().await
        && let (Some(end_ts), Some(period)) = (end_ts, period)
    {
        let result = synth_candles_page(end_ts, period, per_call);
        return (StatusCode::OK, Json(json!({"id": 1, "result": result}))).into_response();
    }

    {
        let pages = state.candles_pages.lock().await;
        if !pages.is_empty() {
            let result = end_ts
                .and_then(|ts| pages.get(&ts).cloned())
                .unwrap_or_else(|| json!([]));
            return (StatusCode::OK, Json(json!({"id": 1, "result": result}))).into_response();
        }
    }

    let response_value = state.candles_response.lock().await.clone();
    // A configured `error` envelope is returned verbatim so tests can drive
    // the REST-error path; any other value is wrapped as a `result` envelope.
    let body = if response_value.get("error").is_some() {
        response_value
    } else {
        json!({"id": 1, "result": response_value})
    };
    (StatusCode::OK, Json(body)).into_response()
}

fn synth_candles_page(end_ts: i64, period: i64, per_call: usize) -> Value {
    let mut bars = Vec::with_capacity(per_call);
    for i in 0..per_call {
        let bucket = end_ts - period * i as i64;

        bars.push(json!({
            "open_price": "100.0",
            "high_price": "101.0",
            "low_price": "99.0",
            "close_price": "100.5",
            "volume_usd": "0",
            "volume_contracts": "1.0",
            "timestamp": bucket,
            "timestamp_bucket": bucket,
        }));
    }
    Value::Array(bars)
}

fn candle_json(bucket: i64) -> Value {
    json!({
        "open_price": "100.0",
        "high_price": "101.0",
        "low_price": "99.0",
        "close_price": "100.5",
        "volume_usd": "0",
        "volume_contracts": "1.0",
        "timestamp": bucket,
        "timestamp_bucket": bucket,
    })
}

fn datetime_from_secs(secs: i64) -> DateTime<Utc> {
    Utc.timestamp_opt(secs, 0).unwrap()
}

async fn handle_get_tickers(State(state): State<RestState>, body: axum::body::Bytes) -> Response {
    let parsed: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    state.ticker_calls.lock().await.push(parsed);

    let response_value = state.ticker_response.lock().await.clone();
    // A configured `error` envelope is returned verbatim so tests can drive
    // the REST-error path; any other value is wrapped as a `result` envelope.
    let body = if response_value.get("error").is_some() {
        response_value
    } else if response_value.get("tickers").is_some() {
        json!({"id": 1, "result": response_value})
    } else {
        let instrument_name = response_value
            .get("instrument_name")
            .and_then(Value::as_str)
            .unwrap_or("ETH-PERP");
        json!({"id": 1, "result": {"tickers": {instrument_name: response_value}}})
    };
    (StatusCode::OK, Json(body)).into_response()
}

async fn handle_get_instrument(
    State(state): State<RestState>,
    body: axum::body::Bytes,
) -> Response {
    let parsed: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    state.instrument_calls.lock().await.push(parsed);

    let response_value = state.instrument_response.lock().await.clone();
    let body = if response_value.get("error").is_some() {
        response_value
    } else {
        json!({"id": 1, "result": response_value})
    };
    (StatusCode::OK, Json(body)).into_response()
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

async fn start_rest_server(state: RestState) -> SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let router = Router::new()
        .route("/public/get_instruments", post(handle_get_instruments))
        .route("/public/get_instrument", post(handle_get_instrument))
        .route("/public/get_trade_history", post(handle_get_trade_history))
        .route(
            "/public/get_funding_rate_history",
            post(handle_get_funding_rate_history),
        )
        .route(
            "/public/get_tradingview_chart_data",
            post(handle_get_tradingview_chart_data),
        )
        .route("/public/get_tickers", post(handle_get_tickers))
        .route("/health", get(handle_rest_health))
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

    while let Some(frame) = socket.next().await {
        let Ok(frame) = frame else { break };
        match frame {
            Message::Text(text) => {
                let Ok(payload) = serde_json::from_str::<Value>(&text) else {
                    continue;
                };
                let id = payload.get("id").and_then(Value::as_u64).unwrap_or(0);
                let method = payload.get("method").and_then(Value::as_str).unwrap_or("");

                match method {
                    "subscribe" => {
                        state.subscribe_frames.lock().await.push(payload.clone());
                        let channels = payload
                            .get("params")
                            .and_then(|p| p.get("channels"))
                            .and_then(Value::as_array)
                            .cloned()
                            .unwrap_or_default();
                        let reply = json!({"id": id, "result": {"channels": channels}});
                        if socket
                            .send(Message::Text(reply.to_string().into()))
                            .await
                            .is_err()
                        {
                            break;
                        }

                        for channel in channels {
                            let Some(channel) = channel.as_str() else {
                                continue;
                            };

                            if let Some(notification) = subscription_notification(channel)
                                && socket
                                    .send(Message::Text(notification.to_string().into()))
                                    .await
                                    .is_err()
                            {
                                break;
                            }
                        }
                    }
                    "unsubscribe" => {
                        state.unsubscribe_frames.lock().await.push(payload.clone());
                        let reply = json!({"id": id, "result": {"success": true}});
                        if socket
                            .send(Message::Text(reply.to_string().into()))
                            .await
                            .is_err()
                        {
                            break;
                        }
                    }
                    _ => {}
                }
            }
            Message::Close(_) => break,
            _ => {}
        }
    }

    state.connection_count.fetch_sub(1, Ordering::SeqCst);
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

fn data_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test_data")
}

fn load_json(filename: &str) -> Value {
    let content = std::fs::read_to_string(data_path().join(filename))
        .unwrap_or_else(|_| panic!("failed to read {filename}"));
    serde_json::from_str(&content).expect("invalid json")
}

fn subscription_notification(channel: &str) -> Option<Value> {
    let data = if channel.starts_with("ticker_slim.") {
        load_json("perps/ws_ticker_slim_eth.json")
    } else if channel.starts_with("ticker.") {
        json!({
            "timestamp": 1_700_000_000_010_i64,
            "instrument_ticker": load_json("perps/ws_ticker_eth.json"),
        })
    } else if channel.starts_with("orderbook.") {
        load_json("perps/ws_orderbook_eth.json")
    } else if channel == "trades.option.ETH" {
        json!([load_json("options/ws_trade_eth.json")])
    } else if channel == "trades.perp.ETH" {
        json!([load_json("perps/ws_trade_eth.json")])
    } else {
        return None;
    };

    Some(json!({
        "jsonrpc": "2.0",
        "method": "subscription",
        "params": {
            "channel": channel,
            "data": data,
        }
    }))
}

fn config(rest_addr: SocketAddr, ws_addr: SocketAddr) -> DeriveDataClientConfig {
    DeriveDataClientConfig {
        base_url_rest: Some(rest_url(rest_addr)),
        base_url_ws: Some(ws_url(ws_addr)),
        proxy_url: None,
        environment: DeriveEnvironment::Mainnet,
        http_timeout_secs: 5,
        ws_timeout_secs: 5,
        update_instruments_interval_mins: 60,
        currencies: Vec::new(),
        include_expired: false,
        auto_load_missing_instruments: true,
        transport_backend: TransportBackend::default(),
    }
}

fn params(values: &[(&str, Value)]) -> Params {
    let mut params = Params::new();
    for (key, value) in values {
        params.insert((*key).to_string(), value.clone());
    }
    params
}

fn subscribe_quotes(instrument_id: InstrumentId, params: Option<Params>) -> SubscribeQuotes {
    SubscribeQuotes::new(
        instrument_id,
        Some(*DERIVE_CLIENT_ID),
        None,
        UUID4::new(),
        UnixNanos::default(),
        None,
        params,
    )
}

fn unsubscribe_quotes(instrument_id: InstrumentId, params: Option<Params>) -> UnsubscribeQuotes {
    UnsubscribeQuotes::new(
        instrument_id,
        Some(*DERIVE_CLIENT_ID),
        None,
        UUID4::new(),
        UnixNanos::default(),
        None,
        params,
    )
}

fn subscribe_book_deltas(
    instrument_id: InstrumentId,
    depth: Option<usize>,
    params: Option<Params>,
) -> SubscribeBookDeltas {
    SubscribeBookDeltas::new(
        instrument_id,
        BookType::L2_MBP,
        Some(*DERIVE_CLIENT_ID),
        None,
        UUID4::new(),
        UnixNanos::default(),
        depth.and_then(NonZeroUsize::new),
        false,
        None,
        params,
    )
}

fn subscribe_book_depth10(
    instrument_id: InstrumentId,
    params: Option<Params>,
) -> SubscribeBookDepth10 {
    SubscribeBookDepth10::new(
        instrument_id,
        BookType::L2_MBP,
        Some(*DERIVE_CLIENT_ID),
        None,
        UUID4::new(),
        UnixNanos::default(),
        NonZeroUsize::new(10),
        false,
        None,
        params,
    )
}

fn unsubscribe_book_deltas(
    instrument_id: InstrumentId,
    params: Option<Params>,
) -> UnsubscribeBookDeltas {
    UnsubscribeBookDeltas::new(
        instrument_id,
        Some(*DERIVE_CLIENT_ID),
        None,
        UUID4::new(),
        UnixNanos::default(),
        None,
        params,
    )
}

fn subscribe_trades(instrument_id: InstrumentId) -> SubscribeTrades {
    SubscribeTrades::new(
        instrument_id,
        Some(*DERIVE_CLIENT_ID),
        None,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    )
}

fn unsubscribe_trades(instrument_id: InstrumentId) -> UnsubscribeTrades {
    UnsubscribeTrades::new(
        instrument_id,
        Some(*DERIVE_CLIENT_ID),
        None,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    )
}

async fn wait_for_subscribe(state: &WsState, channel: &str) {
    wait_until_async(
        || {
            let state = state.clone();
            let channel = channel.to_string();
            async move {
                state.subscribes().await.iter().any(|frame| {
                    frame["params"]["channels"]
                        .as_array()
                        .is_some_and(|channels| channels.iter().any(|c| c == &channel))
                })
            }
        },
        Duration::from_secs(5),
    )
    .await;
}

async fn wait_for_unsubscribe(state: &WsState, channel: &str) {
    wait_until_async(
        || {
            let state = state.clone();
            let channel = channel.to_string();
            async move {
                state.unsubscribes().await.iter().any(|frame| {
                    frame["params"]["channels"]
                        .as_array()
                        .is_some_and(|channels| channels.iter().any(|c| c == &channel))
                })
            }
        },
        Duration::from_secs(5),
    )
    .await;
}

async fn recv_data(rx: &mut tokio::sync::mpsc::UnboundedReceiver<DataEvent>) -> Data {
    match tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("timeout waiting for data event")
        .expect("data event channel closed")
    {
        DataEvent::Data(data) => data,
        other => panic!("expected data event, was {other:?}"),
    }
}

#[rstest]
#[tokio::test]
async fn test_data_client_subscribes_dispatches_and_unsubscribes_exact_channels() {
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    let rest_addr = start_rest_server(rest_state.clone()).await;
    let ws_addr = start_ws_server(ws_state.clone()).await;
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    replace_data_event_sender(tx);

    let mut client = DeriveDataClient::new(*DERIVE_CLIENT_ID, config(rest_addr, ws_addr)).unwrap();
    client.connect().await.unwrap();

    let instrument_id = InstrumentId::from("ETH-PERP.DERIVE");
    let option_id = InstrumentId::from("ETH-20260627-3500-C.DERIVE");
    client
        .subscribe_quotes(subscribe_quotes(
            instrument_id,
            Some(params(&[("interval", json!("100"))])),
        ))
        .unwrap();
    wait_for_subscribe(&ws_state, "ticker_slim.ETH-PERP.100").await;

    match recv_data(&mut rx).await {
        Data::Quote(quote) => {
            assert_eq!(quote.instrument_id, instrument_id);
            assert_eq!(quote.bid_price, Price::from("1992.36"));
            assert_eq!(quote.ask_price, Price::from("1992.37"));
            assert_eq!(quote.bid_size, Quantity::from("1.505"));
            assert_eq!(quote.ask_size, Quantity::from("1.505"));
        }
        other => panic!("expected quote data, was {other:?}"),
    }

    client
        .unsubscribe_quotes(&unsubscribe_quotes(instrument_id, None))
        .unwrap();
    wait_for_unsubscribe(&ws_state, "ticker_slim.ETH-PERP.100").await;

    client
        .subscribe_book_deltas(subscribe_book_deltas(instrument_id, Some(20), None))
        .unwrap();
    wait_for_subscribe(&ws_state, "orderbook.ETH-PERP.1.20").await;

    match recv_data(&mut rx).await {
        Data::Deltas(deltas) => {
            assert_eq!(deltas.instrument_id, instrument_id);
            assert_eq!(deltas.deltas.len(), 3);
            assert_eq!(deltas.deltas[1].order.price, Price::from("3500.00"));
        }
        other => panic!("expected deltas data, was {other:?}"),
    }

    client
        .unsubscribe_book_deltas(&unsubscribe_book_deltas(instrument_id, None))
        .unwrap();
    wait_for_unsubscribe(&ws_state, "orderbook.ETH-PERP.1.20").await;

    client
        .subscribe_trades(subscribe_trades(option_id))
        .unwrap();
    wait_for_subscribe(&ws_state, "trades.option.ETH").await;

    match recv_data(&mut rx).await {
        Data::Trade(trade) => {
            assert_eq!(trade.instrument_id, option_id);
            assert_eq!(trade.trade_id.to_string(), "option-trade-1");
        }
        other => panic!("expected trade data, was {other:?}"),
    }

    client
        .unsubscribe_trades(&unsubscribe_trades(option_id))
        .unwrap();
    wait_for_unsubscribe(&ws_state, "trades.option.ETH").await;

    let requests = rest_state.requests().await;
    assert_eq!(requests.len(), 3);
    let bodies: HashSet<String> = requests.iter().map(|r| r.body.to_string()).collect();
    assert!(bodies.contains(
        &json!({"currency": "ETH", "instrument_type": "perp", "expired": false}).to_string()
    ));
    assert!(bodies.contains(
        &json!({"currency": "ETH", "instrument_type": "option", "expired": false}).to_string()
    ));
    assert!(bodies.contains(
        &json!({"currency": "ETH", "instrument_type": "erc20", "expired": false}).to_string()
    ));

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_subscribe_book_depth10_emits_depth10_snapshot() {
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    let rest_addr = start_rest_server(rest_state).await;
    let ws_addr = start_ws_server(ws_state.clone()).await;
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    replace_data_event_sender(tx);

    let mut client = DeriveDataClient::new(*DERIVE_CLIENT_ID, config(rest_addr, ws_addr)).unwrap();
    client.connect().await.unwrap();

    let instrument_id = InstrumentId::from("ETH-PERP.DERIVE");
    client
        .subscribe_book_depth10(subscribe_book_depth10(instrument_id, None))
        .unwrap();
    wait_for_subscribe(&ws_state, "orderbook.ETH-PERP.1.10").await;

    match recv_data(&mut rx).await {
        Data::Depth10(depth) => {
            assert_eq!(depth.instrument_id, instrument_id);
            assert_eq!(depth.bids[0].price, Price::from("3500.00"));
            assert_eq!(depth.bids[0].size, Quantity::from("1.000"));
            assert_eq!(depth.asks[0].price, Price::from("3501.00"));
            assert_eq!(depth.asks[0].size, Quantity::from("2.000"));
            assert_eq!(depth.bid_counts[0], 1);
            assert_eq!(depth.ask_counts[0], 1);
        }
        other => panic!("expected depth10 data, was {other:?}"),
    }

    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(
        rx.try_recv().is_err(),
        "book depth10 subscription must not emit extra data",
    );

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_request_instruments_returns_err_for_empty_currencies() {
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    let rest_addr = start_rest_server(rest_state.clone()).await;
    let ws_addr = start_ws_server(ws_state.clone()).await;
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    replace_data_event_sender(tx);

    let mut cfg = config(rest_addr, ws_addr);
    cfg.currencies = Vec::new();
    let client = DeriveDataClient::new(*DERIVE_CLIENT_ID, cfg).unwrap();

    let request = RequestInstruments::new(
        None,
        None,
        Some(*DERIVE_CLIENT_ID),
        None,
        UUID4::new(),
        UnixNanos::default(),
        None,
    );

    let err = client
        .request_instruments(request)
        .expect_err("must reject empty currencies");
    let msg = err.to_string();
    assert!(
        msg.contains("requires at least one configured currency"),
        "{msg}"
    );
}

fn request_trades(instrument_id: InstrumentId, limit: Option<usize>) -> RequestTrades {
    RequestTrades::new(
        instrument_id,
        None,
        None,
        limit.and_then(NonZeroUsize::new),
        Some(*DERIVE_CLIENT_ID),
        UUID4::new(),
        UnixNanos::default(),
        None,
    )
}

fn request_funding_rates(instrument_id: InstrumentId, limit: Option<usize>) -> RequestFundingRates {
    RequestFundingRates::new(
        instrument_id,
        None,
        None,
        limit.and_then(NonZeroUsize::new),
        Some(*DERIVE_CLIENT_ID),
        UUID4::new(),
        UnixNanos::default(),
        None,
    )
}

fn request_bars(bar_type: BarType, limit: Option<usize>) -> RequestBars {
    RequestBars::new(
        bar_type,
        None,
        None,
        limit.and_then(NonZeroUsize::new),
        Some(*DERIVE_CLIENT_ID),
        UUID4::new(),
        UnixNanos::default(),
        None,
    )
}

fn request_bars_window(
    bar_type: BarType,
    start: Option<DateTime<Utc>>,
    end: Option<DateTime<Utc>>,
    limit: Option<usize>,
) -> RequestBars {
    RequestBars::new(
        bar_type,
        start,
        end,
        limit.and_then(NonZeroUsize::new),
        Some(*DERIVE_CLIENT_ID),
        UUID4::new(),
        UnixNanos::default(),
        None,
    )
}

fn request_forward_prices(
    underlying: &str,
    instrument_id: Option<InstrumentId>,
) -> RequestForwardPrices {
    RequestForwardPrices::new(
        *DERIVE_VENUE,
        Ustr::from(underlying),
        instrument_id,
        Some(*DERIVE_CLIENT_ID),
        UUID4::new(),
        UnixNanos::default(),
        None,
    )
}

async fn recv_response(rx: &mut tokio::sync::mpsc::UnboundedReceiver<DataEvent>) -> DataResponse {
    recv_response_within(rx, Duration::from_secs(5)).await
}

async fn recv_response_within(
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<DataEvent>,
    within: Duration,
) -> DataResponse {
    let deadline = tokio::time::Instant::now() + within;
    loop {
        let event = tokio::time::timeout_at(deadline, rx.recv())
            .await
            .expect("timeout waiting for response event")
            .expect("data event channel closed");
        if let DataEvent::Response(response) = event {
            return response;
        }
    }
}

fn page_trade(trade_id: &str) -> Value {
    json!({
        "direction": "buy",
        "index_price": "3500",
        "instrument_name": "ETH-PERP",
        "liquidity_role": "taker",
        "mark_price": "3500",
        "realized_pnl": "0",
        "subaccount_id": 1,
        "timestamp": 1_700_000_000_000_i64,
        "trade_amount": "0.25",
        "trade_fee": "0.01",
        "trade_id": trade_id,
        "trade_price": "3500",
        "tx_hash": "0xhash",
        "tx_status": "settled",
        "wallet": "0xwallet"
    })
}

fn trade_page(num_pages: usize, count: usize, id_prefix: &str) -> Value {
    let trades: Vec<Value> = (0..count)
        .map(|i| page_trade(&format!("{id_prefix}-{i}")))
        .collect();
    json!({
        "trades": trades,
        "pagination": {"count": (num_pages * count) as i64, "num_pages": num_pages as i64},
    })
}

async fn connect_with_eth_currency(rest_addr: SocketAddr, ws_addr: SocketAddr) -> DeriveDataClient {
    let mut cfg = config(rest_addr, ws_addr);
    cfg.currencies = vec!["ETH".to_string()];
    let mut client = DeriveDataClient::new(*DERIVE_CLIENT_ID, cfg).unwrap();
    client.connect().await.unwrap();
    client
}

#[rstest]
#[tokio::test]
async fn test_request_trades_paginates_with_constant_page_size() {
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    *rest_state.trade_history_pages.lock().await = vec![
        trade_page(3, 10, "p1"),
        trade_page(3, 10, "p2"),
        trade_page(3, 10, "p3"),
    ];
    let rest_addr = start_rest_server(rest_state.clone()).await;
    let ws_addr = start_ws_server(ws_state.clone()).await;
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    replace_data_event_sender(tx);

    let client = connect_with_eth_currency(rest_addr, ws_addr).await;

    client
        .request_trades(request_trades(
            InstrumentId::from("ETH-PERP.DERIVE"),
            Some(25),
        ))
        .unwrap();

    let response = recv_response(&mut rx).await;
    let DataResponse::Trades(trades) = response else {
        panic!("expected trades response");
    };
    assert_eq!(trades.data.len(), 25);

    let calls = rest_state.trade_history_calls().await;
    assert_eq!(calls.len(), 3, "must walk all three pages to reach 25");

    let pages: Vec<u64> = calls
        .iter()
        .map(|body| body.get("page").and_then(Value::as_u64).unwrap())
        .collect();
    assert_eq!(pages, vec![1, 2, 3]);

    let page_sizes: Vec<u64> = calls
        .iter()
        .map(|body| body.get("page_size").and_then(Value::as_u64).unwrap())
        .collect();
    assert!(
        page_sizes.windows(2).all(|w| w[0] == w[1]),
        "page_size must be constant across paginated calls, was {page_sizes:?}",
    );
    assert_eq!(page_sizes[0], 25, "page_size should equal capped limit");
}

#[rstest]
#[tokio::test]
async fn test_request_trades_terminates_on_num_pages() {
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    *rest_state.trade_history_pages.lock().await = vec![trade_page(1, 5, "single")];
    let rest_addr = start_rest_server(rest_state.clone()).await;
    let ws_addr = start_ws_server(ws_state.clone()).await;
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    replace_data_event_sender(tx);

    let client = connect_with_eth_currency(rest_addr, ws_addr).await;

    client
        .request_trades(request_trades(InstrumentId::from("ETH-PERP.DERIVE"), None))
        .unwrap();

    let response = recv_response(&mut rx).await;
    let DataResponse::Trades(trades) = response else {
        panic!("expected trades response");
    };
    assert_eq!(trades.data.len(), 5);
    assert_eq!(rest_state.trade_history_calls().await.len(), 1);
}

#[rstest]
#[tokio::test]
async fn test_request_trades_terminates_on_empty_page() {
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    // Declare 5 pages but page 2 returns empty. Loop must break without panicking
    // and emit the page-1 trades.
    *rest_state.trade_history_pages.lock().await = vec![
        trade_page(5, 4, "p1"),
        json!({"trades": [], "pagination": {"count": 4, "num_pages": 5}}),
    ];
    let rest_addr = start_rest_server(rest_state.clone()).await;
    let ws_addr = start_ws_server(ws_state.clone()).await;
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    replace_data_event_sender(tx);

    let client = connect_with_eth_currency(rest_addr, ws_addr).await;

    client
        .request_trades(request_trades(InstrumentId::from("ETH-PERP.DERIVE"), None))
        .unwrap();

    let response = recv_response(&mut rx).await;
    let DataResponse::Trades(trades) = response else {
        panic!("expected trades response");
    };
    assert_eq!(trades.data.len(), 4);
    assert_eq!(rest_state.trade_history_calls().await.len(), 2);
}

#[rstest]
#[tokio::test]
async fn test_request_trades_returns_err_for_missing_instrument() {
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    let rest_addr = start_rest_server(rest_state).await;
    let ws_addr = start_ws_server(ws_state).await;
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    replace_data_event_sender(tx);

    let client = DeriveDataClient::new(*DERIVE_CLIENT_ID, config(rest_addr, ws_addr)).unwrap();

    let err = client
        .request_trades(request_trades(InstrumentId::from("ETH-PERP.DERIVE"), None))
        .expect_err("must reject missing instrument");
    assert!(err.to_string().contains("not found in cache"), "{err}",);
}

#[rstest]
#[tokio::test]
async fn test_request_funding_rates_emits_response_with_records() {
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    *rest_state.funding_rate_history_response.lock().await =
        load_json("perps/http_public_funding_rate_history_eth.json");
    let rest_addr = start_rest_server(rest_state.clone()).await;
    let ws_addr = start_ws_server(ws_state.clone()).await;
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    replace_data_event_sender(tx);

    let client = connect_with_eth_currency(rest_addr, ws_addr).await;

    client
        .request_funding_rates(request_funding_rates(
            InstrumentId::from("ETH-PERP.DERIVE"),
            None,
        ))
        .unwrap();

    let response = recv_response(&mut rx).await;
    let DataResponse::FundingRates(rates) = response else {
        panic!("expected funding rates response");
    };
    assert_eq!(rates.data.len(), 3);
    assert_eq!(rates.data[0].rate.to_string(), "0.00012");
    assert_eq!(
        rates.data[0].ts_event,
        UnixNanos::from(1_700_000_000_000_000_000)
    );
    assert_eq!(rates.data[2].rate.to_string(), "0.00011");
    assert_eq!(
        rates.data[2].ts_event,
        UnixNanos::from(1_700_007_200_000_000_000)
    );

    let calls = rest_state.funding_rate_history_calls().await;
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0]["instrument_name"], "ETH-PERP");
}

#[rstest]
#[tokio::test]
async fn test_request_funding_rates_returns_err_for_non_perp() {
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    let rest_addr = start_rest_server(rest_state).await;
    let ws_addr = start_ws_server(ws_state.clone()).await;
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    replace_data_event_sender(tx);

    let client = connect_with_eth_currency(rest_addr, ws_addr).await;

    let err = client
        .request_funding_rates(request_funding_rates(
            InstrumentId::from("ETH-20260627-3500-C.DERIVE"),
            None,
        ))
        .expect_err("must reject non-perp instrument");
    assert!(
        err.to_string()
            .contains("only available for Derive perpetual"),
        "{err}",
    );
}

#[rstest]
#[tokio::test]
async fn test_request_funding_rates_returns_err_for_missing_instrument() {
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    let rest_addr = start_rest_server(rest_state).await;
    let ws_addr = start_ws_server(ws_state).await;
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    replace_data_event_sender(tx);

    let client = DeriveDataClient::new(*DERIVE_CLIENT_ID, config(rest_addr, ws_addr)).unwrap();

    let err = client
        .request_funding_rates(request_funding_rates(
            InstrumentId::from("ETH-PERP.DERIVE"),
            None,
        ))
        .expect_err("must reject missing instrument");
    assert!(err.to_string().contains("not found in cache"), "{err}",);
}

#[rstest]
#[tokio::test]
async fn test_request_bars_emits_response_with_records() {
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    *rest_state.candles_response.lock().await = load_json("perps/http_public_candles_eth.json");
    let rest_addr = start_rest_server(rest_state.clone()).await;
    let ws_addr = start_ws_server(ws_state.clone()).await;
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    replace_data_event_sender(tx);

    let client = connect_with_eth_currency(rest_addr, ws_addr).await;
    let bar_type = BarType::from("ETH-PERP.DERIVE-15-MINUTE-LAST-EXTERNAL");

    client.request_bars(request_bars(bar_type, None)).unwrap();

    let response = recv_response(&mut rx).await;
    let DataResponse::Bars(bars) = response else {
        panic!("expected bars response");
    };
    assert_eq!(bars.bar_type, bar_type);
    assert_eq!(bars.data.len(), 3);
    assert_eq!(bars.data[0].open, Price::from("3500.00"));
    assert_eq!(bars.data[0].high, Price::from("3501.50"));
    assert_eq!(bars.data[0].low, Price::from("3499.00"));
    assert_eq!(bars.data[0].close, Price::from("3501.00"));
    // Volume must match `volume_contracts` (3.527), not `volume_usd` (12345.6)
    assert_eq!(bars.data[0].volume, Quantity::from("3.527"));
    assert_eq!(
        bars.data[0].ts_event,
        UnixNanos::from(1_700_000_000_000_000_000),
    );
    assert_eq!(
        bars.data[2].ts_event,
        UnixNanos::from(1_700_001_800_000_000_000),
    );

    let calls = rest_state.candles_calls().await;
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0]["instrument_name"], "ETH-PERP");
    // Derive's `period` enum is bucket size in seconds: 15 MINUTE -> 900
    assert_eq!(calls[0]["period"], 900);

    // Default window when start/end are None: end_ts = now, start_ts =
    // end_ts - period * DERIVE_CANDLES_DEFAULT_LIMIT. Pin the span so any
    // mutation of the default_span math is caught.
    let start_ts = calls[0]["start_timestamp"].as_i64().unwrap();
    let end_ts = calls[0]["end_timestamp"].as_i64().unwrap();
    assert!(start_ts < end_ts, "start_ts={start_ts} end_ts={end_ts}");
    assert_eq!(end_ts - start_ts, 900 * 1000);
}

#[rstest]
#[tokio::test]
async fn test_request_bars_honors_limit() {
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    *rest_state.candles_response.lock().await = load_json("perps/http_public_candles_eth.json");
    let rest_addr = start_rest_server(rest_state.clone()).await;
    let ws_addr = start_ws_server(ws_state.clone()).await;
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    replace_data_event_sender(tx);

    let client = connect_with_eth_currency(rest_addr, ws_addr).await;
    let bar_type = BarType::from("ETH-PERP.DERIVE-15-MINUTE-LAST-EXTERNAL");

    client
        .request_bars(request_bars(bar_type, Some(2)))
        .unwrap();

    let response = recv_response(&mut rx).await;
    let DataResponse::Bars(bars) = response else {
        panic!("expected bars response");
    };
    assert_eq!(bars.data.len(), 2);
}

#[rstest]
#[tokio::test]
async fn test_request_bars_returns_err_for_missing_instrument() {
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    let rest_addr = start_rest_server(rest_state).await;
    let ws_addr = start_ws_server(ws_state).await;
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    replace_data_event_sender(tx);

    let client = DeriveDataClient::new(*DERIVE_CLIENT_ID, config(rest_addr, ws_addr)).unwrap();
    let bar_type = BarType::from("ETH-PERP.DERIVE-1-MINUTE-LAST-EXTERNAL");

    let err = client
        .request_bars(request_bars(bar_type, None))
        .expect_err("must reject missing instrument");
    assert!(err.to_string().contains("not found in cache"), "{err}",);
}

#[rstest]
#[tokio::test]
async fn test_request_bars_returns_err_for_non_external_source() {
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    let rest_addr = start_rest_server(rest_state).await;
    let ws_addr = start_ws_server(ws_state.clone()).await;
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    replace_data_event_sender(tx);

    let client = connect_with_eth_currency(rest_addr, ws_addr).await;
    let bar_type = BarType::from("ETH-PERP.DERIVE-1-MINUTE-LAST-INTERNAL");

    let err = client
        .request_bars(request_bars(bar_type, None))
        .expect_err("must reject INTERNAL aggregation source");
    assert!(
        err.to_string().contains("EXTERNAL aggregation source"),
        "{err}",
    );
}

#[rstest]
#[tokio::test]
async fn test_request_bars_returns_err_for_non_last_price_type() {
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    let rest_addr = start_rest_server(rest_state).await;
    let ws_addr = start_ws_server(ws_state.clone()).await;
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    replace_data_event_sender(tx);

    let client = connect_with_eth_currency(rest_addr, ws_addr).await;
    let bar_type = BarType::from("ETH-PERP.DERIVE-1-MINUTE-MID-EXTERNAL");

    let err = client
        .request_bars(request_bars(bar_type, None))
        .expect_err("must reject non-LAST price type");
    assert!(err.to_string().contains("PriceType::Last"), "{err}",);
}

#[rstest]
#[tokio::test]
async fn test_request_bars_returns_err_for_unsupported_spec() {
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    let rest_addr = start_rest_server(rest_state).await;
    let ws_addr = start_ws_server(ws_state.clone()).await;
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    replace_data_event_sender(tx);

    let client = connect_with_eth_currency(rest_addr, ws_addr).await;
    // Derive's enum tops out at WEEK1; SECOND-aggregated bars are unsupported
    let bar_type = BarType::from("ETH-PERP.DERIVE-30-SECOND-LAST-EXTERNAL");

    let err = client
        .request_bars(request_bars(bar_type, None))
        .expect_err("must reject unsupported bar spec");
    assert!(
        err.to_string().contains("unsupported Derive bar spec"),
        "{err}",
    );
}

#[rstest]
#[tokio::test]
async fn test_request_bars_drops_response_on_rest_error() {
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    // Configure the mock so `get_candles` returns an Err. The production
    // contract under test is "any REST error must drop the BarsResponse",
    // independent of which DeriveHttpError variant the handler surfaced.
    *rest_state.candles_response.lock().await = json!({
        "id": 1,
        "error": {"code": -32602, "message": "boom"},
    });
    let rest_addr = start_rest_server(rest_state.clone()).await;
    let ws_addr = start_ws_server(ws_state.clone()).await;
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    replace_data_event_sender(tx);

    let client = connect_with_eth_currency(rest_addr, ws_addr).await;
    let bar_type = BarType::from("ETH-PERP.DERIVE-15-MINUTE-LAST-EXTERNAL");

    client.request_bars(request_bars(bar_type, None)).unwrap();

    // Wait until the REST call lands on the mock so the test is robust
    // against scheduling delays between runtimes.
    {
        let state = rest_state.clone();
        wait_until_async(
            move || {
                let state = state.clone();
                async move { !state.candles_calls.lock().await.is_empty() }
            },
            Duration::from_secs(5),
        )
        .await;
    }

    // After the failed REST call, the handler logs and returns without
    // emitting a BarsResponse. Verify nothing arrives within a window
    // generous enough to survive scheduling delays on slow CI runners.
    let received = tokio::time::timeout(Duration::from_secs(1), rx.recv()).await;
    if let Ok(Some(DataEvent::Response(DataResponse::Bars(_)))) = received {
        panic!("BarsResponse must be dropped when get_candles fails");
    }
}

#[rstest]
#[tokio::test]
async fn test_request_bars_walks_multiple_pages_to_start() {
    // Three pages of three 1-minute bars; the mock keys responses by the
    // request's `end_timestamp` so the test fails fast if the backwards-walk
    // math changes (next end_ts must equal earliest_bucket - 1).
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    let period: i64 = 60;
    let start_secs: i64 = 1_700_000_000;
    let end_secs: i64 = start_secs + period * 8;

    {
        let mut pages = rest_state.candles_pages.lock().await;
        pages.insert(
            end_secs,
            Value::Array(vec![
                candle_json(start_secs + period * 6),
                candle_json(start_secs + period * 7),
                candle_json(start_secs + period * 8),
            ]),
        );
        pages.insert(
            start_secs + period * 6 - 1,
            Value::Array(vec![
                candle_json(start_secs + period * 3),
                candle_json(start_secs + period * 4),
                candle_json(start_secs + period * 5),
            ]),
        );
        pages.insert(
            start_secs + period * 3 - 1,
            Value::Array(vec![
                candle_json(start_secs),
                candle_json(start_secs + period),
                candle_json(start_secs + period * 2),
            ]),
        );
    }

    let rest_addr = start_rest_server(rest_state.clone()).await;
    let ws_addr = start_ws_server(ws_state).await;
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    replace_data_event_sender(tx);

    let client = connect_with_eth_currency(rest_addr, ws_addr).await;
    let bar_type = BarType::from("ETH-PERP.DERIVE-1-MINUTE-LAST-EXTERNAL");

    client
        .request_bars(request_bars_window(
            bar_type,
            Some(datetime_from_secs(start_secs)),
            Some(datetime_from_secs(end_secs)),
            None,
        ))
        .unwrap();

    let response = recv_response(&mut rx).await;
    let DataResponse::Bars(bars) = response else {
        panic!("expected bars response");
    };

    assert_eq!(bars.data.len(), 9);

    for (i, bar) in bars.data.iter().enumerate() {
        let expected_secs = start_secs + period * i as i64;

        assert_eq!(
            bar.ts_event,
            UnixNanos::from((expected_secs as u64) * 1_000_000_000),
            "bar {i} ts_event mismatch",
        );
    }

    let calls = rest_state.candles_calls().await;
    assert_eq!(calls.len(), 3);
    assert_eq!(calls[0]["end_timestamp"], end_secs);
    assert_eq!(calls[1]["end_timestamp"], start_secs + period * 6 - 1);
    assert_eq!(calls[2]["end_timestamp"], start_secs + period * 3 - 1);
    assert!(
        calls.iter().all(|c| c["start_timestamp"] == start_secs),
        "start_timestamp must remain pinned across pages",
    );
}

#[rstest]
#[tokio::test]
async fn test_request_bars_honors_limit_across_pages() {
    // limit=5 must cap the walk: page 1 yields 3, page 2 yields 3, total
    // 6 >= 5 triggers loop exit, then the leading bar is dropped so only
    // the 5 most recent survive.
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    let period: i64 = 60;
    let start_secs: i64 = 1_700_000_000;
    let end_secs: i64 = start_secs + period * 8;

    {
        let mut pages = rest_state.candles_pages.lock().await;
        pages.insert(
            end_secs,
            Value::Array(vec![
                candle_json(start_secs + period * 6),
                candle_json(start_secs + period * 7),
                candle_json(start_secs + period * 8),
            ]),
        );
        pages.insert(
            start_secs + period * 6 - 1,
            Value::Array(vec![
                candle_json(start_secs + period * 3),
                candle_json(start_secs + period * 4),
                candle_json(start_secs + period * 5),
            ]),
        );
        pages.insert(
            start_secs + period * 3 - 1,
            Value::Array(vec![
                candle_json(start_secs),
                candle_json(start_secs + period),
                candle_json(start_secs + period * 2),
            ]),
        );
    }

    let rest_addr = start_rest_server(rest_state.clone()).await;
    let ws_addr = start_ws_server(ws_state).await;
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    replace_data_event_sender(tx);

    let client = connect_with_eth_currency(rest_addr, ws_addr).await;
    let bar_type = BarType::from("ETH-PERP.DERIVE-1-MINUTE-LAST-EXTERNAL");

    client
        .request_bars(request_bars_window(
            bar_type,
            Some(datetime_from_secs(start_secs)),
            Some(datetime_from_secs(end_secs)),
            Some(5),
        ))
        .unwrap();

    let response = recv_response(&mut rx).await;
    let DataResponse::Bars(bars) = response else {
        panic!("expected bars response");
    };

    assert_eq!(bars.data.len(), 5);
    // Most recent 5 bars survive after dropping the oldest from page 2.
    assert_eq!(
        bars.data[0].ts_event,
        UnixNanos::from(((start_secs + period * 4) as u64) * 1_000_000_000),
    );
    assert_eq!(
        bars.data[4].ts_event,
        UnixNanos::from(((start_secs + period * 8) as u64) * 1_000_000_000),
    );

    let calls = rest_state.candles_calls().await;
    assert_eq!(calls.len(), 2);
}

#[rstest]
#[tokio::test]
async fn test_request_bars_dedups_overlapping_pages() {
    // The venue is contractually expected to return non-overlapping windows,
    // but the dedup AHashSet defends against any future regression. Page 2
    // deliberately repeats page 1's earliest bucket.
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    let period: i64 = 60;
    let start_secs: i64 = 1_700_000_000;
    let end_secs: i64 = start_secs + period * 5;

    {
        let mut pages = rest_state.candles_pages.lock().await;
        pages.insert(
            end_secs,
            Value::Array(vec![
                candle_json(start_secs + period * 3),
                candle_json(start_secs + period * 4),
                candle_json(start_secs + period * 5),
            ]),
        );
        pages.insert(
            start_secs + period * 3 - 1,
            Value::Array(vec![
                candle_json(start_secs),
                candle_json(start_secs + period),
                candle_json(start_secs + period * 2),
                // Overlap with page 1's earliest bucket; must be deduped.
                candle_json(start_secs + period * 3),
            ]),
        );
    }

    let rest_addr = start_rest_server(rest_state.clone()).await;
    let ws_addr = start_ws_server(ws_state).await;
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    replace_data_event_sender(tx);

    let client = connect_with_eth_currency(rest_addr, ws_addr).await;
    let bar_type = BarType::from("ETH-PERP.DERIVE-1-MINUTE-LAST-EXTERNAL");

    client
        .request_bars(request_bars_window(
            bar_type,
            Some(datetime_from_secs(start_secs)),
            Some(datetime_from_secs(end_secs)),
            None,
        ))
        .unwrap();

    let response = recv_response(&mut rx).await;
    let DataResponse::Bars(bars) = response else {
        panic!("expected bars response");
    };

    assert_eq!(bars.data.len(), 6, "dedup must drop the repeated bucket");

    let mut seen: HashSet<UnixNanos> = HashSet::new();

    for bar in &bars.data {
        assert!(
            seen.insert(bar.ts_event),
            "duplicate ts_event {:?}",
            bar.ts_event
        );
    }
}

#[rstest]
#[tokio::test]
async fn test_request_bars_terminates_at_safety_cap() {
    // The synthetic mock returns one bar per call with timestamp = end_ts,
    // so each iteration advances `current_end` by exactly `period`. With
    // start_ts far enough in the past, the walk would run forever without
    // the safety cap; assert it stops at 100 pages.
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    let period: i64 = 60;
    let end_secs: i64 = 1_700_000_000 + period * 200;
    let start_secs: i64 = 1_700_000_000;
    *rest_state.candles_generated_per_call.lock().await = Some(1);

    let rest_addr = start_rest_server(rest_state.clone()).await;
    let ws_addr = start_ws_server(ws_state).await;
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    replace_data_event_sender(tx);

    let client = connect_with_eth_currency(rest_addr, ws_addr).await;
    let bar_type = BarType::from("ETH-PERP.DERIVE-1-MINUTE-LAST-EXTERNAL");

    client
        .request_bars(request_bars_window(
            bar_type,
            Some(datetime_from_secs(start_secs)),
            Some(datetime_from_secs(end_secs)),
            None,
        ))
        .unwrap();

    // The 100-page walk is paced by the non-matching REST quota (10/s after the
    // 50-request burst), so it takes ~5s; allow well beyond that.
    let response = recv_response_within(&mut rx, Duration::from_secs(15)).await;
    let DataResponse::Bars(bars) = response else {
        panic!("expected bars response");
    };

    assert_eq!(bars.data.len(), 100);
    let calls = rest_state.candles_calls().await;
    assert_eq!(calls.len(), 100);
}

#[rstest]
#[tokio::test]
async fn test_request_forward_prices_emits_response_with_record() {
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    *rest_state.ticker_response.lock().await = load_json("options/http_ticker_eth_snapshot.json");
    let rest_addr = start_rest_server(rest_state.clone()).await;
    let ws_addr = start_ws_server(ws_state.clone()).await;
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    replace_data_event_sender(tx);

    let client = connect_with_eth_currency(rest_addr, ws_addr).await;
    let instrument_id = InstrumentId::from("ETH-20260627-3500-C.DERIVE");

    client
        .request_forward_prices(request_forward_prices("ETH", Some(instrument_id)))
        .unwrap();

    let response = recv_response(&mut rx).await;
    let DataResponse::ForwardPrices(forward) = response else {
        panic!("expected forward prices response");
    };
    assert_eq!(forward.venue, *DERIVE_VENUE);
    assert_eq!(forward.data.len(), 1);
    assert_eq!(forward.data[0].instrument_id, instrument_id);
    assert_eq!(forward.data[0].forward_price.to_string(), "3505");
    assert_eq!(forward.data[0].underlying_index.as_deref(), Some("ETH"));

    let calls = rest_state.ticker_calls().await;
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0]["instrument_type"], "option");
    assert_eq!(calls[0]["currency"], "ETH");
    assert_eq!(calls[0]["expiry_date"], "20260627");
}

#[rstest]
#[tokio::test]
async fn test_request_forward_prices_propagates_request_venue() {
    // The response venue must come from the request, not a hard-coded constant.
    // Build the request with a synthetic venue and assert it round-trips.
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    *rest_state.ticker_response.lock().await = load_json("options/http_ticker_eth_snapshot.json");
    let rest_addr = start_rest_server(rest_state.clone()).await;
    let ws_addr = start_ws_server(ws_state.clone()).await;
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    replace_data_event_sender(tx);

    let client = connect_with_eth_currency(rest_addr, ws_addr).await;
    let instrument_id = InstrumentId::from("ETH-20260627-3500-C.DERIVE");
    let other_venue = Venue::from("OTHER");
    let request = RequestForwardPrices::new(
        other_venue,
        Ustr::from("ETH"),
        Some(instrument_id),
        Some(*DERIVE_CLIENT_ID),
        UUID4::new(),
        UnixNanos::default(),
        None,
    );

    client.request_forward_prices(request).unwrap();

    let response = recv_response(&mut rx).await;
    let DataResponse::ForwardPrices(forward) = response else {
        panic!("expected forward prices response");
    };
    assert_eq!(forward.venue, other_venue);
}

#[rstest]
#[tokio::test]
async fn test_request_forward_prices_uses_request_underlying_for_index() {
    // `public/get_tickers` carries option pricing but not option reference
    // details, so the response uses the request's underlying for the index.
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    *rest_state.ticker_response.lock().await = load_json("options/http_ticker_eth_snapshot.json");
    let rest_addr = start_rest_server(rest_state.clone()).await;
    let ws_addr = start_ws_server(ws_state.clone()).await;
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    replace_data_event_sender(tx);

    let client = connect_with_eth_currency(rest_addr, ws_addr).await;
    let instrument_id = InstrumentId::from("ETH-20260627-3500-C.DERIVE");

    client
        .request_forward_prices(request_forward_prices("ETH", Some(instrument_id)))
        .unwrap();

    let response = recv_response(&mut rx).await;
    let DataResponse::ForwardPrices(forward) = response else {
        panic!("expected forward prices response");
    };
    assert_eq!(forward.data.len(), 1);
    assert_eq!(forward.data[0].underlying_index.as_deref(), Some("ETH"));
}

#[rstest]
#[tokio::test]
async fn test_request_forward_prices_returns_err_for_missing_instrument_id() {
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    let rest_addr = start_rest_server(rest_state).await;
    let ws_addr = start_ws_server(ws_state.clone()).await;
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    replace_data_event_sender(tx);

    let client = connect_with_eth_currency(rest_addr, ws_addr).await;

    let err = client
        .request_forward_prices(request_forward_prices("ETH", None))
        .expect_err("must reject bulk request");
    assert!(
        err.to_string().contains("requires an `instrument_id`"),
        "{err}",
    );
}

#[rstest]
#[tokio::test]
async fn test_request_forward_prices_returns_err_for_non_option_instrument() {
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    let rest_addr = start_rest_server(rest_state).await;
    let ws_addr = start_ws_server(ws_state.clone()).await;
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    replace_data_event_sender(tx);

    let client = connect_with_eth_currency(rest_addr, ws_addr).await;
    let perp_id = InstrumentId::from("ETH-PERP.DERIVE");

    let err = client
        .request_forward_prices(request_forward_prices("ETH", Some(perp_id)))
        .expect_err("must reject non-option instrument");
    assert!(
        err.to_string().contains("only meaningful for options"),
        "{err}",
    );
}

#[rstest]
#[tokio::test]
async fn test_request_forward_prices_emits_empty_response_on_rest_error() {
    // The engine waits for this response before creating the OptionChainManager.
    // On REST failure we must still emit an (empty) ForwardPricesResponse so the
    // engine can fall back to live-tick bootstrap.
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    *rest_state.ticker_response.lock().await = json!({
        "id": 1,
        "error": {"code": -32602, "message": "boom"},
    });
    let rest_addr = start_rest_server(rest_state.clone()).await;
    let ws_addr = start_ws_server(ws_state.clone()).await;
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    replace_data_event_sender(tx);

    let client = connect_with_eth_currency(rest_addr, ws_addr).await;
    let instrument_id = InstrumentId::from("ETH-20260627-3500-C.DERIVE");

    client
        .request_forward_prices(request_forward_prices("ETH", Some(instrument_id)))
        .unwrap();

    let response = recv_response(&mut rx).await;
    let DataResponse::ForwardPrices(forward) = response else {
        panic!("expected forward prices response");
    };
    assert!(
        forward.data.is_empty(),
        "must emit empty data on REST error"
    );
    assert_eq!(forward.venue, *DERIVE_VENUE);
}

#[rstest]
#[tokio::test]
async fn test_request_forward_prices_emits_empty_response_when_ticker_lacks_option_pricing() {
    // Non-option ticker (perp snapshot fixture) has no option_pricing. The
    // engine must still get a response to unblock the OptionChainManager.
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    *rest_state.ticker_response.lock().await = load_json("perps/http_ticker_eth_snapshot.json");
    let rest_addr = start_rest_server(rest_state.clone()).await;
    let ws_addr = start_ws_server(ws_state.clone()).await;
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    replace_data_event_sender(tx);

    let client = connect_with_eth_currency(rest_addr, ws_addr).await;
    let instrument_id = InstrumentId::from("ETH-20260627-3500-C.DERIVE");

    client
        .request_forward_prices(request_forward_prices("ETH", Some(instrument_id)))
        .unwrap();

    let response = recv_response(&mut rx).await;
    let DataResponse::ForwardPrices(forward) = response else {
        panic!("expected forward prices response");
    };
    assert!(
        forward.data.is_empty(),
        "must emit empty data when option_pricing is absent"
    );
}

#[rstest]
#[tokio::test]
async fn test_request_forward_prices_returns_err_for_missing_instrument() {
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    let rest_addr = start_rest_server(rest_state).await;
    let ws_addr = start_ws_server(ws_state).await;
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    replace_data_event_sender(tx);

    let client = DeriveDataClient::new(*DERIVE_CLIENT_ID, config(rest_addr, ws_addr)).unwrap();
    let instrument_id = InstrumentId::from("ETH-20260627-3500-C.DERIVE");

    let err = client
        .request_forward_prices(request_forward_prices("ETH", Some(instrument_id)))
        .expect_err("must reject missing instrument");
    assert!(err.to_string().contains("not found in cache"), "{err}",);
}

#[rstest]
#[tokio::test]
async fn test_data_client_connect_disconnect() {
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    let rest_addr = start_rest_server(rest_state).await;
    let ws_addr = start_ws_server(ws_state).await;
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    replace_data_event_sender(tx);

    let mut client = DeriveDataClient::new(*DERIVE_CLIENT_ID, config(rest_addr, ws_addr)).unwrap();
    assert!(!client.is_connected());

    client.connect().await.unwrap();
    assert!(client.is_connected());

    client.disconnect().await.unwrap();
    assert!(!client.is_connected());
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn test_data_client_reconnect_after_disconnect_resumes_stream() {
    // After disconnect() cancels the token, a fresh connect() must refresh
    // it. Otherwise the new WS consumption loop spawns onto a cancelled
    // token and exits immediately: outbound subscribe frames still go out
    // (they bypass the consumption loop), but no inbound data reaches the
    // data_sender. This test pins both halves of the lifecycle by waiting
    // for a Data::Trade event on the reconnected session, which only
    // arrives if the consumption loop is alive.
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    let rest_addr = start_rest_server(rest_state.clone()).await;
    let ws_addr = start_ws_server(ws_state.clone()).await;
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    replace_data_event_sender(tx);

    let mut client = connect_with_eth_currency(rest_addr, ws_addr).await;
    assert!(client.is_connected());
    client.disconnect().await.unwrap();
    assert!(!client.is_connected());

    while rx.try_recv().is_ok() {}

    client.connect().await.unwrap();
    assert!(client.is_connected());

    while rx.try_recv().is_ok() {}

    let option_id = InstrumentId::from("ETH-20260627-3500-C.DERIVE");
    client
        .subscribe_trades(subscribe_trades(option_id))
        .unwrap();
    wait_for_subscribe(&ws_state, "trades.option.ETH").await;

    // The mock pushes a synthetic trade notification on every subscribe.
    // If the consumption loop is dead (regressed token-refresh fix), the
    // notification is silently dropped and recv_data times out.
    match recv_data(&mut rx).await {
        Data::Trade(trade) => assert_eq!(trade.instrument_id, option_id),
        other => panic!("expected trade data, was {other:?}"),
    }

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn test_data_client_disconnect_stops_event_flow() {
    // disconnect() must abort tracked request tasks so no late responses
    // arrive after the call returns. Use request_trades because it pages
    // and would otherwise keep firing after the WS is torn down.
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    *rest_state.trade_history_pages.lock().await = vec![trade_page(3, 10, "p1")];
    let rest_addr = start_rest_server(rest_state.clone()).await;
    let ws_addr = start_ws_server(ws_state.clone()).await;
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    replace_data_event_sender(tx);

    let client = connect_with_eth_currency(rest_addr, ws_addr).await;
    let instrument_id = InstrumentId::from("ETH-PERP.DERIVE");

    client
        .request_trades(request_trades(instrument_id, None))
        .unwrap();

    // Tear down before the request can possibly drain all 3 pages
    let mut client = client;
    let disconnect_deadline = Duration::from_secs(5);
    tokio::time::timeout(disconnect_deadline, client.disconnect())
        .await
        .expect("disconnect must complete promptly even with in-flight requests")
        .unwrap();
    assert!(!client.is_connected());

    while rx.try_recv().is_ok() {}

    let quiet_window = Duration::from_millis(500);
    let maybe_event = tokio::time::timeout(quiet_window, rx.recv()).await;
    assert!(
        maybe_event.is_err(),
        "no data events should arrive after disconnect, was: {maybe_event:?}",
    );
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn test_data_client_reset_after_subscribe_clears_state() {
    // reset() on a live client with the WS stream up and a tracked
    // subscribe task must succeed without panic, and the client must
    // accept a fresh connect() that resumes data flow. Exercises the
    // `abort_pending_tasks()` and `ws_stream_handle.take().abort()`
    // branches added in reset(), plus the connect()-time WS teardown
    // that rebuilds out_rx after a reset.
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    let rest_addr = start_rest_server(rest_state).await;
    let ws_addr = start_ws_server(ws_state.clone()).await;
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    replace_data_event_sender(tx);

    let mut client = connect_with_eth_currency(rest_addr, ws_addr).await;
    let option_id = InstrumentId::from("ETH-20260627-3500-C.DERIVE");
    client
        .subscribe_trades(subscribe_trades(option_id))
        .unwrap();

    client.reset().unwrap();
    assert!(!client.is_connected());

    while rx.try_recv().is_ok() {}

    client.connect().await.unwrap();
    assert!(client.is_connected());

    while rx.try_recv().is_ok() {}

    client
        .subscribe_trades(subscribe_trades(option_id))
        .unwrap();
    wait_for_subscribe(&ws_state, "trades.option.ETH").await;

    match recv_data(&mut rx).await {
        Data::Trade(trade) => assert_eq!(trade.instrument_id, option_id),
        other => panic!("expected trade data, was {other:?}"),
    }

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test(flavor = "multi_thread")]
async fn test_data_client_stop_then_connect_clears_stale_subscriptions() {
    // Regression: stop() cancels the token without clearing local sub state,
    // so a follow-up connect() must clear the active_* maps when it tears
    // down the WS client. Otherwise a resubscribe hits the early-return on
    // the stale entry and no new WS frame is sent on the fresh socket.
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    let rest_addr = start_rest_server(rest_state).await;
    let ws_addr = start_ws_server(ws_state.clone()).await;
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    replace_data_event_sender(tx);

    let mut client = connect_with_eth_currency(rest_addr, ws_addr).await;
    let option_id = InstrumentId::from("ETH-20260627-3500-C.DERIVE");
    client
        .subscribe_trades(subscribe_trades(option_id))
        .unwrap();
    wait_for_subscribe(&ws_state, "trades.option.ETH").await;

    client.stop().unwrap();
    assert!(!client.is_connected());

    while rx.try_recv().is_ok() {}
    ws_state.subscribe_frames.lock().await.clear();

    client.connect().await.unwrap();
    assert!(client.is_connected());

    while rx.try_recv().is_ok() {}

    // A fresh subscribe must reach the WS server, proving the active_*
    // entry from before stop() did not silently suppress the new call.
    client
        .subscribe_trades(subscribe_trades(option_id))
        .unwrap();
    wait_for_subscribe(&ws_state, "trades.option.ETH").await;

    match recv_data(&mut rx).await {
        Data::Trade(trade) => assert_eq!(trade.instrument_id, option_id),
        other => panic!("expected trade data, was {other:?}"),
    }

    client.disconnect().await.unwrap();
}

fn request_instrument(instrument_id: InstrumentId) -> RequestInstrument {
    RequestInstrument::new(
        instrument_id,
        None,
        None,
        Some(*DERIVE_CLIENT_ID),
        UUID4::new(),
        UnixNanos::default(),
        None,
    )
}

fn request_quotes(instrument_id: InstrumentId) -> RequestQuotes {
    RequestQuotes::new(
        instrument_id,
        None,
        None,
        None,
        Some(*DERIVE_CLIENT_ID),
        UUID4::new(),
        UnixNanos::default(),
        None,
    )
}

fn request_quotes_window(
    instrument_id: InstrumentId,
    start: Option<DateTime<Utc>>,
    end: Option<DateTime<Utc>>,
) -> RequestQuotes {
    RequestQuotes::new(
        instrument_id,
        start,
        end,
        None,
        Some(*DERIVE_CLIENT_ID),
        UUID4::new(),
        UnixNanos::default(),
        None,
    )
}

#[rstest]
#[tokio::test]
async fn test_request_instrument_emits_response_and_caches() {
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    let snapshot = load_json("perps/http_get_instrument_eth.json");
    let payload = snapshot
        .get("result")
        .cloned()
        .expect("fixture missing `result`");
    *rest_state.instrument_response.lock().await = payload;
    let rest_addr = start_rest_server(rest_state.clone()).await;
    let ws_addr = start_ws_server(ws_state.clone()).await;
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    replace_data_event_sender(tx);

    let client = DeriveDataClient::new(*DERIVE_CLIENT_ID, config(rest_addr, ws_addr)).unwrap();
    let instrument_id = InstrumentId::from("ETH-PERP.DERIVE");

    client
        .request_instrument(request_instrument(instrument_id))
        .unwrap();

    let response = recv_response(&mut rx).await;
    let DataResponse::Instrument(boxed) = response else {
        panic!("expected instrument response");
    };
    assert_eq!(boxed.instrument_id, instrument_id);
    assert_eq!(boxed.data.id(), instrument_id);
    assert_eq!(boxed.data.price_precision(), 2);
    assert_eq!(boxed.data.size_precision(), 3);
    assert_eq!(boxed.data.price_increment(), Price::from("0.01"));
    assert_eq!(boxed.data.size_increment(), Quantity::from("0.001"));

    let calls = rest_state.instrument_calls().await;
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0]["instrument_name"], "ETH-PERP");

    // request_quotes returns Err when the instrument is not in the cache, so
    // accepting the call proves the handler populated `self.instruments`.
    client
        .request_quotes(request_quotes(instrument_id))
        .expect("instrument must be cached after request_instrument success");
}

#[rstest]
#[tokio::test]
async fn test_request_instrument_drops_response_on_rest_error() {
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    *rest_state.instrument_response.lock().await = json!({
        "id": 1,
        "error": {"code": -32602, "message": "boom"},
    });
    let rest_addr = start_rest_server(rest_state.clone()).await;
    let ws_addr = start_ws_server(ws_state.clone()).await;
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    replace_data_event_sender(tx);

    let client = DeriveDataClient::new(*DERIVE_CLIENT_ID, config(rest_addr, ws_addr)).unwrap();
    let instrument_id = InstrumentId::from("ETH-PERP.DERIVE");

    client
        .request_instrument(request_instrument(instrument_id))
        .unwrap();

    {
        let state = rest_state.clone();
        wait_until_async(
            move || {
                let state = state.clone();
                async move { !state.instrument_calls.lock().await.is_empty() }
            },
            Duration::from_secs(5),
        )
        .await;
    }

    let received = tokio::time::timeout(Duration::from_secs(1), rx.recv()).await;
    if let Ok(Some(DataEvent::Response(DataResponse::Instrument(_)))) = received {
        panic!("InstrumentResponse must be dropped when get_instrument fails");
    }
}

#[rstest]
#[tokio::test]
async fn test_request_instrument_returns_err_for_non_derive_venue() {
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    let rest_addr = start_rest_server(rest_state).await;
    let ws_addr = start_ws_server(ws_state).await;
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    replace_data_event_sender(tx);

    let client = DeriveDataClient::new(*DERIVE_CLIENT_ID, config(rest_addr, ws_addr)).unwrap();
    let foreign_id = InstrumentId::from("ETH-PERP.OTHER");

    let err = client
        .request_instrument(request_instrument(foreign_id))
        .expect_err("must reject non-Derive venue");
    assert!(err.to_string().contains("not for venue"), "{err}");
}

#[rstest]
#[tokio::test]
async fn test_request_quotes_emits_response_with_single_tick() {
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    *rest_state.ticker_response.lock().await = load_json("perps/http_ticker_eth_snapshot.json");
    let rest_addr = start_rest_server(rest_state.clone()).await;
    let ws_addr = start_ws_server(ws_state.clone()).await;
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    replace_data_event_sender(tx);

    let client = connect_with_eth_currency(rest_addr, ws_addr).await;
    let instrument_id = InstrumentId::from("ETH-PERP.DERIVE");

    client
        .request_quotes(request_quotes(instrument_id))
        .unwrap();

    let response = recv_response(&mut rx).await;
    let DataResponse::Quotes(quotes) = response else {
        panic!("expected quotes response");
    };
    assert_eq!(quotes.instrument_id, instrument_id);
    assert_eq!(quotes.data.len(), 1);
    assert_eq!(quotes.data[0].bid_price, Price::from("3499.50"));
    assert_eq!(quotes.data[0].ask_price, Price::from("3501.00"));
    assert_eq!(quotes.data[0].bid_size, Quantity::from("0.800"));
    assert_eq!(quotes.data[0].ask_size, Quantity::from("1.200"));

    let calls = rest_state.ticker_calls().await;
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0]["instrument_type"], "perp");
    assert_eq!(calls[0]["currency"], "ETH");
    assert!(calls[0].get("expiry_date").is_none());
}

#[rstest]
#[tokio::test]
async fn test_request_quotes_drops_response_on_rest_error() {
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    *rest_state.ticker_response.lock().await = json!({
        "id": 1,
        "error": {"code": -32602, "message": "boom"},
    });
    let rest_addr = start_rest_server(rest_state.clone()).await;
    let ws_addr = start_ws_server(ws_state.clone()).await;
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    replace_data_event_sender(tx);

    let client = connect_with_eth_currency(rest_addr, ws_addr).await;
    // Drain connect-time Instrument events so the quiet-window check below
    // cannot consume one in place of a missing QuotesResponse.
    while rx.try_recv().is_ok() {}

    let instrument_id = InstrumentId::from("ETH-PERP.DERIVE");

    client
        .request_quotes(request_quotes(instrument_id))
        .unwrap();

    {
        let state = rest_state.clone();
        wait_until_async(
            move || {
                let state = state.clone();
                async move { !state.ticker_calls.lock().await.is_empty() }
            },
            Duration::from_secs(5),
        )
        .await;
    }

    let deadline = tokio::time::Instant::now() + Duration::from_secs(1);
    loop {
        match tokio::time::timeout_at(deadline, rx.recv()).await {
            Ok(Some(DataEvent::Response(DataResponse::Quotes(_)))) => {
                panic!("QuotesResponse must be dropped when get_ticker fails")
            }
            Ok(Some(_)) => {}
            Ok(None) | Err(_) => break,
        }
    }
}

// Fixture `http_ticker_eth_perp_snapshot.json` has `timestamp = 1_700_000_000_000`
// ms; offsets below are seconds-since-epoch and snapshot_secs is 1_700_000_000.
#[rstest]
#[case::unbounded(None, None, 1)]
#[case::straddle(Some(1_699_999_999), Some(1_700_000_001), 1)]
#[case::inclusive_lower(Some(1_700_000_000), None, 1)]
#[case::inclusive_upper(None, Some(1_700_000_000), 1)]
#[case::start_after(Some(1_700_000_001), None, 0)]
#[case::end_before(None, Some(1_699_999_999), 0)]
#[case::both_in_past(Some(1_699_000_000), Some(1_699_999_999), 0)]
#[case::both_in_future(Some(1_700_000_001), Some(1_700_001_000), 0)]
#[tokio::test]
async fn test_request_quotes_window_filter(
    #[case] start_secs: Option<i64>,
    #[case] end_secs: Option<i64>,
    #[case] expected_count: usize,
) {
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    *rest_state.ticker_response.lock().await = load_json("perps/http_ticker_eth_snapshot.json");
    let rest_addr = start_rest_server(rest_state.clone()).await;
    let ws_addr = start_ws_server(ws_state.clone()).await;
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    replace_data_event_sender(tx);

    let client = connect_with_eth_currency(rest_addr, ws_addr).await;
    let instrument_id = InstrumentId::from("ETH-PERP.DERIVE");

    client
        .request_quotes(request_quotes_window(
            instrument_id,
            start_secs.map(datetime_from_secs),
            end_secs.map(datetime_from_secs),
        ))
        .unwrap();

    let response = recv_response(&mut rx).await;
    let DataResponse::Quotes(quotes) = response else {
        panic!("expected quotes response");
    };
    assert_eq!(quotes.data.len(), expected_count);
}

#[rstest]
#[tokio::test]
async fn test_request_quotes_returns_err_for_missing_instrument() {
    let rest_state = RestState::default();
    let ws_state = WsState::default();
    let rest_addr = start_rest_server(rest_state).await;
    let ws_addr = start_ws_server(ws_state).await;
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    replace_data_event_sender(tx);

    let client = DeriveDataClient::new(*DERIVE_CLIENT_ID, config(rest_addr, ws_addr)).unwrap();

    let err = client
        .request_quotes(request_quotes(InstrumentId::from("ETH-PERP.DERIVE")))
        .expect_err("must reject missing instrument");
    assert!(err.to_string().contains("not found in cache"), "{err}");
}
