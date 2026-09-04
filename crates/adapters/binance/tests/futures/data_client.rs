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

//! Integration tests for the Binance Futures data client.

use std::{
    collections::HashMap,
    net::SocketAddr,
    num::NonZeroUsize,
    str::FromStr,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use axum::{
    Router,
    extract::{
        RawQuery, State,
        ws::{Message, WebSocket},
    },
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
};
use nautilus_binance::{
    common::{
        bar::BinanceBar,
        consts::{BINANCE_CLIENT_ID, BINANCE_VENUE},
        enums::BinanceProductType,
    },
    config::BinanceDataClientConfig,
    data_types::{
        BinanceFuturesLiquidation, BinanceFuturesMarkPriceUpdate, BinanceFuturesOpenInterest,
        BinanceFuturesOpenInterestHist, BinanceFuturesTicker,
    },
    futures::BinanceFuturesDataClient,
};
use nautilus_common::{
    clients::DataClient,
    live::runner::{set_data_event_sender, set_system_event_sender},
    messages::{
        DataEvent, SystemEvent,
        data::{
            DataResponse, RequestBars, RequestBookSnapshot, RequestCustomData, RequestFundingRates,
            RequestTrades,
            subscribe::{
                SubscribeBars, SubscribeBookDeltas, SubscribeCustomData, SubscribeMarkPrices,
                SubscribeQuotes, SubscribeTrades,
            },
            unsubscribe::{
                UnsubscribeBookDeltas, UnsubscribeCustomData, UnsubscribeQuotes, UnsubscribeTrades,
            },
        },
        system::SocketState,
    },
    testing::wait_until_async,
};
use nautilus_core::{Params, UUID4, UnixNanos};
use nautilus_live::{SocketReconnectRegistry, SocketReconnectRequestOutcome};
use nautilus_model::{
    data::{
        BarType, BookOrder, CustomData, Data, DataType, OrderBookDelta, OrderBookDeltas, QuoteTick,
    },
    enums::{BookAction, BookType, OrderSide, RecordFlag},
    identifiers::InstrumentId,
    instruments::InstrumentAny,
};
use nautilus_network::http::HttpClient;
use parking_lot::Mutex;
use rstest::rstest;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde_json::json;

type MarketQuery = (String, HashMap<String, String>);
type MarketQueries = Arc<Mutex<Vec<MarketQuery>>>;

#[derive(Clone)]
struct DataTestServerState {
    depth_failures_before_success: usize,
    depth_requests: Arc<AtomicUsize>,
    depth_response_delay: Duration,
    depth_snapshot_last_update_ids: Vec<u64>,
    depth_update_delay: Duration,
    depth_update_first_update_id: u64,
    depth_update_final_update_id: u64,
    depth_update_prev_final_update_id: u64,
    second_depth_update_prev_final_update_id: Option<u64>,
    depth_update_repetitions: usize,
    send_ticker_on_connect: bool,
    subscriptions: Arc<Mutex<Vec<Vec<String>>>>,
    unsubscriptions: Arc<Mutex<Vec<Vec<String>>>>,
    market_queries: MarketQueries,
}

impl Default for DataTestServerState {
    fn default() -> Self {
        Self {
            depth_failures_before_success: 0,
            depth_requests: Arc::new(AtomicUsize::new(0)),
            depth_response_delay: Duration::ZERO,
            depth_snapshot_last_update_ids: vec![1027024],
            depth_update_delay: Duration::from_millis(50),
            depth_update_first_update_id: 1027024,
            depth_update_final_update_id: 1027025,
            depth_update_prev_final_update_id: 1027023,
            second_depth_update_prev_final_update_id: None,
            depth_update_repetitions: 1,
            send_ticker_on_connect: false,
            subscriptions: Arc::new(Mutex::new(Vec::new())),
            unsubscriptions: Arc::new(Mutex::new(Vec::new())),
            market_queries: Arc::new(Mutex::new(Vec::new())),
        }
    }
}

fn liquidation_data_type_for_instrument(instrument_id: InstrumentId) -> DataType {
    let mut metadata = Params::new();
    metadata.insert(
        "instrument_id".to_string(),
        serde_json::Value::String(instrument_id.to_string()),
    );
    DataType::new(
        "BinanceFuturesLiquidation",
        Some(metadata),
        Some(instrument_id.to_string()),
    )
}

fn open_interest_data_type_for_instrument(instrument_id: InstrumentId) -> DataType {
    let mut metadata = Params::new();
    metadata.insert(
        "instrument_id".to_string(),
        serde_json::Value::String(instrument_id.to_string()),
    );
    DataType::new(
        "BinanceFuturesOpenInterest",
        Some(metadata),
        Some(instrument_id.to_string()),
    )
}

fn open_interest_hist_data_type_for_instrument(
    instrument_id: InstrumentId,
    period: &str,
) -> DataType {
    let mut metadata = Params::new();
    metadata.insert(
        "instrument_id".to_string(),
        serde_json::Value::String(instrument_id.to_string()),
    );
    metadata.insert(
        "period".to_string(),
        serde_json::Value::String(period.to_string()),
    );
    DataType::new(
        "BinanceFuturesOpenInterestHist",
        Some(metadata),
        Some(instrument_id.to_string()),
    )
}

fn binance_bar_data_type(bar_type: BarType) -> DataType {
    let mut metadata = Params::new();
    metadata.insert(
        "bar_type".to_string(),
        serde_json::Value::String(bar_type.to_string()),
    );
    DataType::new("BinanceBar", Some(metadata), Some(bar_type.to_string()))
}

fn json_response(body: &serde_json::Value) -> Response {
    (
        StatusCode::OK,
        [("content-type", "application/json")],
        body.to_string(),
    )
        .into_response()
}

fn load_fixture(name: &str) -> serde_json::Value {
    let path = format!(
        "{}/test_data/futures/http_json/{name}",
        env!("CARGO_MANIFEST_DIR")
    );
    let content = std::fs::read_to_string(&path).expect("Failed to read fixture");
    serde_json::from_str(&content).expect("Failed to parse fixture JSON")
}

async fn handle_ws(
    State(state): State<DataTestServerState>,
    ws: axum::extract::WebSocketUpgrade,
) -> Response {
    ws.on_upgrade(move |socket| handle_ws_connection(socket, state))
}

async fn handle_ws_connection(mut socket: WebSocket, state: DataTestServerState) {
    if state.send_ticker_on_connect {
        tokio::time::sleep(Duration::from_millis(200)).await;
        let ticker = ticker_stream_payload();
        let _result = socket.send(Message::Text(ticker.to_string().into())).await;
    }

    while let Some(Ok(msg)) = socket.recv().await {
        if let Message::Text(text) = msg
            && let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&text)
        {
            let method = parsed.get("method").and_then(|m| m.as_str());
            let id = parsed.get("id").and_then(|v| v.as_u64()).unwrap_or(1);

            if method == Some("SUBSCRIBE") {
                let resp = json!({"result": null, "id": id});
                let _result = socket.send(Message::Text(resp.to_string().into())).await;

                let streams = parsed
                    .get("params")
                    .and_then(|p| p.as_array())
                    .map(|params| {
                        params
                            .iter()
                            .filter_map(|param| param.as_str().map(str::to_string))
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();

                if !streams.is_empty() {
                    state.subscriptions.lock().push(streams.clone());
                }

                for stream in streams {
                    let symbol = stream
                        .split_once('@')
                        .map_or(stream.as_str(), |(symbol, _)| symbol)
                        .to_ascii_uppercase();

                    if stream.contains("@aggTrade") {
                        let trade = json!({
                            "e": "aggTrade",
                            "E": 1700000000000_i64,
                            "s": symbol,
                            "a": 1,
                            "p": "50000.00",
                            "q": "0.001",
                            "f": 1,
                            "l": 1,
                            "T": 1700000000000_i64,
                            "m": false
                        });
                        tokio::time::sleep(Duration::from_millis(50)).await;
                        let _result = socket.send(Message::Text(trade.to_string().into())).await;
                    } else if stream.contains("@bookTicker") {
                        let quote = json!({
                            "e": "bookTicker",
                            "u": 12345,
                            "E": 1700000000000_i64,
                            "T": 1700000000000_i64,
                            "s": symbol,
                            "b": "50000.00",
                            "B": "1.000",
                            "a": "50001.00",
                            "A": "0.500"
                        });
                        tokio::time::sleep(Duration::from_millis(50)).await;
                        let _result = socket.send(Message::Text(quote.to_string().into())).await;
                    } else if stream.contains("@ticker") {
                        let ticker = ticker_stream_payload();
                        tokio::time::sleep(Duration::from_millis(50)).await;
                        let _result = socket.send(Message::Text(ticker.to_string().into())).await;
                    } else if stream.contains("@kline_") {
                        let kline = json!({
                            "e": "kline",
                            "E": 1700000059999_i64,
                            "s": symbol,
                            "k": {
                                "t": 1700000000000_i64,
                                "T": 1700000059999_i64,
                                "s": "BTCUSDT",
                                "i": "1m",
                                "f": 201,
                                "L": 207,
                                "o": "50000.00",
                                "c": "50002.00",
                                "h": "50003.00",
                                "l": "49999.00",
                                "v": "7.500",
                                "n": 7,
                                "x": true,
                                "q": "375007.50",
                                "V": "3.250",
                                "Q": "162501.25",
                                "B": "0"
                            }
                        });
                        tokio::time::sleep(Duration::from_millis(50)).await;
                        let _result = socket.send(Message::Text(kline.to_string().into())).await;
                    } else if stream.contains("@depth") {
                        for index in 0..state.depth_update_repetitions {
                            let update_offset = index as u64;
                            let first_update_id = if index == 0 {
                                state.depth_update_first_update_id
                            } else {
                                state.depth_update_final_update_id + update_offset
                            };
                            let prev_final_update_id = if index == 0 {
                                state.depth_update_prev_final_update_id
                            } else if index == 1 {
                                state
                                    .second_depth_update_prev_final_update_id
                                    .unwrap_or(state.depth_update_final_update_id)
                            } else {
                                state.depth_update_final_update_id + update_offset - 1
                            };
                            let depth_update = json!({
                                "e": "depthUpdate",
                                "E": 1700000000000_i64,
                                "T": 1700000000000_i64,
                                "s": "BTCUSDT",
                                "U": first_update_id,
                                "u": state.depth_update_final_update_id + update_offset,
                                "pu": prev_final_update_id,
                                "b": [["50000.00", "1.000"], ["49999.00", "2.000"]],
                                "a": [["50001.00", "0.500"], ["50002.00", "1.500"]]
                            });
                            tokio::time::sleep(state.depth_update_delay).await;
                            let _result = socket
                                .send(Message::Text(depth_update.to_string().into()))
                                .await;
                        }
                    } else if stream.contains("@markPrice") {
                        let mark_price = json!({
                            "e": "markPriceUpdate",
                            "E": 1700000000000_i64,
                            "s": "BTCUSDT",
                            "p": "50000.50",
                            "i": "50000.25",
                            "P": "50000.75",
                            "r": "0.00010000",
                            "T": 1700028800000_i64
                        });
                        tokio::time::sleep(Duration::from_millis(50)).await;
                        let _result = socket
                            .send(Message::Text(mark_price.to_string().into()))
                            .await;
                    } else if stream.contains("@forceOrder") || stream.contains("!forceOrder@arr") {
                        let last_filled_qty = if stream.contains("!forceOrder@arr") {
                            "0.002"
                        } else {
                            "0.001"
                        };
                        let liquidation = json!({
                            "e": "forceOrder",
                            "E": 1700000000000_i64,
                            "o": {
                                "s": "BTCUSDT",
                                "S": "SELL",
                                "o": "LIMIT",
                                "f": "IOC",
                                "q": "0.003",
                                "p": "50000.10",
                                "ap": "50000.20",
                                "X": "FILLED",
                                "l": last_filled_qty,
                                "z": "0.003",
                                "T": 1700000000000_i64
                            }
                        });
                        tokio::time::sleep(Duration::from_millis(50)).await;
                        let _result = socket
                            .send(Message::Text(liquidation.to_string().into()))
                            .await;
                    }
                }
            } else if method == Some("UNSUBSCRIBE") {
                let resp = json!({"result": null, "id": id});
                let _result = socket.send(Message::Text(resp.to_string().into())).await;

                let streams = parsed
                    .get("params")
                    .and_then(|p| p.as_array())
                    .map(|params| {
                        params
                            .iter()
                            .filter_map(|param| param.as_str().map(str::to_string))
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();

                if !streams.is_empty() {
                    state.unsubscriptions.lock().push(streams);
                }
            }
        }
    }
}

async fn handle_depth(State(state): State<DataTestServerState>) -> Response {
    let request = state.depth_requests.fetch_add(1, Ordering::Relaxed);
    if request < state.depth_failures_before_success {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            [("content-type", "application/json")],
            json!({"code": -1000, "msg": "Transient depth failure"}).to_string(),
        )
            .into_response();
    }

    let successful_request = request - state.depth_failures_before_success;
    let last_update_id = state
        .depth_snapshot_last_update_ids
        .get(successful_request)
        .copied()
        .or_else(|| state.depth_snapshot_last_update_ids.last().copied())
        .unwrap_or(1027024);

    tokio::time::sleep(state.depth_response_delay).await;

    json_response(&json!({
        "lastUpdateId": last_update_id,
        "E": 1700000000000_i64,
        "T": 1700000000000_i64,
        "bids": [["50000.00", "1.000"], ["49999.00", "2.000"]],
        "asks": [["50001.00", "0.500"], ["50002.00", "1.500"]]
    }))
}

async fn handle_open_interest() -> Response {
    json_response(&json!({
        "symbol": "BTCUSDT",
        "openInterest": "12345.678",
        "time": 1700000000000_i64
    }))
}

async fn handle_open_interest_coinm() -> Response {
    json_response(&json!({
        "symbol": "BTCUSD_PERP",
        "openInterest": "987.654",
        "time": 1700000005000_i64
    }))
}

async fn handle_open_interest_hist(raw_query: RawQuery) -> Response {
    let query = raw_query.0.unwrap_or_default();
    let params: HashMap<String, String> = serde_urlencoded::from_str(&query).unwrap_or_default();

    if params
        .get("symbol")
        .is_some_and(|symbol| symbol == "BTCUSDT")
        && params.get("period").is_some_and(|period| period == "5m")
    {
        return json_response(&json!([
            {
                "symbol": "BTCUSDT",
                "sumOpenInterest": "100.0",
                "sumOpenInterestValue": "1000.0",
                "timestamp": 1700000000000_i64,
                "CMCCirculatingSupply": "123"
            },
            {
                "symbol": "BTCUSDT",
                "sumOpenInterest": "101.0",
                "sumOpenInterestValue": "1005.0",
                "timestamp": 1700000300000_i64,
                "CMCCirculatingSupply": "123"
            }
        ]));
    }

    if params.get("pair").is_some_and(|pair| pair == "BTCUSD")
        && params
            .get("contractType")
            .is_some_and(|contract_type| contract_type == "PERPETUAL")
        && params.get("period").is_some_and(|period| period == "5m")
    {
        return json_response(&json!([
            {
                "pair": "BTCUSD",
                "contractType": "PERPETUAL",
                "sumOpenInterest": "200.0",
                "sumOpenInterestValue": "1500.0",
                "timestamp": 1700000600000_i64
            },
            {
                "pair": "BTCUSD",
                "contractType": "PERPETUAL",
                "sumOpenInterest": "201.0",
                "sumOpenInterestValue": "1510.0",
                "timestamp": 1700000900000_i64
            }
        ]));
    }

    if params.get("pair").is_some_and(|pair| pair == "BTCUSD")
        && params
            .get("contractType")
            .is_some_and(|contract_type| contract_type == "CURRENT_QUARTER")
        && params.get("period").is_some_and(|period| period == "5m")
    {
        return json_response(&json!([
            {
                "pair": "BTCUSD",
                "contractType": "CURRENT_QUARTER",
                "sumOpenInterest": "300.0",
                "sumOpenInterestValue": "2500.0",
                "timestamp": 1700001200000_i64
            }
        ]));
    }

    (
        StatusCode::BAD_REQUEST,
        [("content-type", "application/json")],
        json!({"code": -1102, "msg": "Unexpected open interest history params"}).to_string(),
    )
        .into_response()
}

async fn handle_funding_rate(raw_query: RawQuery) -> Response {
    let query = raw_query.0.unwrap_or_default();
    let params: HashMap<String, String> = serde_urlencoded::from_str(&query).unwrap_or_default();

    if params
        .get("symbol")
        .is_some_and(|symbol| symbol == "BTCUSDT")
        && params.get("limit").is_some_and(|limit| limit == "2")
    {
        return json_response(&json!([
            {
                "symbol": "BTCUSDT",
                "fundingRate": "0.00010000",
                "fundingTime": 1700000000000_i64,
                "markPrice": "50000.00"
            },
            {
                "symbol": "BTCUSDT",
                "fundingRate": "-0.00007500",
                "fundingTime": 1700028800000_i64,
                "markPrice": "50100.00"
            }
        ]));
    }

    (
        StatusCode::BAD_REQUEST,
        [("content-type", "application/json")],
        json!({"code": -1102, "msg": "Unexpected funding rate params"}).to_string(),
    )
        .into_response()
}

fn futures_agg_trades_response(
    state: &DataTestServerState,
    raw_query: &RawQuery,
    path: &str,
) -> Response {
    let query = serde_urlencoded::from_str::<HashMap<String, String>>(
        raw_query.0.as_deref().unwrap_or_default(),
    )
    .unwrap();
    let time = query
        .get("startTime")
        .or_else(|| query.get("endTime"))
        .and_then(|value| value.parse().ok())
        .unwrap_or_else(|| jiff::Timestamp::now().as_millisecond());
    state.market_queries.lock().push((path.to_string(), query));
    let quantity = if path.starts_with("/dapi") {
        "2"
    } else {
        "0.123"
    };
    json_response(&json!([{
        "a": 101,
        "p": "50001.23",
        "q": quantity,
        "f": 201,
        "l": 207,
        "T": time,
        "m": false
    }]))
}

async fn handle_usdm_agg_trades(
    State(state): State<DataTestServerState>,
    raw_query: RawQuery,
) -> Response {
    futures_agg_trades_response(&state, &raw_query, "/fapi/v1/aggTrades")
}

async fn handle_coinm_agg_trades(
    State(state): State<DataTestServerState>,
    raw_query: RawQuery,
) -> Response {
    futures_agg_trades_response(&state, &raw_query, "/dapi/v1/aggTrades")
}

fn futures_klines_response(
    state: &DataTestServerState,
    raw_query: &RawQuery,
    path: &str,
) -> Response {
    let query = serde_urlencoded::from_str::<HashMap<String, String>>(
        raw_query.0.as_deref().unwrap_or_default(),
    )
    .unwrap();
    let close_time = query
        .get("endTime")
        .and_then(|value| value.parse::<i64>().ok())
        .unwrap_or_else(|| jiff::Timestamp::now().as_millisecond() - 1_000);
    state.market_queries.lock().push((path.to_string(), query));
    json_response(&json!([[
        close_time - 59_999,
        "50000.00",
        "50003.00",
        "49999.00",
        "50002.00",
        "8",
        close_time,
        "375007.50",
        7,
        "3",
        "162501.25",
        "0"
    ]]))
}

async fn handle_usdm_klines(
    State(state): State<DataTestServerState>,
    raw_query: RawQuery,
) -> Response {
    futures_klines_response(&state, &raw_query, "/fapi/v1/klines")
}

async fn handle_coinm_klines(
    State(state): State<DataTestServerState>,
    raw_query: RawQuery,
) -> Response {
    futures_klines_response(&state, &raw_query, "/dapi/v1/klines")
}

fn create_data_test_router(state: DataTestServerState) -> Router {
    Router::new()
        .route("/fapi/v1/ping", get(|| async { json_response(&json!({})) }))
        .route("/dapi/v1/ping", get(|| async { json_response(&json!({})) }))
        .route(
            "/fapi/v1/exchangeInfo",
            get(|| async { json_response(&load_fixture("exchange_info_usdm.json")) }),
        )
        .route(
            "/dapi/v1/exchangeInfo",
            get(|| async { json_response(&load_fixture("exchange_info_delivery_coinm.json")) }),
        )
        .route("/fapi/v1/depth", get(handle_depth))
        .route("/fapi/v1/aggTrades", get(handle_usdm_agg_trades))
        .route("/dapi/v1/aggTrades", get(handle_coinm_agg_trades))
        .route("/fapi/v1/klines", get(handle_usdm_klines))
        .route("/dapi/v1/klines", get(handle_coinm_klines))
        .route("/fapi/v1/openInterest", get(handle_open_interest))
        .route("/dapi/v1/openInterest", get(handle_open_interest_coinm))
        .route(
            "/futures/data/openInterestHist",
            get(handle_open_interest_hist),
        )
        .route("/fapi/v1/fundingRate", get(handle_funding_rate))
        .route("/ws", get(handle_ws))
        .with_state(state)
}

async fn start_data_test_server() -> SocketAddr {
    start_data_test_server_with_state(DataTestServerState::default()).await
}

async fn start_data_test_server_with_state(state: DataTestServerState) -> SocketAddr {
    let router = create_data_test_router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        axum::serve(listener, router.into_make_service())
            .await
            .unwrap();
    });

    let health_url = format!("http://{addr}/fapi/v1/ping");
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

    addr
}

async fn recv_data(
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<DataEvent>,
    timeout: Duration,
) -> Option<Data> {
    let deadline = tokio::time::Instant::now() + timeout;

    loop {
        let remaining = deadline.checked_duration_since(tokio::time::Instant::now())?;
        match tokio::time::timeout(remaining, rx.recv()).await {
            Ok(Some(DataEvent::Data(data))) => return Some(data),
            Ok(Some(_)) => {}
            Ok(None) | Err(_) => return None,
        }
    }
}

fn create_test_data_client(
    base_url_http: String,
    base_url_ws: String,
) -> (
    BinanceFuturesDataClient,
    tokio::sync::mpsc::UnboundedReceiver<DataEvent>,
) {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    set_data_event_sender(tx);

    let config = BinanceDataClientConfig {
        product_type: BinanceProductType::UsdM,
        base_url_http: Some(base_url_http),
        base_url_ws: Some(base_url_ws),
        ..Default::default()
    };

    let client =
        BinanceFuturesDataClient::new(*BINANCE_CLIENT_ID, config, BinanceProductType::UsdM)
            .unwrap();

    (client, rx)
}

fn create_test_data_client_for_product_type(
    base_url_http: String,
    base_url_ws: String,
    product_type: BinanceProductType,
) -> (
    BinanceFuturesDataClient,
    tokio::sync::mpsc::UnboundedReceiver<DataEvent>,
) {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    set_data_event_sender(tx);

    let config = BinanceDataClientConfig {
        product_type,
        base_url_http: Some(base_url_http),
        base_url_ws: Some(base_url_ws),
        ..Default::default()
    };

    let client = BinanceFuturesDataClient::new(*BINANCE_CLIENT_ID, config, product_type).unwrap();

    (client, rx)
}

#[rstest]
#[tokio::test]
async fn test_client_creation() {
    let addr = start_data_test_server().await;
    let base_url_http = format!("http://{addr}");
    let base_url_ws = format!("ws://{addr}/ws");

    let (client, _rx) = create_test_data_client(base_url_http, base_url_ws);

    assert_eq!(client.client_id(), *BINANCE_CLIENT_ID);
    assert_eq!(client.venue(), Some(*BINANCE_VENUE));
    assert!(!client.is_connected());
}

#[derive(Clone, Copy)]
enum AggregateTradeBounds {
    Start,
    End,
    Both,
}

#[rstest]
#[case::usdm(
    BinanceProductType::UsdM,
    "BTCUSDT_260925.BINANCE",
    "btcusdt_260925@bookTicker"
)]
#[case::coinm(
    BinanceProductType::CoinM,
    "BTCUSD_260925.BINANCE",
    "btcusd_260925@bookTicker"
)]
#[tokio::test]
async fn test_delivery_instrument_connects_and_subscribes_with_raw_symbol(
    #[case] product_type: BinanceProductType,
    #[case] expected_id: &str,
    #[case] expected_stream: &str,
) {
    let state = DataTestServerState::default();
    let addr = start_data_test_server_with_state(state.clone()).await;
    let base_url_http = format!("http://{addr}");
    let base_url_ws = format!("ws://{addr}/ws");
    let (mut client, mut rx) =
        create_test_data_client_for_product_type(base_url_http, base_url_ws, product_type);
    let instrument_id = InstrumentId::from(expected_id);

    client.connect().await.unwrap();

    wait_until_async(
        || {
            let found = rx.try_recv().is_ok_and(|event| {
                matches!(
                    event,
                    DataEvent::Instrument(InstrumentAny::CryptoFuture(future))
                        if future.id == instrument_id
                )
            });
            async move { found }
        },
        Duration::from_secs(5),
    )
    .await;

    client
        .subscribe_quotes(SubscribeQuotes::new(
            instrument_id,
            Some(*BINANCE_CLIENT_ID),
            None,
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ))
        .unwrap();

    wait_until_async(
        || {
            let found = recorded_streams_include(&state.subscriptions, expected_stream);
            async move { found }
        },
        Duration::from_secs(5),
    )
    .await;

    wait_until_async(
        || {
            let found = rx.try_recv().is_ok_and(|event| {
                matches!(event, DataEvent::Data(Data::Quote(quote)) if quote.instrument_id == instrument_id)
            });
            async move { found }
        },
        Duration::from_secs(5),
    )
    .await;
}

