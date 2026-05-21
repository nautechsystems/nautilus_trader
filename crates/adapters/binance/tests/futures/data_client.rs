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

use std::{collections::HashMap, net::SocketAddr, num::NonZeroUsize, time::Duration};

use axum::{
    Router,
    extract::{
        RawQuery,
        ws::{Message, WebSocket},
    },
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
};
use nautilus_binance::{
    common::{
        consts::{BINANCE_CLIENT_ID, BINANCE_VENUE},
        enums::BinanceProductType,
    },
    config::BinanceDataClientConfig,
    data_types::{
        BinanceFuturesLiquidation, BinanceFuturesOpenInterest, BinanceFuturesOpenInterestHist,
    },
    futures::BinanceFuturesDataClient,
};
use nautilus_common::{
    clients::DataClient,
    live::runner::set_data_event_sender,
    messages::{
        DataEvent,
        data::{
            DataResponse, RequestCustomData,
            subscribe::{
                SubscribeBookDeltas, SubscribeCustomData, SubscribeMarkPrices, SubscribeQuotes,
                SubscribeTrades,
            },
            unsubscribe::{UnsubscribeCustomData, UnsubscribeQuotes, UnsubscribeTrades},
        },
    },
    testing::wait_until_async,
};
use nautilus_core::{Params, UUID4, UnixNanos};
use nautilus_model::{
    data::{CustomData, Data, DataType},
    enums::BookType,
    identifiers::InstrumentId,
};
use nautilus_network::http::HttpClient;
use rstest::rstest;
use serde_json::json;

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

fn json_response(body: &serde_json::Value) -> Response {
    (
        StatusCode::OK,
        [("content-type", "application/json")],
        body.to_string(),
    )
        .into_response()
}

async fn handle_ws(ws: axum::extract::WebSocketUpgrade) -> Response {
    ws.on_upgrade(handle_ws_connection)
}