#[rstest]
#[tokio::test]
async fn test_connect_emits_instruments() {
    let addr = start_data_test_server().await;
    let base_url_http = format!("http://{addr}");
    let base_url_ws = format!("ws://{addr}/ws");

    let (mut client, mut rx) = create_test_data_client(base_url_http, base_url_ws);

    client.connect().await.unwrap();
    assert!(client.is_connected());

    wait_until_async(
        || {
            let found = rx
                .try_recv()
                .is_ok_and(|e| matches!(e, DataEvent::Instrument(_)));
            async move { found }
        },
        Duration::from_secs(5),
    )
    .await;
}

#[rstest]
#[tokio::test]
async fn test_connect_emits_socket_state_changes() {
    let addr = start_data_test_server().await;
    let base_url_http = format!("http://{addr}");
    let base_url_ws = format!("ws://{addr}/ws");
    let (system_tx, mut system_rx) = tokio::sync::mpsc::unbounded_channel();
    set_system_event_sender(system_tx);
    let registry = SocketReconnectRegistry::default();
    let (mut client, _data_rx) =
        registry.scope(|| create_test_data_client(base_url_http, base_url_ws));

    client.connect().await.unwrap();

    let mut changes = Vec::new();
    wait_until_async(
        || {
            while let Ok(event) = system_rx.try_recv() {
                let SystemEvent::SocketState(change) = event;
                changes.push(change);
            }
            let done = changes.len() == 2;
            async move { done }
        },
        Duration::from_secs(5),
    )
    .await;

    assert_eq!(changes.len(), 2);
    assert_eq!(changes[0].client_id, *BINANCE_CLIENT_ID);
    assert_eq!(changes[0].venue, Some(*BINANCE_VENUE));
    assert_eq!(
        changes[0].endpoint,
        ustr::Ustr::from("binance-futures-market-streams")
    );
    assert_eq!(changes[0].state, SocketState::Connected);
    assert_eq!(changes[1].client_id, *BINANCE_CLIENT_ID);
    assert_eq!(changes[1].venue, Some(*BINANCE_VENUE));
    assert_eq!(
        changes[1].endpoint,
        ustr::Ustr::from("binance-futures-public-streams")
    );
    assert_eq!(changes[1].state, SocketState::Connected);

    let market_endpoint = ustr::Ustr::from("binance-futures-market-streams");
    let public_endpoint = ustr::Ustr::from("binance-futures-public-streams");
    let market_handle = registry
        .handle(*BINANCE_CLIENT_ID, market_endpoint)
        .unwrap();
    let public_handle = registry
        .handle(*BINANCE_CLIENT_ID, public_endpoint)
        .unwrap();
    assert_eq!(
        market_handle.request_reconnect(),
        SocketReconnectRequestOutcome::Accepted
    );
    assert_eq!(
        public_handle.request_reconnect(),
        SocketReconnectRequestOutcome::Accepted
    );

    let mut disconnected = std::collections::HashSet::new();
    wait_until_async(
        || {
            while let Ok(event) = system_rx.try_recv() {
                let SystemEvent::SocketState(change) = event;
                if change.state == SocketState::Disconnected {
                    disconnected.insert(change.endpoint);
                }
            }
            let done =
                disconnected.contains(&market_endpoint) && disconnected.contains(&public_endpoint);
            async move { done }
        },
        Duration::from_secs(5),
    )
    .await;

    client.disconnect().await.unwrap();
    assert!(
        registry
            .handle(*BINANCE_CLIENT_ID, market_endpoint)
            .is_none()
    );
    assert!(
        registry
            .handle(*BINANCE_CLIENT_ID, public_endpoint)
            .is_none()
    );
}

#[rstest]
#[tokio::test]
async fn test_disconnect_sets_state() {
    let addr = start_data_test_server().await;
    let base_url_http = format!("http://{addr}");
    let base_url_ws = format!("ws://{addr}/ws");

    let (mut client, _rx) = create_test_data_client(base_url_http, base_url_ws);

    client.connect().await.unwrap();
    assert!(client.is_connected());

    client.disconnect().await.unwrap();
    assert!(!client.is_connected());
}

#[rstest]
#[tokio::test]
async fn test_request_open_interest_usdm_emits_custom_data_response() {
    let addr = start_data_test_server().await;
    let base_url_http = format!("http://{addr}");
    let base_url_ws = format!("ws://{addr}/ws");

    let (client, mut rx) = create_test_data_client(base_url_http, base_url_ws);
    let instrument_id = InstrumentId::from("BTCUSDT-PERP.BINANCE");
    let data_type = open_interest_data_type_for_instrument(instrument_id);

    client
        .request_data(RequestCustomData::new(
            *BINANCE_CLIENT_ID,
            data_type.clone(),
            None,
            None,
            None,
            UUID4::new(),
            UnixNanos::default(),
            None,
        ))
        .unwrap();

    wait_until_async(
        || {
            let found = rx.try_recv().is_ok_and(|event| {
                let DataEvent::Response(DataResponse::Data(resp)) = event else {
                    return false;
                };
                let Some(custom) = resp.data.as_ref().downcast_ref::<CustomData>() else {
                    return false;
                };

                custom
                    .data
                    .as_any()
                    .downcast_ref::<BinanceFuturesOpenInterest>()
                    .is_some_and(|payload| {
                        payload.instrument_id == instrument_id
                            && payload.open_interest == Decimal::from_str("12345.678").unwrap()
                            && payload.ts_event.as_u64()
                                == UnixNanos::from_millis(1700000000000).as_u64()
                            && custom.data_type == data_type
                    })
            });
            async move { found }
        },
        Duration::from_secs(5),
    )
    .await;
}

#[rstest]
#[tokio::test]
async fn test_request_open_interest_hist_usdm_emits_batch_custom_data_response() {
    let addr = start_data_test_server().await;
    let base_url_http = format!("http://{addr}");
    let base_url_ws = format!("ws://{addr}/ws");

    let (client, mut rx) = create_test_data_client(base_url_http, base_url_ws);
    let instrument_id = InstrumentId::from("BTCUSDT-PERP.BINANCE");
    let data_type = open_interest_hist_data_type_for_instrument(instrument_id, "5m");

    client
        .request_data(RequestCustomData::new(
            *BINANCE_CLIENT_ID,
            data_type.clone(),
            None,
            None,
            Some(NonZeroUsize::new(2).unwrap()),
            UUID4::new(),
            UnixNanos::default(),
            None,
        ))
        .unwrap();

    wait_until_async(
        || {
            let found = rx.try_recv().is_ok_and(|event| {
                let DataEvent::Response(DataResponse::Data(resp)) = event else {
                    return false;
                };
                let Some(custom) = resp.data.as_ref().downcast_ref::<CustomData>() else {
                    return false;
                };
                let Some(payload) = custom
                    .data
                    .as_any()
                    .downcast_ref::<BinanceFuturesOpenInterestHist>()
                else {
                    return false;
                };

                payload.instrument_id == instrument_id
                    && payload.period == "5m"
                    && payload.points.len() == 2
                    && payload.points[0].sum_open_interest == Decimal::from_str("100.0").unwrap()
                    && payload.points[1].sum_open_interest_value
                        == Decimal::from_str("1005.0").unwrap()
                    && payload.ts_event.as_u64() == UnixNanos::from_millis(1700000300000).as_u64()
                    && custom.data_type == data_type
            });
            async move { found }
        },
        Duration::from_secs(5),
    )
    .await;
}

#[rstest]
#[tokio::test]
async fn test_request_open_interest_coinm_uses_symbol_mapping() {
    let addr = start_data_test_server().await;
    let base_url_http = format!("http://{addr}");
    let base_url_ws = format!("ws://{addr}/ws");

    let (client, mut rx) = create_test_data_client_for_product_type(
        base_url_http,
        base_url_ws,
        BinanceProductType::CoinM,
    );
    let instrument_id = InstrumentId::from("BTCUSD_PERP.BINANCE");
    let data_type = open_interest_data_type_for_instrument(instrument_id);

    client
        .request_data(RequestCustomData::new(
            *BINANCE_CLIENT_ID,
            data_type.clone(),
            None,
            None,
            None,
            UUID4::new(),
            UnixNanos::default(),
            None,
        ))
        .unwrap();

    wait_until_async(
        || {
            let found = rx.try_recv().is_ok_and(|event| {
                let DataEvent::Response(DataResponse::Data(resp)) = event else {
                    return false;
                };
                let Some(custom) = resp.data.as_ref().downcast_ref::<CustomData>() else {
                    return false;
                };

                custom
                    .data
                    .as_any()
                    .downcast_ref::<BinanceFuturesOpenInterest>()
                    .is_some_and(|payload| {
                        payload.instrument_id == instrument_id
                            && payload.open_interest == Decimal::from_str("987.654").unwrap()
                            && custom.data_type == data_type
                    })
            });
            async move { found }
        },
        Duration::from_secs(5),
    )
    .await;
}

#[rstest]
#[tokio::test]
async fn test_request_open_interest_hist_coinm_uses_pair_and_contract_type_mapping() {
    let addr = start_data_test_server().await;
    let base_url_http = format!("http://{addr}");
    let base_url_ws = format!("ws://{addr}/ws");

    let (client, mut rx) = create_test_data_client_for_product_type(
        base_url_http,
        base_url_ws,
        BinanceProductType::CoinM,
    );
    let instrument_id = InstrumentId::from("BTCUSD_PERP.BINANCE");
    let data_type = open_interest_hist_data_type_for_instrument(instrument_id, "5m");

    client
        .request_data(RequestCustomData::new(
            *BINANCE_CLIENT_ID,
            data_type.clone(),
            None,
            None,
            Some(NonZeroUsize::new(2).unwrap()),
            UUID4::new(),
            UnixNanos::default(),
            None,
        ))
        .unwrap();

    wait_until_async(
        || {
            let found = rx.try_recv().is_ok_and(|event| {
                let DataEvent::Response(DataResponse::Data(resp)) = event else {
                    return false;
                };
                let Some(custom) = resp.data.as_ref().downcast_ref::<CustomData>() else {
                    return false;
                };
                let Some(payload) = custom
                    .data
                    .as_any()
                    .downcast_ref::<BinanceFuturesOpenInterestHist>()
                else {
                    return false;
                };

                payload.instrument_id == instrument_id
                    && payload.period == "5m"
                    && payload.points.len() == 2
                    && payload.points[0].sum_open_interest == Decimal::from_str("200.0").unwrap()
                    && payload.points[1].sum_open_interest_value
                        == Decimal::from_str("1510.0").unwrap()
                    && custom.data_type == data_type
            });
            async move { found }
        },
        Duration::from_secs(5),
    )
    .await;
}

#[rstest]
#[tokio::test]
async fn test_request_open_interest_hist_coinm_delivery_uses_exchange_contract_type() {
    let addr = start_data_test_server().await;
    let base_url_http = format!("http://{addr}");
    let base_url_ws = format!("ws://{addr}/ws");
    let (mut client, mut rx) = create_test_data_client_for_product_type(
        base_url_http,
        base_url_ws,
        BinanceProductType::CoinM,
    );
    let instrument_id = InstrumentId::from("BTCUSD_260925.BINANCE");
    let data_type = open_interest_hist_data_type_for_instrument(instrument_id, "5m");

    client.connect().await.unwrap();

    while rx.try_recv().is_ok() {}

    client
        .request_data(RequestCustomData::new(
            *BINANCE_CLIENT_ID,
            data_type.clone(),
            None,
            None,
            Some(NonZeroUsize::new(1).unwrap()),
            UUID4::new(),
            UnixNanos::default(),
            None,
        ))
        .unwrap();

    wait_until_async(
        || {
            let found = rx.try_recv().is_ok_and(|event| {
                let DataEvent::Response(DataResponse::Data(resp)) = event else {
                    return false;
                };
                let Some(custom) = resp.data.as_ref().downcast_ref::<CustomData>() else {
                    return false;
                };

                custom
                    .data
                    .as_any()
                    .downcast_ref::<BinanceFuturesOpenInterestHist>()
                    .is_some_and(|payload| {
                        payload.instrument_id == instrument_id
                            && payload.points.len() == 1
                            && payload.points[0].sum_open_interest == Decimal::from(300)
                            && custom.data_type == data_type
                    })
            });
            async move { found }
        },
        Duration::from_secs(5),
    )
    .await;
}

#[rstest]
#[tokio::test]
async fn test_request_open_interest_requires_instrument_id_metadata() {
    let addr = start_data_test_server().await;
    let base_url_http = format!("http://{addr}");
    let base_url_ws = format!("ws://{addr}/ws");

    let (client, _rx) = create_test_data_client(base_url_http, base_url_ws);
    let data_type = DataType::new("BinanceFuturesOpenInterest", None, None);

    let result = client.request_data(RequestCustomData::new(
        *BINANCE_CLIENT_ID,
        data_type,
        None,
        None,
        None,
        UUID4::new(),
        UnixNanos::default(),
        None,
    ));

    assert!(result.is_err());
}

#[rstest]
#[tokio::test]
async fn test_request_unsupported_custom_data_returns_ok() {
    let addr = start_data_test_server().await;
    let base_url_http = format!("http://{addr}");
    let base_url_ws = format!("ws://{addr}/ws");

    let (client, _rx) = create_test_data_client(base_url_http, base_url_ws);
    let data_type = DataType::new("UnsupportedBinanceCustomData", None, None);

    let result = client.request_data(RequestCustomData::new(
        *BINANCE_CLIENT_ID,
        data_type,
        None,
        None,
        None,
        UUID4::new(),
        UnixNanos::default(),
        None,
    ));

    assert!(result.is_ok());
}