async fn handle_ws_connection(mut socket: WebSocket) {
    while let Some(Ok(msg)) = socket.recv().await {
        if let Message::Text(text) = msg
            && let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&text)
        {
            let method = parsed.get("method").and_then(|m| m.as_str());
            let id = parsed.get("id").and_then(|v| v.as_u64()).unwrap_or(1);

            if method == Some("SUBSCRIBE") {
                let resp = json!({"result": null, "id": id});
                let _result = socket.send(Message::Text(resp.to_string().into())).await;

                if let Some(params) = parsed.get("params").and_then(|p| p.as_array()) {
                    for param in params {
                        if let Some(stream) = param.as_str() {
                            if stream.contains("@aggTrade") {
                                let trade = json!({
                                    "e": "aggTrade",
                                    "E": 1700000000000_i64,
                                    "s": "BTCUSDT",
                                    "a": 1,
                                    "p": "50000.00",
                                    "q": "0.001",
                                    "f": 1,
                                    "l": 1,
                                    "T": 1700000000000_i64,
                                    "m": false
                                });
                                tokio::time::sleep(Duration::from_millis(50)).await;
                                let _result =
                                    socket.send(Message::Text(trade.to_string().into())).await;
                            } else if stream.contains("@bookTicker") {
                                let quote = json!({
                                    "e": "bookTicker",
                                    "u": 12345,
                                    "E": 1700000000000_i64,
                                    "T": 1700000000000_i64,
                                    "s": "BTCUSDT",
                                    "b": "50000.00",
                                    "B": "1.000",
                                    "a": "50001.00",
                                    "A": "0.500"
                                });
                                tokio::time::sleep(Duration::from_millis(50)).await;
                                let _result =
                                    socket.send(Message::Text(quote.to_string().into())).await;
                            } else if stream.contains("@depth") {
                                let depth_update = json!({
                                    "e": "depthUpdate",
                                    "E": 1700000000000_i64,
                                    "T": 1700000000000_i64,
                                    "s": "BTCUSDT",
                                    "U": 1027024,
                                    "u": 1027025,
                                    "pu": 1027023,
                                    "b": [["50000.00", "1.000"], ["49999.00", "2.000"]],
                                    "a": [["50001.00", "0.500"], ["50002.00", "1.500"]]
                                });
                                tokio::time::sleep(Duration::from_millis(50)).await;
                                let _result = socket
                                    .send(Message::Text(depth_update.to_string().into()))
                                    .await;
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
                            } else if stream.contains("@forceOrder")
                                || stream.contains("!forceOrder@arr")
                            {
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
                    }
                }
            } else if method == Some("UNSUBSCRIBE") {
                let resp = json!({"result": null, "id": id});
                let _result = socket.send(Message::Text(resp.to_string().into())).await;
            }
        }
    }
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

    (
        StatusCode::BAD_REQUEST,
        [("content-type", "application/json")],
        json!({"code": -1102, "msg": "Unexpected open interest history params"}).to_string(),
    )
        .into_response()
}

fn create_data_test_router() -> Router {
    Router::new()
        .route("/fapi/v1/ping", get(|| async { json_response(&json!({})) }))
        .route(
            "/fapi/v1/exchangeInfo",
            get(|| async {
                json_response(&json!({
                    "timezone": "UTC",
                    "serverTime": 1700000000000_i64,
                    "rateLimits": [],
                    "exchangeFilters": [],
                    "symbols": [{
                        "symbol": "BTCUSDT",
                        "pair": "BTCUSDT",
                        "contractType": "PERPETUAL",
                        "deliveryDate": 4133404800000_i64,
                        "onboardDate": 1569398400000_i64,
                        "status": "TRADING",
                        "baseAsset": "BTC",
                        "quoteAsset": "USDT",
                        "marginAsset": "USDT",
                        "pricePrecision": 2,
                        "quantityPrecision": 3,
                        "baseAssetPrecision": 8,
                        "quotePrecision": 8,
                        "maintMarginPercent": "2.5000",
                        "requiredMarginPercent": "5.0000",
                        "underlyingType": "COIN",
                        "settlePlan": 0,
                        "triggerProtect": "0.0500",
                        "filters": [
                            {"filterType": "PRICE_FILTER", "minPrice": "0.10", "maxPrice": "1000000", "tickSize": "0.10"},
                            {"filterType": "LOT_SIZE", "minQty": "0.001", "maxQty": "1000", "stepSize": "0.001"},
                            {"filterType": "MIN_NOTIONAL", "notional": "5"}
                        ],
                        "orderTypes": ["LIMIT", "MARKET", "STOP", "STOP_MARKET", "TAKE_PROFIT", "TAKE_PROFIT_MARKET", "TRAILING_STOP_MARKET"],
                        "timeInForce": ["GTC", "IOC", "FOK", "GTD"]
                    }]
                }))
            }),
        )
        .route(
            "/fapi/v1/depth",
            get(|| async {
                json_response(&json!({
                    "lastUpdateId": 1027024,
                    "E": 1700000000000_i64,
                    "T": 1700000000000_i64,
                    "bids": [["50000.00", "1.000"], ["49999.00", "2.000"]],
                    "asks": [["50001.00", "0.500"], ["50002.00", "1.500"]]
                }))
            }),
        )
        .route("/fapi/v1/openInterest", get(handle_open_interest))
        .route("/dapi/v1/openInterest", get(handle_open_interest_coinm))
        .route("/futures/data/openInterestHist", get(handle_open_interest_hist))
        .route("/ws", get(handle_ws))
}

async fn start_data_test_server() -> SocketAddr {
    let router = create_data_test_router();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        axum::serve(listener, router.into_make_service())
            .await
            .unwrap();
    });

    let health_url = format!("http://{addr}/fapi/v1/ping");
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

    addr
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
        product_types: vec![BinanceProductType::UsdM],
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
        product_types: vec![product_type],
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
                            && payload.open_interest == "12345.678"
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
                    && payload.points[0].sum_open_interest == "100.0"
                    && payload.points[1].sum_open_interest_value == "1005.0"
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
                            && payload.open_interest == "987.654"
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
                    && payload.points[0].sum_open_interest == "200.0"
                    && payload.points[1].sum_open_interest_value == "1510.0"
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
            let found = rx.try_recv().is_ok_and(|e| matches!(e, DataEvent::Data(_)));
            async move { found }
        },
        Duration::from_secs(5),
    )
    .await;
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