#[rstest]
#[tokio::test]
async fn test_request_funding_rates_emits_response() {
    let addr = start_data_test_server().await;
    let base_url_http = format!("http://{addr}");
    let base_url_ws = format!("ws://{addr}/ws");

    let (client, mut rx) = create_test_data_client(base_url_http, base_url_ws);
    let instrument_id = InstrumentId::from("BTCUSDT-PERP.BINANCE");

    client
        .request_funding_rates(RequestFundingRates::new(
            instrument_id,
            None,
            None,
            Some(NonZeroUsize::new(2).unwrap()),
            None,
            UUID4::new(),
            UnixNanos::default(),
            None,
        ))
        .unwrap();

    wait_until_async(
        || {
            let found = rx.try_recv().is_ok_and(|event| {
                let DataEvent::Response(DataResponse::FundingRates(resp)) = event else {
                    return false;
                };

                resp.instrument_id == instrument_id
                    && resp.data.len() == 2
                    && resp.data[0].rate == dec!(0.0001)
                    && resp.data[0].ts_event == UnixNanos::from_millis(1700000000000)
                    && resp.data[1].rate == dec!(-0.000075)
                    && resp.data[1].ts_event == UnixNanos::from_millis(1700028800000)
            });
            async move { found }
        },
        Duration::from_secs(5),
    )
    .await;
}

#[rstest]
#[case::usdm(
    BinanceProductType::UsdM,
    "BTCUSDT_260925.BINANCE",
    "/fapi/v1/aggTrades",
    "BTCUSDT_260925",
    dec!(50001.2),
    dec!(0.123),
    AggregateTradeBounds::Both
)]
#[case::coinm(
    BinanceProductType::CoinM,
    "BTCUSD_260925.BINANCE",
    "/dapi/v1/aggTrades",
    "BTCUSD_260925",
    dec!(50001.2),
    dec!(2),
    AggregateTradeBounds::Both
)]
#[case::start_only(
    BinanceProductType::UsdM,
    "BTCUSDT_260925.BINANCE",
    "/fapi/v1/aggTrades",
    "BTCUSDT_260925",
    dec!(50001.2),
    dec!(0.123),
    AggregateTradeBounds::Start
)]
#[case::end_only(
    BinanceProductType::CoinM,
    "BTCUSD_260925.BINANCE",
    "/dapi/v1/aggTrades",
    "BTCUSD_260925",
    dec!(50001.2),
    dec!(2),
    AggregateTradeBounds::End
)]
#[tokio::test]
async fn test_request_bounded_aggregate_trades_routes_futures_product(
    #[case] product_type: BinanceProductType,
    #[case] instrument: &str,
    #[case] expected_path: &str,
    #[case] expected_symbol: &str,
    #[case] expected_price: Decimal,
    #[case] expected_size: Decimal,
    #[case] bounds: AggregateTradeBounds,
) {
    let state = DataTestServerState::default();
    let addr = start_data_test_server_with_state(state.clone()).await;
    let (mut client, mut rx) = create_test_data_client_for_product_type(
        format!("http://{addr}"),
        format!("ws://{addr}/ws"),
        product_type,
    );
    client.connect().await.unwrap();

    while rx.try_recv().is_ok() {}
    let instrument_id = InstrumentId::from(instrument);
    let start = jiff::Timestamp::now() - jiff::SignedDuration::from_mins(10);
    let end = start + jiff::SignedDuration::from_mins(5);
    let (request_start, request_end) = match bounds {
        AggregateTradeBounds::Start => (Some(start), None),
        AggregateTradeBounds::End => (None, Some(end)),
        AggregateTradeBounds::Both => (Some(start), Some(end)),
    };
    let expected_start = request_start.map(|value| value.as_millisecond().to_string());
    let expected_end = request_end.map(|value| value.as_millisecond().to_string());
    let expected_event_time = request_start
        .as_ref()
        .or(request_end.as_ref())
        .unwrap()
        .as_millisecond();

    client
        .request_trades(RequestTrades::new(
            instrument_id,
            request_start,
            request_end,
            Some(NonZeroUsize::new(456).unwrap()),
            Some(*BINANCE_CLIENT_ID),
            UUID4::new(),
            UnixNanos::default(),
            None,
        ))
        .unwrap();

    let event = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("timeout waiting for aggregate trades")
        .expect("data channel closed");
    let DataEvent::Response(DataResponse::Trades(response)) = event else {
        panic!("expected trades response");
    };
    let queries = state.market_queries.lock();
    let (path, query) = &queries[0];
    assert_eq!(path, expected_path);
    assert_eq!(
        query.get("symbol").map(String::as_str),
        Some(expected_symbol)
    );
    assert_eq!(
        query.get("startTime").map(String::as_str),
        expected_start.as_deref()
    );
    assert_eq!(
        query.get("endTime").map(String::as_str),
        expected_end.as_deref()
    );
    assert_eq!(query.get("limit").map(String::as_str), Some("456"));
    assert_eq!(response.instrument_id, instrument_id);
    assert_eq!(response.data.len(), 1);
    assert_eq!(response.data[0].price.as_decimal(), expected_price);
    assert_eq!(response.data[0].size.as_decimal(), expected_size);
    assert_eq!(response.data[0].trade_id.to_string(), "101");
    assert_eq!(
        response.data[0].ts_event,
        UnixNanos::from_millis(expected_event_time as u64)
    );
    assert_eq!(response.data[0].ts_init, response.data[0].ts_event);
}

#[rstest]
#[case::usdm(
    BinanceProductType::UsdM,
    "BTCUSDT_260925.BINANCE-1-MINUTE-LAST-EXTERNAL",
    "/fapi/v1/klines",
    "BTCUSDT_260925"
)]
#[case::coinm(
    BinanceProductType::CoinM,
    "BTCUSD_260925.BINANCE-1-MINUTE-LAST-EXTERNAL",
    "/dapi/v1/klines",
    "BTCUSD_260925"
)]
#[tokio::test]
async fn test_request_historical_binance_bars_routes_futures_product(
    #[case] product_type: BinanceProductType,
    #[case] bar_type_raw: &str,
    #[case] expected_path: &str,
    #[case] expected_symbol: &str,
) {
    let state = DataTestServerState::default();
    let addr = start_data_test_server_with_state(state.clone()).await;
    let (mut client, mut rx) = create_test_data_client_for_product_type(
        format!("http://{addr}"),
        format!("ws://{addr}/ws"),
        product_type,
    );
    client.connect().await.unwrap();

    while rx.try_recv().is_ok() {}
    let bar_type = BarType::from(bar_type_raw);
    let data_type = binance_bar_data_type(bar_type);
    let start = jiff::Timestamp::from_millisecond(1_700_000_000_000).unwrap();
    let end = jiff::Timestamp::from_millisecond(1_700_000_059_999).unwrap();

    client
        .request_data(RequestCustomData::new(
            *BINANCE_CLIENT_ID,
            data_type.clone(),
            Some(start),
            Some(end),
            Some(NonZeroUsize::new(321).unwrap()),
            UUID4::new(),
            UnixNanos::default(),
            None,
        ))
        .unwrap();

    let event = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("timeout waiting for BinanceBar history")
        .expect("data channel closed");
    let DataEvent::Response(DataResponse::Data(response)) = event else {
        panic!("expected custom data response");
    };
    let bars = response
        .data
        .as_ref()
        .downcast_ref::<Vec<BinanceBar>>()
        .expect("expected BinanceBar vector");
    let queries = state.market_queries.lock();
    let (path, query) = &queries[0];
    assert_eq!(path, expected_path);
    assert_eq!(
        query.get("symbol").map(String::as_str),
        Some(expected_symbol)
    );
    assert_eq!(query.get("interval").map(String::as_str), Some("1m"));
    assert_eq!(
        query.get("startTime").map(String::as_str),
        Some("1700000000000")
    );
    assert_eq!(
        query.get("endTime").map(String::as_str),
        Some("1700000059999")
    );
    assert_eq!(query.get("limit").map(String::as_str), Some("321"));
    assert_eq!(response.data_type, data_type);
    assert_eq!(bars.len(), 1);
    assert_eq!(bars[0].bar_type, bar_type);
    assert_eq!(bars[0].open.as_decimal(), dec!(50000));
    assert_eq!(bars[0].high.as_decimal(), dec!(50003));
    assert_eq!(bars[0].low.as_decimal(), dec!(49999));
    assert_eq!(bars[0].close.as_decimal(), dec!(50002));
    assert_eq!(bars[0].volume.as_decimal(), dec!(8));
    assert_eq!(bars[0].quote_volume, dec!(375007.50));
    assert_eq!(bars[0].count, 7);
    assert_eq!(bars[0].taker_buy_base_volume, dec!(3));
    assert_eq!(bars[0].taker_buy_quote_volume, dec!(162501.25));
    assert_eq!(bars[0].ts_init, bars[0].ts_event);
}

#[rstest]
#[case(
    "BTCUSDT-PERP.BINANCE-1-MINUTE-BID-EXTERNAL",
    "historical BinanceBar requests require LAST price type"
)]
#[case(
    "BTCUSDT-PERP.BINANCE-1-TICK-LAST-EXTERNAL",
    "historical BinanceBar requests require time aggregation"
)]
fn test_request_historical_binance_bars_rejects_non_venue_bar_types(
    #[case] bar_type: &str,
    #[case] expected: &str,
) {
    let (client, _rx) = create_test_data_client(
        "http://127.0.0.1:1".to_string(),
        "ws://127.0.0.1:1/ws".to_string(),
    );
    let bar_type = BarType::from(bar_type);
    let request = RequestCustomData::new(
        *BINANCE_CLIENT_ID,
        binance_bar_data_type(bar_type),
        None,
        None,
        None,
        UUID4::new(),
        UnixNanos::default(),
        None,
    );

    let error = client.request_data(request).unwrap_err();

    assert_eq!(error.to_string(), expected);
}

#[rstest]
#[case(
    "BTCUSDT-PERP.BINANCE-1-MINUTE-LAST-INTERNAL",
    "Binance historical bars require EXTERNAL aggregation"
)]
#[case(
    "BTCUSDT-PERP.BINANCE-1-TICK-LAST-EXTERNAL",
    "Binance historical bars require time aggregation"
)]
fn test_request_bars_rejects_non_venue_bar_types(#[case] bar_type: &str, #[case] expected: &str) {
    let (client, _rx) = create_test_data_client(
        "http://127.0.0.1:1".to_string(),
        "ws://127.0.0.1:1/ws".to_string(),
    );
    let request = RequestBars::new(
        BarType::from(bar_type),
        None,
        None,
        None,
        Some(*BINANCE_CLIENT_ID),
        UUID4::new(),
        UnixNanos::default(),
        None,
    );

    let error = client.request_bars(request).unwrap_err();

    assert_eq!(error.to_string(), expected);
}

#[rstest]
#[tokio::test]
async fn test_subscribe_trades() {
    let addr = start_data_test_server().await;
    let base_url_http = format!("http://{addr}");
    let base_url_ws = format!("ws://{addr}/ws");

    let (mut client, mut rx) = create_test_data_client(base_url_http, base_url_ws);

    client.connect().await.unwrap();

    // Drain instrument events from connect
    wait_until_async(
        || {
            let found = rx
                .try_recv()
                .is_ok_and(|e| matches!(e, DataEvent::Instrument(_)));
            async move { found }
        },
        Duration::from_secs(5),
    )
    .await;

    while rx.try_recv().is_ok() {}

    let instrument_id = InstrumentId::from("BTCUSDT-PERP.BINANCE");
    let cmd = SubscribeTrades::new(
        instrument_id,
        Some(*BINANCE_CLIENT_ID),
        None,
        nautilus_core::UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );

    client.subscribe_trades(cmd).unwrap();

    wait_until_async(
        || {
            let found = rx.try_recv().is_ok_and(|e| matches!(e, DataEvent::Data(_)));
            async move { found }
        },
        Duration::from_secs(5),
    )
    .await;
}

#[rstest]
#[tokio::test]
async fn test_subscribe_quotes() {
    let addr = start_data_test_server().await;
    let base_url_http = format!("http://{addr}");
    let base_url_ws = format!("ws://{addr}/ws");

    let (mut client, mut rx) = create_test_data_client(base_url_http, base_url_ws);

    client.connect().await.unwrap();

    // Drain instrument events from connect
    wait_until_async(
        || {
            let found = rx
                .try_recv()
                .is_ok_and(|e| matches!(e, DataEvent::Instrument(_)));
            async move { found }
        },
        Duration::from_secs(5),
    )
    .await;

    while rx.try_recv().is_ok() {}

    let instrument_id = InstrumentId::from("BTCUSDT-PERP.BINANCE");
    let cmd = SubscribeQuotes::new(
        instrument_id,
        Some(*BINANCE_CLIENT_ID),
        None,
        nautilus_core::UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );

    client.subscribe_quotes(cmd).unwrap();

    wait_until_async(
        || {
            let found = rx.try_recv().is_ok_and(|e| matches!(e, DataEvent::Data(_)));
            async move { found }
        },
        Duration::from_secs(5),
    )
    .await;
}

#[rstest]
#[tokio::test]
async fn test_subscribe_l1_mbp_uses_book_ticker_and_rejects_invalid_depth() {
    let state = DataTestServerState::default();
    let addr = start_data_test_server_with_state(state.clone()).await;
    let (mut client, mut rx) =
        create_test_data_client(format!("http://{addr}"), format!("ws://{addr}/ws"));
    client.connect().await.unwrap();

    while rx.try_recv().is_ok() {}
    let instrument_id = InstrumentId::from("BTCUSDT-PERP.BINANCE");

    client
        .subscribe_book_deltas(SubscribeBookDeltas::new(
            instrument_id,
            BookType::L1_MBP,
            Some(*BINANCE_CLIENT_ID),
            None,
            UUID4::new(),
            UnixNanos::default(),
            Some(NonZeroUsize::new(1).unwrap()),
            false,
            None,
            None,
        ))
        .unwrap();

    let data = recv_data(&mut rx, Duration::from_secs(5))
        .await
        .expect("expected top-of-book quote");
    let Data::Quote(quote) = data else {
        panic!("expected top-of-book quote");
    };
    assert!(recorded_streams_include(
        &state.subscriptions,
        "btcusdt@bookTicker"
    ));
    assert_eq!(quote.instrument_id, instrument_id);
    assert_eq!(quote.bid_price.as_decimal(), dec!(50000.00));
    assert_eq!(quote.bid_size.as_decimal(), dec!(1.000));
    assert_eq!(quote.ask_price.as_decimal(), dec!(50001.00));
    assert_eq!(quote.ask_size.as_decimal(), dec!(0.500));

    let data = recv_data(&mut rx, Duration::from_secs(5))
        .await
        .expect("expected L1 deltas");
    let Data::BookDeltas(deltas) = data else {
        panic!("expected L1 deltas");
    };
    assert_eq!(*deltas, expected_l1_deltas(quote, 12345));

    let invalid = client.subscribe_book_deltas(SubscribeBookDeltas::new(
        InstrumentId::from("ETHUSDT-PERP.BINANCE"),
        BookType::L1_MBP,
        Some(*BINANCE_CLIENT_ID),
        None,
        UUID4::new(),
        UnixNanos::default(),
        Some(NonZeroUsize::new(2).unwrap()),
        false,
        None,
        None,
    ));
    assert_eq!(
        invalid.unwrap_err().to_string(),
        "Binance Futures L1_MBP supports depth 1 only"
    );

    let conflict = client.subscribe_book_deltas(SubscribeBookDeltas::new(
        instrument_id,
        BookType::L2_MBP,
        Some(*BINANCE_CLIENT_ID),
        None,
        UUID4::new(),
        UnixNanos::default(),
        Some(NonZeroUsize::new(5).unwrap()),
        false,
        None,
        None,
    ));
    assert_eq!(
        conflict.unwrap_err().to_string(),
        "cannot subscribe L1_MBP and L2_MBP for the same Binance Futures instrument"
    );
}

#[rstest]
#[tokio::test]
async fn test_top_of_book_reference_count_shares_quote_and_l1_stream() {
    let state = DataTestServerState::default();
    let addr = start_data_test_server_with_state(state.clone()).await;
    let (mut client, mut rx) =
        create_test_data_client(format!("http://{addr}"), format!("ws://{addr}/ws"));
    client.connect().await.unwrap();

    while rx.try_recv().is_ok() {}
    let instrument_id = InstrumentId::from("BTCUSDT-PERP.BINANCE");

    client
        .subscribe_quotes(SubscribeQuotes::new(
            instrument_id,
            Some(*BINANCE_CLIENT_ID),
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
            BookType::L1_MBP,
            Some(*BINANCE_CLIENT_ID),
            None,
            UUID4::new(),
            UnixNanos::default(),
            None,
            false,
            None,
            None,
        ))
        .unwrap();

    wait_until_async(
        || {
            let count = state
                .subscriptions
                .lock()
                .iter()
                .flatten()
                .filter(|stream| stream.as_str() == "btcusdt@bookTicker")
                .count();
            async move { count == 1 }
        },
        Duration::from_secs(5),
    )
    .await;

    client
        .unsubscribe_quotes(&UnsubscribeQuotes::new(
            instrument_id,
            Some(*BINANCE_CLIENT_ID),
            None,
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ))
        .unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(!recorded_streams_include(
        &state.unsubscriptions,
        "btcusdt@bookTicker"
    ));

    client
        .unsubscribe_book_deltas(&UnsubscribeBookDeltas::new(
            instrument_id,
            Some(*BINANCE_CLIENT_ID),
            None,
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ))
        .unwrap();
    wait_until_async(
        || {
            let found = recorded_streams_include(&state.unsubscriptions, "btcusdt@bookTicker");
            async move { found }
        },
        Duration::from_secs(5),
    )
    .await;
}

#[rstest]
#[tokio::test]
async fn test_subscribe_bars_emits_core_and_binance_bars() {
    let state = DataTestServerState::default();
    let addr = start_data_test_server_with_state(state.clone()).await;
    let (mut client, mut rx) =
        create_test_data_client(format!("http://{addr}"), format!("ws://{addr}/ws"));
    client.connect().await.unwrap();

    while rx.try_recv().is_ok() {}
    let bar_type = BarType::from("BTCUSDT-PERP.BINANCE-1-MINUTE-LAST-EXTERNAL");

    client
        .subscribe_bars(SubscribeBars::new(
            bar_type,
            Some(*BINANCE_CLIENT_ID),
            None,
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ))
        .unwrap();

    let core = recv_data(&mut rx, Duration::from_secs(5))
        .await
        .expect("expected core bar");
    let custom = recv_data(&mut rx, Duration::from_secs(5))
        .await
        .expect("expected BinanceBar custom data");
    let Data::Bar(core) = core else {
        panic!("expected core bar");
    };
    let Data::Custom(custom) = custom else {
        panic!("expected BinanceBar custom data");
    };
    let payload = custom
        .data
        .as_any()
        .downcast_ref::<BinanceBar>()
        .expect("expected BinanceBar payload");

    assert!(recorded_streams_include(
        &state.subscriptions,
        "btcusdt@kline_1m"
    ));
    assert_eq!(core.bar_type, bar_type);
    assert_eq!(core.open.as_decimal(), dec!(50000.00));
    assert_eq!(core.high.as_decimal(), dec!(50003.00));
    assert_eq!(core.low.as_decimal(), dec!(49999.00));
    assert_eq!(core.close.as_decimal(), dec!(50002.00));
    assert_eq!(core.volume.as_decimal(), dec!(7.500));
    assert_eq!(payload.bar_type, bar_type);
    assert_eq!(payload.open, core.open);
    assert_eq!(payload.high, core.high);
    assert_eq!(payload.low, core.low);
    assert_eq!(payload.close, core.close);
    assert_eq!(payload.volume, core.volume);
    assert_eq!(payload.quote_volume, dec!(375007.50));
    assert_eq!(payload.count, 7);
    assert_eq!(payload.taker_buy_base_volume, dec!(3.250));
    assert_eq!(payload.taker_buy_quote_volume, dec!(162501.25));
    assert_eq!(payload.ts_event, core.ts_event);
    assert_eq!(payload.ts_init, core.ts_init);

    let second_error = client
        .subscribe_bars(SubscribeBars::new(
            BarType::from("BTCUSDT-PERP.BINANCE-1-SECOND-LAST-EXTERNAL"),
            Some(*BINANCE_CLIENT_ID),
            None,
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ))
        .unwrap_err();
    assert_eq!(
        second_error.to_string(),
        "Binance Futures does not support second-level kline intervals"
    );
}

#[rstest]
#[tokio::test]
async fn test_subscribe_book_deltas() {
    let addr = start_data_test_server().await;
    let base_url_http = format!("http://{addr}");
    let base_url_ws = format!("ws://{addr}/ws");

    let (mut client, mut rx) = create_test_data_client(base_url_http, base_url_ws);

    client.connect().await.unwrap();

    // Drain instrument events from connect
    wait_until_async(
        || {
            let found = rx
                .try_recv()
                .is_ok_and(|e| matches!(e, DataEvent::Instrument(_)));
            async move { found }
        },
        Duration::from_secs(5),
    )
    .await;

    while rx.try_recv().is_ok() {}

    let instrument_id = InstrumentId::from("BTCUSDT-PERP.BINANCE");
    let cmd = SubscribeBookDeltas::new(
        instrument_id,
        BookType::L2_MBP,
        Some(*BINANCE_CLIENT_ID),
        None,
        nautilus_core::UUID4::new(),
        UnixNanos::default(),
        None,
        false,
        None,
        None,
    );

    client.subscribe_book_deltas(cmd).unwrap();

    let snapshot = recv_data(&mut rx, Duration::from_secs(5))
        .await
        .expect("expected REST depth snapshot data");
    let Data::BookDeltas(snapshot) = snapshot else {
        panic!("expected order book deltas");
    };

    let replayed = recv_data(&mut rx, Duration::from_secs(5))
        .await
        .expect("expected replayed depth diff data");
    let Data::BookDeltas(replayed) = replayed else {
        panic!("expected order book deltas");
    };

    assert_eq!(snapshot.sequence, 1027024);
    assert_eq!(snapshot.deltas.len(), 5);
    assert_eq!(snapshot.deltas[0].action, BookAction::Clear);
    assert_eq!(snapshot.deltas[1].action, BookAction::Add);
    assert_eq!(snapshot.deltas[1].order.side, Some(OrderSide::Buy));
    assert_eq!(snapshot.deltas[1].order.price.as_decimal(), dec!(50000.00));
    assert_eq!(snapshot.deltas[1].order.size.as_decimal(), dec!(1.000));
    assert_eq!(snapshot.deltas[4].action, BookAction::Add);
    assert_eq!(snapshot.deltas[4].order.side, Some(OrderSide::Sell));
    assert_eq!(snapshot.deltas[4].order.price.as_decimal(), dec!(50002.00));
    assert_eq!(snapshot.deltas[4].order.size.as_decimal(), dec!(1.500));
    assert_eq!(snapshot.deltas[4].flags, RecordFlag::F_LAST as u8);

    assert_eq!(replayed.sequence, 1027025);
    assert_eq!(replayed.deltas.len(), 4);
    assert_eq!(replayed.deltas[0].action, BookAction::Update);
    assert_eq!(replayed.deltas[0].order.side, Some(OrderSide::Buy));
    assert_eq!(replayed.deltas[0].order.price.as_decimal(), dec!(50000.00));
    assert_eq!(replayed.deltas[0].order.size.as_decimal(), dec!(1.000));
    assert_eq!(replayed.deltas[3].action, BookAction::Update);
    assert_eq!(replayed.deltas[3].order.side, Some(OrderSide::Sell));
    assert_eq!(replayed.deltas[3].order.price.as_decimal(), dec!(50002.00));
    assert_eq!(replayed.deltas[3].order.size.as_decimal(), dec!(1.500));
    assert_eq!(replayed.deltas[3].flags, RecordFlag::F_LAST as u8);
}

#[rstest]
#[tokio::test]
async fn test_request_book_snapshot_returns_exact_futures_book() {
    let addr = start_data_test_server().await;
    let (mut client, mut rx) =
        create_test_data_client(format!("http://{addr}"), format!("ws://{addr}/ws"));
    client.connect().await.unwrap();

    while rx.try_recv().is_ok() {}
    let instrument_id = InstrumentId::from("BTCUSDT-PERP.BINANCE");

    client
        .request_book_snapshot(RequestBookSnapshot::new(
            instrument_id,
            Some(NonZeroUsize::new(5).unwrap()),
            Some(*BINANCE_CLIENT_ID),
            UUID4::new(),
            UnixNanos::default(),
            None,
        ))
        .unwrap();

    let event = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("timeout waiting for book snapshot")
        .expect("data channel closed");
    let DataEvent::Response(DataResponse::Book(response)) = event else {
        panic!("expected book snapshot response");
    };
    let bids = response.data.bids(None).collect::<Vec<_>>();
    let asks = response.data.asks(None).collect::<Vec<_>>();
    assert_eq!(response.instrument_id, instrument_id);
    assert_eq!(response.data.book_type, BookType::L2_MBP);
    assert_eq!(bids.len(), 2);
    assert_eq!(asks.len(), 2);
    assert_eq!(bids[0].price.value.as_decimal(), dec!(50000.00));
    assert_eq!(bids[0].size_decimal(), dec!(1.000));
    assert_eq!(bids[1].price.value.as_decimal(), dec!(49999.00));
    assert_eq!(bids[1].size_decimal(), dec!(2.000));
    assert_eq!(asks[0].price.value.as_decimal(), dec!(50001.00));
    assert_eq!(asks[0].size_decimal(), dec!(0.500));
    assert_eq!(asks[1].price.value.as_decimal(), dec!(50002.00));
    assert_eq!(asks[1].size_decimal(), dec!(1.500));

    let error = client
        .request_book_snapshot(RequestBookSnapshot::new(
            instrument_id,
            Some(NonZeroUsize::new(6).unwrap()),
            Some(*BINANCE_CLIENT_ID),
            UUID4::new(),
            UnixNanos::default(),
            None,
        ))
        .unwrap_err();
    assert_eq!(
        error.to_string(),
        "invalid Binance Futures order-book depth 6; valid values are [5, 10, 20, 50, 100, 500, 1000]"
    );
}

#[rstest]
#[tokio::test]
async fn test_subscribe_book_deltas_accepts_first_overlapping_diff_with_older_pu() {
    let state = DataTestServerState {
        depth_update_first_update_id: 1027020,
        depth_update_final_update_id: 1027025,
        depth_update_prev_final_update_id: 1027019,
        ..Default::default()
    };
    let addr = start_data_test_server_with_state(state).await;
    let base_url_http = format!("http://{addr}");
    let base_url_ws = format!("ws://{addr}/ws");

    let (mut client, mut rx) = create_test_data_client(base_url_http, base_url_ws);

    client.connect().await.unwrap();

    wait_until_async(
        || {
            let found = rx
                .try_recv()
                .is_ok_and(|e| matches!(e, DataEvent::Instrument(_)));
            async move { found }
        },
        Duration::from_secs(5),
    )
    .await;

    while rx.try_recv().is_ok() {}

    let instrument_id = InstrumentId::from("BTCUSDT-PERP.BINANCE");
    let cmd = SubscribeBookDeltas::new(
        instrument_id,
        BookType::L2_MBP,
        Some(*BINANCE_CLIENT_ID),
        None,
        nautilus_core::UUID4::new(),
        UnixNanos::default(),
        None,
        false,
        None,
        None,
    );

    client.subscribe_book_deltas(cmd).unwrap();

    let snapshot = recv_data(&mut rx, Duration::from_secs(5))
        .await
        .expect("expected REST depth snapshot data");
    let Data::BookDeltas(snapshot) = snapshot else {
        panic!("expected order book deltas");
    };

    let replayed = recv_data(&mut rx, Duration::from_secs(5))
        .await
        .expect("expected replayed depth diff data");
    let Data::BookDeltas(replayed) = replayed else {
        panic!("expected order book deltas");
    };

    assert_eq!(snapshot.sequence, 1027024);
    assert_eq!(snapshot.deltas[0].action, BookAction::Clear);
    assert_eq!(replayed.sequence, 1027025);
    assert_eq!(replayed.deltas[0].action, BookAction::Update);
}

#[rstest]
#[tokio::test]
async fn test_subscribe_book_deltas_rejects_gap_after_snapshot_seam_update() {
    let state = DataTestServerState {
        depth_update_first_update_id: 1027024,
        depth_update_final_update_id: 1027024,
        depth_update_prev_final_update_id: 1027023,
        second_depth_update_prev_final_update_id: Some(1027020),
        depth_update_repetitions: 2,
        depth_response_delay: Duration::from_millis(150),
        ..Default::default()
    };
    let depth_requests = state.depth_requests.clone();
    let addr = start_data_test_server_with_state(state).await;
    let base_url_http = format!("http://{addr}");
    let base_url_ws = format!("ws://{addr}/ws");

    let (mut client, mut rx) = create_test_data_client(base_url_http, base_url_ws);

    client.connect().await.unwrap();

    wait_until_async(
        || {
            let found = rx
                .try_recv()
                .is_ok_and(|e| matches!(e, DataEvent::Instrument(_)));
            async move { found }
        },
        Duration::from_secs(5),
    )
    .await;

    while rx.try_recv().is_ok() {}

    let instrument_id = InstrumentId::from("BTCUSDT-PERP.BINANCE");
    let cmd = SubscribeBookDeltas::new(
        instrument_id,
        BookType::L2_MBP,
        Some(*BINANCE_CLIENT_ID),
        None,
        nautilus_core::UUID4::new(),
        UnixNanos::default(),
        None,
        false,
        None,
        None,
    );

    client.subscribe_book_deltas(cmd).unwrap();

    wait_until_async(
        || {
            let request_count = depth_requests.load(Ordering::Relaxed);
            async move { request_count >= 1 }
        },
        Duration::from_secs(5),
    )
    .await;

    assert!(recv_data(&mut rx, Duration::from_secs(1)).await.is_none());
}

#[rstest]
#[tokio::test]
async fn test_subscribe_book_deltas_waits_for_first_depth_update_before_snapshot() {
    let state = DataTestServerState {
        depth_update_delay: Duration::from_millis(500),
        ..Default::default()
    };
    let depth_requests = state.depth_requests.clone();
    let addr = start_data_test_server_with_state(state).await;
    let base_url_http = format!("http://{addr}");
    let base_url_ws = format!("ws://{addr}/ws");

    let (mut client, mut rx) = create_test_data_client(base_url_http, base_url_ws);

    client.connect().await.unwrap();

    wait_until_async(
        || {
            let found = rx
                .try_recv()
                .is_ok_and(|e| matches!(e, DataEvent::Instrument(_)));
            async move { found }
        },
        Duration::from_secs(5),
    )
    .await;

    while rx.try_recv().is_ok() {}

    let instrument_id = InstrumentId::from("BTCUSDT-PERP.BINANCE");
    let cmd = SubscribeBookDeltas::new(
        instrument_id,
        BookType::L2_MBP,
        Some(*BINANCE_CLIENT_ID),
        None,
        nautilus_core::UUID4::new(),
        UnixNanos::default(),
        None,
        false,
        None,
        None,
    );

    client.subscribe_book_deltas(cmd).unwrap();

    assert!(
        recv_data(&mut rx, Duration::from_millis(150))
            .await
            .is_none()
    );
    assert_eq!(depth_requests.load(Ordering::Relaxed), 0);

    wait_until_async(
        || {
            let request_count = depth_requests.load(Ordering::Relaxed);
            async move { request_count >= 1 }
        },
        Duration::from_secs(5),
    )
    .await;

    let snapshot = recv_data(&mut rx, Duration::from_secs(5))
        .await
        .expect("expected REST depth snapshot data after first diff");
    let Data::BookDeltas(snapshot) = snapshot else {
        panic!("expected order book deltas");
    };

    let replayed = recv_data(&mut rx, Duration::from_secs(5))
        .await
        .expect("expected replayed depth diff data after first diff");
    let Data::BookDeltas(replayed) = replayed else {
        panic!("expected order book deltas");
    };

    assert_eq!(snapshot.sequence, 1027024);
    assert_eq!(snapshot.deltas[0].action, BookAction::Clear);
    assert_eq!(replayed.sequence, 1027025);
    assert_eq!(replayed.deltas[0].action, BookAction::Update);
}

#[rstest]
#[tokio::test]
async fn test_subscribe_book_deltas_keeps_buffered_diffs_across_overlap_retry() {
    let state = DataTestServerState {
        depth_snapshot_last_update_ids: vec![1027023, 1027024],
        depth_update_repetitions: 2,
        ..Default::default()
    };
    let depth_requests = state.depth_requests.clone();
    let addr = start_data_test_server_with_state(state).await;
    let base_url_http = format!("http://{addr}");
    let base_url_ws = format!("ws://{addr}/ws");

    let (mut client, mut rx) = create_test_data_client(base_url_http, base_url_ws);

    client.connect().await.unwrap();

    wait_until_async(
        || {
            let found = rx
                .try_recv()
                .is_ok_and(|e| matches!(e, DataEvent::Instrument(_)));
            async move { found }
        },
        Duration::from_secs(5),
    )
    .await;

    while rx.try_recv().is_ok() {}

    let instrument_id = InstrumentId::from("BTCUSDT-PERP.BINANCE");
    let cmd = SubscribeBookDeltas::new(
        instrument_id,
        BookType::L2_MBP,
        Some(*BINANCE_CLIENT_ID),
        None,
        nautilus_core::UUID4::new(),
        UnixNanos::default(),
        None,
        false,
        None,
        None,
    );

    client.subscribe_book_deltas(cmd).unwrap();

    wait_until_async(
        || {
            let request_count = depth_requests.load(Ordering::Relaxed);
            async move { request_count >= 2 }
        },
        Duration::from_secs(5),
    )
    .await;

    let snapshot = recv_data(&mut rx, Duration::from_secs(5))
        .await
        .expect("expected REST depth snapshot data after overlap retry");
    let Data::BookDeltas(snapshot) = snapshot else {
        panic!("expected order book deltas");
    };

    let replayed = recv_data(&mut rx, Duration::from_secs(5))
        .await
        .expect("expected retained replayed depth diff data");
    let Data::BookDeltas(replayed) = replayed else {
        panic!("expected order book deltas");
    };

    assert_eq!(snapshot.sequence, 1027024);
    assert_eq!(replayed.sequence, 1027025);
    assert_eq!(replayed.deltas[0].action, BookAction::Update);
    assert_eq!(depth_requests.load(Ordering::Relaxed), 2);
}

#[rstest]
#[tokio::test]
async fn test_subscribe_book_deltas_retries_transient_depth_snapshot_failure() {
    let state = DataTestServerState {
        depth_failures_before_success: 1,
        depth_update_delay: Duration::from_millis(500),
        depth_update_repetitions: 2,
        ..Default::default()
    };
    let depth_requests = state.depth_requests.clone();
    let addr = start_data_test_server_with_state(state).await;
    let base_url_http = format!("http://{addr}");
    let base_url_ws = format!("ws://{addr}/ws");

    let (mut client, mut rx) = create_test_data_client(base_url_http, base_url_ws);

    client.connect().await.unwrap();

    wait_until_async(
        || {
            let found = rx
                .try_recv()
                .is_ok_and(|e| matches!(e, DataEvent::Instrument(_)));
            async move { found }
        },
        Duration::from_secs(5),
    )
    .await;

    while rx.try_recv().is_ok() {}

    let instrument_id = InstrumentId::from("BTCUSDT-PERP.BINANCE");
    let cmd = SubscribeBookDeltas::new(
        instrument_id,
        BookType::L2_MBP,
        Some(*BINANCE_CLIENT_ID),
        None,
        nautilus_core::UUID4::new(),
        UnixNanos::default(),
        None,
        false,
        None,
        None,
    );

    client.subscribe_book_deltas(cmd).unwrap();

    wait_until_async(
        || {
            let request_count = depth_requests.load(Ordering::Relaxed);
            async move { request_count >= 2 }
        },
        Duration::from_secs(5),
    )
    .await;

    let snapshot = recv_data(&mut rx, Duration::from_secs(5))
        .await
        .expect("expected REST depth snapshot data after retry");
    let Data::BookDeltas(snapshot) = snapshot else {
        panic!("expected order book deltas");
    };

    let replayed = recv_data(&mut rx, Duration::from_secs(5))
        .await
        .expect("expected replayed depth diff data after retry");
    let Data::BookDeltas(replayed) = replayed else {
        panic!("expected order book deltas");
    };

    assert_eq!(snapshot.sequence, 1027024);
    assert_eq!(snapshot.deltas[0].action, BookAction::Clear);
    assert_eq!(replayed.sequence, 1027025);
    assert_eq!(replayed.deltas[0].action, BookAction::Update);
    assert_eq!(depth_requests.load(Ordering::Relaxed), 2);
}

#[rstest]
#[tokio::test]
async fn test_subscribe_mark_prices() {
    let addr = start_data_test_server().await;
    let base_url_http = format!("http://{addr}");
    let base_url_ws = format!("ws://{addr}/ws");

    let (mut client, mut rx) = create_test_data_client(base_url_http, base_url_ws);

    client.connect().await.unwrap();

    // Drain instrument events from connect
    wait_until_async(
        || {
            let found = rx
                .try_recv()
                .is_ok_and(|e| matches!(e, DataEvent::Instrument(_)));
            async move { found }
        },
        Duration::from_secs(5),
    )
    .await;

    while rx.try_recv().is_ok() {}

    let instrument_id = InstrumentId::from("BTCUSDT-PERP.BINANCE");
    let cmd = SubscribeMarkPrices::new(
        instrument_id,
        Some(*BINANCE_CLIENT_ID),
        None,
        nautilus_core::UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );

    client.subscribe_mark_prices(cmd).unwrap();

    wait_until_async(
        || {
            let found = loop {
                match rx.try_recv() {
                    Ok(DataEvent::Data(Data::Custom(custom))) => {
                        let Some(update) = custom
                            .data
                            .as_any()
                            .downcast_ref::<BinanceFuturesMarkPriceUpdate>()
                        else {
                            continue;
                        };
                        break update.instrument_id == instrument_id
                            && update.mark_price.as_decimal() == dec!(50000.50)
                            && update.index_price.as_decimal() == dec!(50000.25)
                            && update.estimated_settle_price.as_decimal() == dec!(50000.75)
                            && update.funding_rate == dec!(0.00010000)
                            && update.next_funding_time
                                == Some(UnixNanos::from_millis(1700028800000));
                    }
                    Ok(_) => {}
                    Err(_) => break false,
                }
            };
            async move { found }
        },
        Duration::from_secs(5),
    )
    .await;
}

#[rstest]
#[tokio::test]
async fn test_subscribe_custom_ticker_for_instrument_emits_custom_data() {
    let state = DataTestServerState::default();
    let addr = start_data_test_server_with_state(state.clone()).await;
    let base_url_http = format!("http://{addr}");
    let base_url_ws = format!("ws://{addr}/ws");

    let (mut client, mut rx) = create_test_data_client(base_url_http, base_url_ws);
    client.connect().await.unwrap();

    wait_until_async(
        || {
            let found = rx
                .try_recv()
                .is_ok_and(|e| matches!(e, DataEvent::Instrument(_)));
            async move { found }
        },
        Duration::from_secs(5),
    )
    .await;

    while rx.try_recv().is_ok() {}

    let instrument_id = InstrumentId::from("BTCUSDT-PERP.BINANCE");
    let data_type = ticker_data_type_for_instrument(instrument_id);
    let cmd = SubscribeCustomData::new(
        Some(*BINANCE_CLIENT_ID),
        None,
        data_type.clone(),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );
    client.subscribe(cmd).unwrap();

    wait_until_async(
        || {
            let found = recorded_streams_include(&state.subscriptions, "btcusdt@ticker");
            async move { found }
        },
        Duration::from_secs(5),
    )
    .await;

    wait_until_async(
        || {
            let found = rx.try_recv().is_ok_and(|event| {
                let DataEvent::Data(Data::Custom(custom)) = event else {
                    return false;
                };

                custom
                    .data
                    .as_any()
                    .downcast_ref::<BinanceFuturesTicker>()
                    .is_some_and(|ticker| {
                        ticker.instrument_id == instrument_id
                            && custom.data_type == data_type
                            && ticker.last_price == dec!(50000.10000000)
                            && ticker.volume == dec!(1234.567)
                            && ticker.num_trades == 101
                    })
            });
            async move { found }
        },
        Duration::from_secs(5),
    )
    .await;
}

#[rstest]
#[tokio::test]
async fn test_unsubscribed_custom_ticker_frame_is_ignored() {
    let state = DataTestServerState {
        send_ticker_on_connect: true,
        ..Default::default()
    };
    let addr = start_data_test_server_with_state(state).await;
    let base_url_http = format!("http://{addr}");
    let base_url_ws = format!("ws://{addr}/ws");

    let (mut client, mut rx) = create_test_data_client(base_url_http, base_url_ws);
    client.connect().await.unwrap();

    wait_until_async(
        || {
            let found = rx
                .try_recv()
                .is_ok_and(|e| matches!(e, DataEvent::Instrument(_)));
            async move { found }
        },
        Duration::from_secs(5),
    )
    .await;

    tokio::time::sleep(Duration::from_millis(500)).await;

    let mut emitted_ticker = false;

    while let Ok(event) = rx.try_recv() {
        let DataEvent::Data(Data::Custom(custom)) = event else {
            continue;
        };

        if custom
            .data
            .as_any()
            .downcast_ref::<BinanceFuturesTicker>()
            .is_some()
        {
            emitted_ticker = true;
        }
    }

    assert!(
        !emitted_ticker,
        "expected unsubscribed Binance Futures ticker frames to be ignored",
    );
}

#[rstest]
#[tokio::test]
async fn test_subscribe_custom_liquidations_for_instrument() {
    let addr = start_data_test_server().await;
    let base_url_http = format!("http://{addr}");
    let base_url_ws = format!("ws://{addr}/ws");

    let (mut client, mut rx) = create_test_data_client(base_url_http, base_url_ws);
    client.connect().await.unwrap();

    wait_until_async(
        || {
            let found = rx
                .try_recv()
                .is_ok_and(|e| matches!(e, DataEvent::Instrument(_)));
            async move { found }
        },
        Duration::from_secs(5),
    )
    .await;

    while rx.try_recv().is_ok() {}

    let instrument_id = InstrumentId::from("BTCUSDT-PERP.BINANCE");
    let data_type = liquidation_data_type_for_instrument(instrument_id);
    let cmd = SubscribeCustomData::new(
        Some(*BINANCE_CLIENT_ID),
        None,
        data_type.clone(),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );
    client.subscribe(cmd).unwrap();

    wait_until_async(
        || {
            let found = rx.try_recv().is_ok_and(|event| {
                let DataEvent::Data(Data::Custom(custom)) = event else {
                    return false;
                };

                custom
                    .data
                    .as_any()
                    .downcast_ref::<BinanceFuturesLiquidation>()
                    .is_some_and(|liq| {
                        liq.instrument_id == instrument_id
                            && custom.data_type == data_type
                            && liq.last_filled_qty.to_string() == "0.001"
                            && liq.accumulated_qty.to_string() == "0.003"
                    })
            });
            async move { found }
        },
        Duration::from_secs(5),
    )
    .await;
}

#[rstest]
#[tokio::test]
async fn test_subscribe_custom_liquidations_all_market() {
    let addr = start_data_test_server().await;
    let base_url_http = format!("http://{addr}");
    let base_url_ws = format!("ws://{addr}/ws");

    let (mut client, mut rx) = create_test_data_client(base_url_http, base_url_ws);
    client.connect().await.unwrap();

    wait_until_async(
        || {
            let found = rx
                .try_recv()
                .is_ok_and(|e| matches!(e, DataEvent::Instrument(_)));
            async move { found }
        },
        Duration::from_secs(5),
    )
    .await;

    while rx.try_recv().is_ok() {}

    let data_type = DataType::new("BinanceFuturesLiquidation", None, None);
    let cmd = SubscribeCustomData::new(
        Some(*BINANCE_CLIENT_ID),
        None,
        data_type.clone(),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );
    client.subscribe(cmd).unwrap();

    wait_until_async(
        || {
            let found = rx.try_recv().is_ok_and(|event| {
                let DataEvent::Data(Data::Custom(custom)) = event else {
                    return false;
                };

                custom
                    .data
                    .as_any()
                    .downcast_ref::<BinanceFuturesLiquidation>()
                    .is_some_and(|liq| {
                        liq.instrument_id == InstrumentId::from("BTCUSDT-PERP.BINANCE")
                            && custom.data_type == data_type
                    })
            });
            async move { found }
        },
        Duration::from_secs(5),
    )
    .await;
}

#[rstest]
#[tokio::test]
async fn test_subscribe_custom_liquidations_overlap_routes_single_event() {
    let addr = start_data_test_server().await;
    let base_url_http = format!("http://{addr}");
    let base_url_ws = format!("ws://{addr}/ws");

    let (mut client, mut rx) = create_test_data_client(base_url_http, base_url_ws);
    client.connect().await.unwrap();

    wait_until_async(
        || {
            let found = rx
                .try_recv()
                .is_ok_and(|e| matches!(e, DataEvent::Instrument(_)));
            async move { found }
        },
        Duration::from_secs(5),
    )
    .await;

    while rx.try_recv().is_ok() {}

    let instrument_id = InstrumentId::from("BTCUSDT-PERP.BINANCE");
    let specific_data_type = liquidation_data_type_for_instrument(instrument_id);
    let all_market_data_type = DataType::new("BinanceFuturesLiquidation", None, None);

    client
        .subscribe(SubscribeCustomData::new(
            Some(*BINANCE_CLIENT_ID),
            None,
            specific_data_type.clone(),
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ))
        .unwrap();

    wait_until_async(
        || {
            let found = rx.try_recv().is_ok_and(|event| {
                let DataEvent::Data(Data::Custom(custom)) = event else {
                    return false;
                };
                custom.data_type == specific_data_type
            });
            async move { found }
        },
        Duration::from_secs(5),
    )
    .await;

    while rx.try_recv().is_ok() {}

    client
        .subscribe(SubscribeCustomData::new(
            Some(*BINANCE_CLIENT_ID),
            None,
            all_market_data_type.clone(),
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ))
        .unwrap();

    wait_until_async(
        || {
            let found = rx.try_recv().is_ok_and(|event| {
                let DataEvent::Data(Data::Custom(custom)) = event else {
                    return false;
                };
                custom.data_type == all_market_data_type
            });
            async move { found }
        },
        Duration::from_secs(5),
    )
    .await;

    tokio::time::sleep(Duration::from_millis(250)).await;

    let mut queued_custom_count = 0_u32;

    while let Ok(event) = rx.try_recv() {
        if matches!(event, DataEvent::Data(Data::Custom(_))) {
            queued_custom_count += 1;
        }
    }

    assert_eq!(
        queued_custom_count, 0,
        "expected overlap subscription to route a single liquidation event",
    );
}

#[rstest]
#[tokio::test]
async fn test_unsubscribe_all_market_restores_specific_liquidation_streams() {
    let addr = start_data_test_server().await;
    let base_url_http = format!("http://{addr}");
    let base_url_ws = format!("ws://{addr}/ws");

    let (mut client, mut rx) = create_test_data_client(base_url_http, base_url_ws);
    client.connect().await.unwrap();

    wait_until_async(
        || {
            let found = rx
                .try_recv()
                .is_ok_and(|e| matches!(e, DataEvent::Instrument(_)));
            async move { found }
        },
        Duration::from_secs(5),
    )
    .await;

    while rx.try_recv().is_ok() {}

    let instrument_id = InstrumentId::from("BTCUSDT-PERP.BINANCE");
    let specific_data_type = liquidation_data_type_for_instrument(instrument_id);
    let all_market_data_type = DataType::new("BinanceFuturesLiquidation", None, None);

    client
        .subscribe(SubscribeCustomData::new(
            Some(*BINANCE_CLIENT_ID),
            None,
            specific_data_type.clone(),
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ))
        .unwrap();

    wait_until_async(
        || {
            let found = rx.try_recv().is_ok_and(|event| {
                let DataEvent::Data(Data::Custom(custom)) = event else {
                    return false;
                };
                custom.data_type == specific_data_type
            });
            async move { found }
        },
        Duration::from_secs(5),
    )
    .await;

    while rx.try_recv().is_ok() {}

    client
        .subscribe(SubscribeCustomData::new(
            Some(*BINANCE_CLIENT_ID),
            None,
            all_market_data_type.clone(),
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ))
        .unwrap();

    wait_until_async(
        || {
            let found = rx.try_recv().is_ok_and(|event| {
                let DataEvent::Data(Data::Custom(custom)) = event else {
                    return false;
                };
                custom.data_type == all_market_data_type
            });
            async move { found }
        },
        Duration::from_secs(5),
    )
    .await;

    while rx.try_recv().is_ok() {}

    client
        .unsubscribe(&UnsubscribeCustomData::new(
            Some(*BINANCE_CLIENT_ID),
            None,
            all_market_data_type,
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ))
        .unwrap();

    wait_until_async(
        || {
            let found = rx.try_recv().is_ok_and(|event| {
                let DataEvent::Data(Data::Custom(custom)) = event else {
                    return false;
                };
                custom.data_type == specific_data_type
            });
            async move { found }
        },
        Duration::from_secs(5),
    )
    .await;
}

#[rstest]
#[tokio::test]
async fn test_rapid_all_market_unsubscribe_does_not_route_all_market_as_specific() {
    let addr = start_data_test_server().await;
    let base_url_http = format!("http://{addr}");
    let base_url_ws = format!("ws://{addr}/ws");

    let (mut client, mut rx) = create_test_data_client(base_url_http, base_url_ws);
    client.connect().await.unwrap();

    wait_until_async(
        || {
            let found = rx
                .try_recv()
                .is_ok_and(|e| matches!(e, DataEvent::Instrument(_)));
            async move { found }
        },
        Duration::from_secs(5),
    )
    .await;

    while rx.try_recv().is_ok() {}

    let instrument_id = InstrumentId::from("BTCUSDT-PERP.BINANCE");
    let specific_data_type = liquidation_data_type_for_instrument(instrument_id);
    let all_market_data_type = DataType::new("BinanceFuturesLiquidation", None, None);

    client
        .subscribe(SubscribeCustomData::new(
            Some(*BINANCE_CLIENT_ID),
            None,
            specific_data_type.clone(),
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ))
        .unwrap();

    wait_until_async(
        || {
            let found = rx.try_recv().is_ok_and(|event| {
                let DataEvent::Data(Data::Custom(custom)) = event else {
                    return false;
                };
                custom.data_type == specific_data_type
            });
            async move { found }
        },
        Duration::from_secs(5),
    )
    .await;

    while rx.try_recv().is_ok() {}

    client
        .subscribe(SubscribeCustomData::new(
            Some(*BINANCE_CLIENT_ID),
            None,
            all_market_data_type.clone(),
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ))
        .unwrap();

    wait_until_async(
        || {
            let found = rx.try_recv().is_ok_and(|event| {
                let DataEvent::Data(Data::Custom(custom)) = event else {
                    return false;
                };

                custom
                    .data
                    .as_any()
                    .downcast_ref::<BinanceFuturesLiquidation>()
                    .is_some_and(|liq| {
                        custom.data_type == all_market_data_type
                            && liq.last_filled_qty.to_string() == "0.002"
                    })
            });
            async move { found }
        },
        Duration::from_secs(5),
    )
    .await;

    while rx.try_recv().is_ok() {}

    client
        .unsubscribe(&UnsubscribeCustomData::new(
            Some(*BINANCE_CLIENT_ID),
            None,
            all_market_data_type,
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ))
        .unwrap();

    tokio::time::sleep(Duration::from_millis(500)).await;

    let mut routed_all_market_as_specific = false;

    while let Ok(event) = rx.try_recv() {
        let DataEvent::Data(Data::Custom(custom)) = event else {
            continue;
        };
        let Some(liq) = custom
            .data
            .as_any()
            .downcast_ref::<BinanceFuturesLiquidation>()
        else {
            continue;
        };

        if custom.data_type == specific_data_type && liq.last_filled_qty.to_string() == "0.002" {
            routed_all_market_as_specific = true;
        }
    }

    assert!(
        !routed_all_market_as_specific,
        "expected transient all-market frames to keep the all-market data type",
    );
}

#[rstest]
#[tokio::test]
async fn test_unsubscribe_trades() {
    let addr = start_data_test_server().await;
    let base_url_http = format!("http://{addr}");
    let base_url_ws = format!("ws://{addr}/ws");

    let (mut client, mut rx) = create_test_data_client(base_url_http, base_url_ws);

    client.connect().await.unwrap();

    // Drain instrument events
    wait_until_async(
        || {
            let found = rx
                .try_recv()
                .is_ok_and(|e| matches!(e, DataEvent::Instrument(_)));
            async move { found }
        },
        Duration::from_secs(5),
    )
    .await;

    while rx.try_recv().is_ok() {}

    let instrument_id = InstrumentId::from("BTCUSDT-PERP.BINANCE");

    let sub_cmd = SubscribeTrades::new(
        instrument_id,
        Some(*BINANCE_CLIENT_ID),
        None,
        nautilus_core::UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );
    client.subscribe_trades(sub_cmd).unwrap();

    wait_until_async(
        || {
            let found = rx.try_recv().is_ok_and(|e| matches!(e, DataEvent::Data(_)));
            async move { found }
        },
        Duration::from_secs(5),
    )
    .await;

    while rx.try_recv().is_ok() {}

    let unsub_cmd = UnsubscribeTrades::new(
        instrument_id,
        Some(*BINANCE_CLIENT_ID),
        None,
        nautilus_core::UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );
    let result = client.unsubscribe_trades(&unsub_cmd);
    result.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_unsubscribe_quotes() {
    let addr = start_data_test_server().await;
    let base_url_http = format!("http://{addr}");
    let base_url_ws = format!("ws://{addr}/ws");

    let (mut client, mut rx) = create_test_data_client(base_url_http, base_url_ws);

    client.connect().await.unwrap();

    // Drain instrument events
    wait_until_async(
        || {
            let found = rx
                .try_recv()
                .is_ok_and(|e| matches!(e, DataEvent::Instrument(_)));
            async move { found }
        },
        Duration::from_secs(5),
    )
    .await;

    while rx.try_recv().is_ok() {}

    let instrument_id = InstrumentId::from("BTCUSDT-PERP.BINANCE");

    let sub_cmd = SubscribeQuotes::new(
        instrument_id,
        Some(*BINANCE_CLIENT_ID),
        None,
        nautilus_core::UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );
    client.subscribe_quotes(sub_cmd).unwrap();

    wait_until_async(
        || {
            let found = rx.try_recv().is_ok_and(|e| matches!(e, DataEvent::Data(_)));
            async move { found }
        },
        Duration::from_secs(5),
    )
    .await;

    while rx.try_recv().is_ok() {}

    let unsub_cmd = UnsubscribeQuotes::new(
        instrument_id,
        Some(*BINANCE_CLIENT_ID),
        None,
        nautilus_core::UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );
    let result = client.unsubscribe_quotes(&unsub_cmd);
    result.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_unsubscribe_custom_ticker_for_instrument_sends_stream_unsubscribe() {
    let state = DataTestServerState::default();
    let addr = start_data_test_server_with_state(state.clone()).await;
    let base_url_http = format!("http://{addr}");
    let base_url_ws = format!("ws://{addr}/ws");

    let (mut client, mut rx) = create_test_data_client(base_url_http, base_url_ws);
    client.connect().await.unwrap();

    wait_until_async(
        || {
            let found = rx
                .try_recv()
                .is_ok_and(|e| matches!(e, DataEvent::Instrument(_)));
            async move { found }
        },
        Duration::from_secs(5),
    )
    .await;

    while rx.try_recv().is_ok() {}

    let instrument_id = InstrumentId::from("BTCUSDT-PERP.BINANCE");
    let data_type = ticker_data_type_for_instrument(instrument_id);
    let sub_cmd = SubscribeCustomData::new(
        Some(*BINANCE_CLIENT_ID),
        None,
        data_type.clone(),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );
    client.subscribe(sub_cmd).unwrap();

    wait_until_async(
        || {
            let found = rx
                .try_recv()
                .is_ok_and(|e| matches!(e, DataEvent::Data(Data::Custom(_))));
            async move { found }
        },
        Duration::from_secs(5),
    )
    .await;

    while rx.try_recv().is_ok() {}

    let unsub_cmd = UnsubscribeCustomData::new(
        Some(*BINANCE_CLIENT_ID),
        None,
        data_type,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );
    client.unsubscribe(&unsub_cmd).unwrap();

    wait_until_async(
        || {
            let found = recorded_streams_include(&state.unsubscriptions, "btcusdt@ticker");
            async move { found }
        },
        Duration::from_secs(5),
    )
    .await;
}

fn ticker_data_type_for_instrument(instrument_id: InstrumentId) -> DataType {
    let mut metadata = Params::new();
    metadata.insert(
        "instrument_id".to_string(),
        serde_json::Value::String(instrument_id.to_string()),
    );
    DataType::new(
        "BinanceFuturesTicker",
        Some(metadata),
        Some(instrument_id.to_string()),
    )
}

fn recorded_streams_include(records: &Arc<Mutex<Vec<Vec<String>>>>, stream: &str) -> bool {
    records
        .lock()
        .iter()
        .any(|streams| streams.iter().any(|recorded| recorded == stream))
}

fn ticker_stream_payload() -> serde_json::Value {
    json!({
        "e": "24hrTicker",
        "E": 1700000000000_i64,
        "s": "BTCUSDT",
        "p": "-100.10000000",
        "P": "-0.200",
        "w": "50050.25000000",
        "c": "50000.10000000",
        "Q": "0.010",
        "o": "50100.20000000",
        "h": "50200.30000000",
        "l": "49900.40000000",
        "v": "1234.567",
        "q": "61734567.89000000",
        "O": 1699913600000_i64,
        "C": 1700000000000_i64,
        "F": 100,
        "L": 200,
        "n": 101
    })
}

#[rstest]
#[tokio::test]
async fn test_unsubscribe_custom_liquidations_for_instrument() {
    let addr = start_data_test_server().await;
    let base_url_http = format!("http://{addr}");
    let base_url_ws = format!("ws://{addr}/ws");

    let (mut client, mut rx) = create_test_data_client(base_url_http, base_url_ws);
    client.connect().await.unwrap();

    wait_until_async(
        || {
            let found = rx
                .try_recv()
                .is_ok_and(|e| matches!(e, DataEvent::Instrument(_)));
            async move { found }
        },
        Duration::from_secs(5),
    )
    .await;

    while rx.try_recv().is_ok() {}

    let instrument_id = InstrumentId::from("BTCUSDT-PERP.BINANCE");
    let data_type = liquidation_data_type_for_instrument(instrument_id);
    let sub_cmd = SubscribeCustomData::new(
        Some(*BINANCE_CLIENT_ID),
        None,
        data_type.clone(),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );
    client.subscribe(sub_cmd).unwrap();

    wait_until_async(
        || {
            let found = rx
                .try_recv()
                .is_ok_and(|e| matches!(e, DataEvent::Data(Data::Custom(_))));
            async move { found }
        },
        Duration::from_secs(5),
    )
    .await;

    while rx.try_recv().is_ok() {}

    let unsub_cmd = UnsubscribeCustomData::new(
        Some(*BINANCE_CLIENT_ID),
        None,
        data_type,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );
    client.unsubscribe(&unsub_cmd).unwrap();
}

#[rstest]
#[tokio::test]
async fn test_unsubscribe_custom_liquidations_all_market() {
    let addr = start_data_test_server().await;
    let base_url_http = format!("http://{addr}");
    let base_url_ws = format!("ws://{addr}/ws");

    let (mut client, mut rx) = create_test_data_client(base_url_http, base_url_ws);
    client.connect().await.unwrap();

    wait_until_async(
        || {
            let found = rx
                .try_recv()
                .is_ok_and(|e| matches!(e, DataEvent::Instrument(_)));
            async move { found }
        },
        Duration::from_secs(5),
    )
    .await;

    while rx.try_recv().is_ok() {}

    let data_type = DataType::new("BinanceFuturesLiquidation", None, None);
    let sub_cmd = SubscribeCustomData::new(
        Some(*BINANCE_CLIENT_ID),
        None,
        data_type.clone(),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );
    client.subscribe(sub_cmd).unwrap();

    wait_until_async(
        || {
            let found = rx
                .try_recv()
                .is_ok_and(|e| matches!(e, DataEvent::Data(Data::Custom(_))));
            async move { found }
        },
        Duration::from_secs(5),
    )
    .await;

    while rx.try_recv().is_ok() {}

    let unsub_cmd = UnsubscribeCustomData::new(
        Some(*BINANCE_CLIENT_ID),
        None,
        data_type,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );
    client.unsubscribe(&unsub_cmd).unwrap();
}

#[rstest]
#[tokio::test]
async fn test_connect_disconnect_reconnect() {
    let addr = start_data_test_server().await;
    let base_url_http = format!("http://{addr}");
    let base_url_ws = format!("ws://{addr}/ws");

    let (mut client, mut rx) = create_test_data_client(base_url_http, base_url_ws);

    client.connect().await.unwrap();
    assert!(client.is_connected());

    // Drain instrument events
    wait_until_async(
        || {
            let found = rx
                .try_recv()
                .is_ok_and(|e| matches!(e, DataEvent::Instrument(_)));
            async move { found }
        },
        Duration::from_secs(5),
    )
    .await;

    client.disconnect().await.unwrap();
    assert!(!client.is_connected());

    // Reconnect
    client.connect().await.unwrap();
    assert!(client.is_connected());

    // Should emit instruments again on reconnect
    wait_until_async(
        || {
            let found = rx
                .try_recv()
                .is_ok_and(|e| matches!(e, DataEvent::Instrument(_)));
            async move { found }
        },
        Duration::from_secs(5),
    )
    .await;
}

#[rstest]
#[tokio::test]
async fn test_subscribe_trades_and_quotes_simultaneously() {
    let addr = start_data_test_server().await;
    let base_url_http = format!("http://{addr}");
    let base_url_ws = format!("ws://{addr}/ws");

    let (mut client, mut rx) = create_test_data_client(base_url_http, base_url_ws);

    client.connect().await.unwrap();

    // Drain instrument events
    wait_until_async(
        || {
            let found = rx
                .try_recv()
                .is_ok_and(|e| matches!(e, DataEvent::Instrument(_)));
            async move { found }
        },
        Duration::from_secs(5),
    )
    .await;

    while rx.try_recv().is_ok() {}

    let instrument_id = InstrumentId::from("BTCUSDT-PERP.BINANCE");

    let trades_cmd = SubscribeTrades::new(
        instrument_id,
        Some(*BINANCE_CLIENT_ID),
        None,
        nautilus_core::UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );
    let quotes_cmd = SubscribeQuotes::new(
        instrument_id,
        Some(*BINANCE_CLIENT_ID),
        None,
        nautilus_core::UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );

    client.subscribe_trades(trades_cmd).unwrap();
    client.subscribe_quotes(quotes_cmd).unwrap();

    let mut data_count = 0;
    wait_until_async(
        || {
            while rx.try_recv().is_ok_and(|e| matches!(e, DataEvent::Data(_))) {
                data_count += 1;
            }
            async move { data_count >= 2 }
        },
        Duration::from_secs(5),
    )
    .await;
}

fn expected_l1_deltas(quote: QuoteTick, sequence: u64) -> OrderBookDeltas {
    let bid = OrderBookDelta::new(
        quote.instrument_id,
        BookAction::Update,
        BookOrder::new(OrderSide::Buy, quote.bid_price, quote.bid_size, 0),
        RecordFlag::F_MBP as u8,
        sequence,
        quote.ts_event,
        quote.ts_init,
    );
    let ask = OrderBookDelta::new(
        quote.instrument_id,
        BookAction::Update,
        BookOrder::new(OrderSide::Sell, quote.ask_price, quote.ask_size, 0),
        RecordFlag::F_MBP as u8 | RecordFlag::F_LAST as u8,
        sequence,
        quote.ts_event,
        quote.ts_init,
    );

    OrderBookDeltas::new(quote.instrument_id, vec![bid, ask])
}
