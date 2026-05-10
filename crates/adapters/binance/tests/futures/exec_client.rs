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

//! Integration tests for the Binance Futures execution client.

use std::{
    cell::RefCell, collections::HashMap, net::SocketAddr, rc::Rc, sync::Arc, time::Duration,
};

use axum::{
    Router,
    extract::{
        Query,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{delete, get, post},
};
use nautilus_binance::{
    config::BinanceExecClientConfig, futures::execution::BinanceFuturesExecutionClient,
};
use nautilus_common::{
    cache::Cache,
    clients::ExecutionClient,
    live::runner::set_exec_event_sender,
    messages::{
        ExecutionEvent,
        execution::{CancelAllOrders, CancelOrder, ModifyOrder, QueryAccount, SubmitOrder},
    },
    testing::wait_until_async,
};
use nautilus_core::{Params, UnixNanos};
use nautilus_live::ExecutionClientCore;
use nautilus_model::{
    accounts::{AccountAny, MarginAccount},
    enums::{AccountType, OmsType, OrderSide, TimeInForce, TrailingOffsetType, TriggerType},
    events::{AccountState, OrderEventAny},
    identifiers::{
        AccountId, ClientId, ClientOrderId, InstrumentId, StrategyId, TraderId, Venue, VenueOrderId,
    },
    orders::{LimitOrder, Order, OrderAny, TrailingStopMarketOrder},
    types::{AccountBalance, Money, Price, Quantity},
};
use nautilus_network::http::HttpClient;
use rstest::rstest;
use serde_json::json;

fn has_auth_headers(headers: &HeaderMap) -> bool {
    headers.contains_key("x-mbx-apikey")
}

fn json_response(body: &serde_json::Value) -> Response {
    (
        StatusCode::OK,
        [("content-type", "application/json")],
        body.to_string(),
    )
        .into_response()
}

fn unauthorized_response() -> Response {
    (
        StatusCode::UNAUTHORIZED,
        [("content-type", "application/json")],
        json!({"code": -2015, "msg": "Invalid API-key"}).to_string(),
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

async fn handle_ws(ws: axum::extract::WebSocketUpgrade) -> Response {
    ws.on_upgrade(handle_ws_connection)
}

async fn handle_ws_connection(mut socket: WebSocket) {
    while let Some(Ok(msg)) = socket.recv().await {
        if let Message::Text(text) = msg
            && let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&text)
            && parsed.get("method").and_then(|m| m.as_str()) == Some("SUBSCRIBE")
        {
            let id = parsed.get("id").and_then(|v| v.as_u64()).unwrap_or(1);
            let resp = json!({"result": null, "id": id});
            let _ = socket.send(Message::Text(resp.to_string().into())).await;
        }
    }
}

async fn handle_ws_trading(ws: WebSocketUpgrade) -> Response {
    ws.on_upgrade(handle_ws_trading_connection)
}

async fn handle_ws_trading_connection(mut socket: WebSocket) {
    while let Some(Ok(msg)) = socket.recv().await {
        let Message::Text(text) = msg else {
            continue;
        };

        let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&text) else {
            continue;
        };

        let request_id = parsed.get("id").and_then(|v| v.as_str()).unwrap_or("");
        let method = parsed.get("method").and_then(|v| v.as_str());

        let response = match method {
            Some("order.place") => {
                let mut order = load_fixture("order_response.json");
                order["clientOrderId"] = parsed
                    .get("params")
                    .and_then(|params| params.get("newClientOrderId"))
                    .cloned()
                    .unwrap_or_else(|| json!("testOrder123"));
                json!({
                    "id": request_id,
                    "status": 200,
                    "result": order,
                    "rateLimits": []
                })
            }
            Some("order.cancel") => json!({
                "id": request_id,
                "status": 400,
                "error": {
                    "code": -2011,
                    "msg": "Unknown order sent"
                },
                "rateLimits": []
            }),
            Some("order.modify") => json!({
                "id": request_id,
                "status": 400,
                "error": {
                    "code": -4028,
                    "msg": "Price or quantity not changed"
                },
                "rateLimits": []
            }),
            _ => continue,
        };

        let _ = socket
            .send(Message::Text(response.to_string().into()))
            .await;
    }
}

fn create_exec_test_router() -> Router {
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
            "/fapi/v1/positionSide/dual",
            get(|headers: HeaderMap| async move {
                if !has_auth_headers(&headers) {
                    return unauthorized_response();
                }
                json_response(&json!({"dualSidePosition": false}))
            }),
        )
        .route(
            "/fapi/v1/listenKey",
            post(|headers: HeaderMap| async move {
                if !has_auth_headers(&headers) {
                    return unauthorized_response();
                }
                json_response(&json!({"listenKey": "test_listen_key"}))
            })
            .put(|headers: HeaderMap| async move {
                if !has_auth_headers(&headers) {
                    return unauthorized_response();
                }
                json_response(&json!({}))
            }),
        )
        .route(
            "/fapi/v2/account",
            get(|headers: HeaderMap| async move {
                if !has_auth_headers(&headers) {
                    return unauthorized_response();
                }
                json_response(&load_fixture("account_info_v2.json"))
            }),
        )
        .route(
            "/fapi/v2/positionRisk",
            get(|headers: HeaderMap| async move {
                if !has_auth_headers(&headers) {
                    return unauthorized_response();
                }
                json_response(&load_fixture("position_risk.json"))
            }),
        )
        .route(
            "/fapi/v1/openOrders",
            get(|headers: HeaderMap| async move {
                if !has_auth_headers(&headers) {
                    return unauthorized_response();
                }
                json_response(&json!([]))
            }),
        )
        .route(
            "/fapi/v1/order",
            post(|headers: HeaderMap| async move {
                if !has_auth_headers(&headers) {
                    return unauthorized_response();
                }
                json_response(&load_fixture("order_response.json"))
            })
            .delete(|headers: HeaderMap| async move {
                if !has_auth_headers(&headers) {
                    return unauthorized_response();
                }
                let mut resp = load_fixture("order_response.json");
                resp["status"] = json!("CANCELED");
                json_response(&resp)
            })
            .put(|headers: HeaderMap| async move {
                if !has_auth_headers(&headers) {
                    return unauthorized_response();
                }
                json_response(&load_fixture("order_response.json"))
            }),
        )
        .route(
            "/fapi/v1/allOpenOrders",
            delete(|headers: HeaderMap| async move {
                if !has_auth_headers(&headers) {
                    return unauthorized_response();
                }
                json_response(&json!({"code": 200, "msg": "The operation of cancel all open order is done."}))
            }),
        )
        .route(
            "/fapi/v1/algoOpenOrders",
            delete(|headers: HeaderMap| async move {
                if !has_auth_headers(&headers) {
                    return unauthorized_response();
                }
                json_response(&json!({"code": 200, "msg": "success"}))
            }),
        )
        .route(
            "/fapi/v1/allOrders",
            get(|headers: HeaderMap| async move {
                if !has_auth_headers(&headers) {
                    return unauthorized_response();
                }
                json_response(&json!([]))
            }),
        )
        .route(
            "/fapi/v1/userTrades",
            get(|headers: HeaderMap| async move {
                if !has_auth_headers(&headers) {
                    return unauthorized_response();
                }
                json_response(&json!([]))
            }),
        )
        .route("/ws", get(handle_ws))
        .route("/ws-fapi/v1", get(handle_ws_trading))
}

fn create_exec_test_router_with_algo_capture(
    captured_query: &Arc<std::sync::Mutex<Option<HashMap<String, String>>>>,
) -> Router {
    create_exec_test_router().route(
        "/fapi/v1/algoOrder",
        post({
            let captured_query = captured_query.clone();

            move |headers: HeaderMap, Query(query): Query<HashMap<String, String>>| async move {
                if !has_auth_headers(&headers) {
                    return unauthorized_response();
                }

                *captured_query.lock().unwrap() = Some(query);

                json_response(&json!({
                    "algoId": 12345,
                    "clientAlgoId": "test-algo-order-001",
                    "algoType": "CONDITIONAL",
                    "orderType": "TRAILING_STOP_MARKET",
                    "symbol": "BTCUSDT",
                    "side": "SELL",
                    "positionSide": "BOTH",
                    "timeInForce": "GTC",
                    "quantity": "0.001",
                    "algoStatus": "NEW",
                    "triggerPrice": "10000.00",
                    "price": "0",
                    "workingType": "MARK_PRICE",
                    "activatePrice": "10000.00",
                    "callbackRate": "0.25",
                    "reduceOnly": true,
                    "closePosition": false,
                    "priceProtect": false,
                    "selfTradePreventionMode": "NONE"
                }))
            }
        }),
    )
}

async fn start_exec_test_server() -> SocketAddr {
    let router = create_exec_test_router();
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

async fn start_exec_test_server_with_algo_capture() -> (
    SocketAddr,
    Arc<std::sync::Mutex<Option<HashMap<String, String>>>>,
) {
    let captured_query = Arc::new(std::sync::Mutex::new(None));
    let router = create_exec_test_router_with_algo_capture(&captured_query);
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

    (addr, captured_query)
}

fn create_exec_test_router_with_order_capture(
    captured_query: &Arc<std::sync::Mutex<Option<HashMap<String, String>>>>,
) -> Router {
    let captured = captured_query.clone();

    // Build a fresh router but override the /fapi/v1/order POST to capture query params
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
            "/fapi/v1/positionSide/dual",
            get(|headers: HeaderMap| async move {
                if !has_auth_headers(&headers) {
                    return unauthorized_response();
                }
                json_response(&json!({"dualSidePosition": false}))
            }),
        )
        .route(
            "/fapi/v1/listenKey",
            post(|headers: HeaderMap| async move {
                if !has_auth_headers(&headers) {
                    return unauthorized_response();
                }
                json_response(&json!({"listenKey": "test_listen_key"}))
            })
            .put(|headers: HeaderMap| async move {
                if !has_auth_headers(&headers) {
                    return unauthorized_response();
                }
                json_response(&json!({}))
            }),
        )
        .route(
            "/fapi/v2/account",
            get(|headers: HeaderMap| async move {
                if !has_auth_headers(&headers) {
                    return unauthorized_response();
                }
                json_response(&load_fixture("account_info_v2.json"))
            }),
        )
        .route(
            "/fapi/v2/positionRisk",
            get(|headers: HeaderMap| async move {
                if !has_auth_headers(&headers) {
                    return unauthorized_response();
                }
                json_response(&load_fixture("position_risk.json"))
            }),
        )
        .route(
            "/fapi/v1/openOrders",
            get(|headers: HeaderMap| async move {
                if !has_auth_headers(&headers) {
                    return unauthorized_response();
                }
                json_response(&json!([]))
            }),
        )
        .route(
            "/fapi/v1/order",
            post({
                move |headers: HeaderMap, Query(query): Query<HashMap<String, String>>| {
                    let captured = captured.clone();
                    async move {
                        if !has_auth_headers(&headers) {
                            return unauthorized_response();
                        }
                        *captured.lock().unwrap() = Some(query);
                        json_response(&load_fixture("order_response.json"))
                    }
                }
            }),
        )
        .route("/ws", get(handle_ws))
}

async fn start_exec_test_server_with_order_capture() -> (
    SocketAddr,
    Arc<std::sync::Mutex<Option<HashMap<String, String>>>>,
) {
    let captured_query = Arc::new(std::sync::Mutex::new(None));
    let router = create_exec_test_router_with_order_capture(&captured_query);
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

    (addr, captured_query)
}

fn create_test_execution_client(
    base_url_http: String,
    base_url_ws: String,
) -> (
    BinanceFuturesExecutionClient,
    tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
    Rc<RefCell<Cache>>,
) {
    let trader_id = TraderId::from("TESTER-001");
    let account_id = AccountId::from("BINANCE-001");
    let client_id = ClientId::from("BINANCE");

    let cache = Rc::new(RefCell::new(Cache::default()));

    let core = ExecutionClientCore::new(
        trader_id,
        client_id,
        Venue::from("BINANCE"),
        OmsType::Hedging,
        account_id,
        AccountType::Margin,
        None,
        cache.clone(),
    );

    let config = BinanceExecClientConfig {
        trader_id,
        account_id,
        base_url_http: Some(base_url_http),
        base_url_ws: Some(base_url_ws),
        use_ws_trading: false,
        api_key: Some("test_api_key".to_string()),
        api_secret: Some("test_api_secret".to_string()),
        ..Default::default()
    };

    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    set_exec_event_sender(tx);

    let client = BinanceFuturesExecutionClient::new(core, config).unwrap();

    (client, rx, cache)
}

fn add_test_account_to_cache(cache: &Rc<RefCell<Cache>>, account_id: AccountId) {
    let account_state = AccountState::new(
        account_id,
        AccountType::Margin,
        vec![AccountBalance::new(
            Money::from("100.0 USDT"),
            Money::from("0 USDT"),
            Money::from("100.0 USDT"),
        )],
        vec![],
        true,
        nautilus_core::UUID4::new(),
        UnixNanos::default(),
        UnixNanos::default(),
        None,
    );

    let account = AccountAny::Margin(MarginAccount::new(account_state, true));
    cache.borrow_mut().add_account(account).unwrap();
}

#[rstest]
#[tokio::test]
async fn test_client_creation() {
    let addr = start_exec_test_server().await;
    let base_url_http = format!("http://{addr}");
    let base_url_ws = format!("ws://{addr}/ws");

    let (client, _rx, _cache) = create_test_execution_client(base_url_http, base_url_ws);

    assert_eq!(client.client_id(), ClientId::from("BINANCE"));
    assert_eq!(client.venue(), Venue::from("BINANCE"));
    assert_eq!(client.oms_type(), OmsType::Hedging);
    assert!(!client.is_connected());
}

#[rstest]
#[tokio::test]
async fn test_connect_loads_instruments_and_account() {
    let addr = start_exec_test_server().await;
    let base_url_http = format!("http://{addr}");
    let base_url_ws = format!("ws://{addr}/ws");

    let (mut client, _rx, cache) = create_test_execution_client(base_url_http, base_url_ws);
    add_test_account_to_cache(&cache, AccountId::from("BINANCE-001"));

    client.connect().await.unwrap();

    assert!(client.is_connected());
}

#[rstest]
#[tokio::test]
async fn test_disconnect_sets_state() {
    let addr = start_exec_test_server().await;
    let base_url_http = format!("http://{addr}");
    let base_url_ws = format!("ws://{addr}/ws");

    let (mut client, _rx, cache) = create_test_execution_client(base_url_http, base_url_ws);
    add_test_account_to_cache(&cache, AccountId::from("BINANCE-001"));

    client.connect().await.unwrap();
    assert!(client.is_connected());

    client.disconnect().await.unwrap();
    assert!(!client.is_connected());
}

#[rstest]
#[tokio::test]
async fn test_submit_order_generates_submitted_event() {
    let addr = start_exec_test_server().await;
    let base_url_http = format!("http://{addr}");
    let base_url_ws = format!("ws://{addr}/ws");

    let (mut client, mut rx, cache) = create_test_execution_client(base_url_http, base_url_ws);
    add_test_account_to_cache(&cache, AccountId::from("BINANCE-001"));

    client.start().unwrap();
    client.connect().await.unwrap();

    let instrument_id = InstrumentId::from("BTCUSDT-PERP.BINANCE");
    let client_order_id = ClientOrderId::new("test-order-001");
    let trader_id = TraderId::from("TESTER-001");
    let strategy_id = StrategyId::from("TEST-STRATEGY");

    let order = LimitOrder::new(
        trader_id,
        strategy_id,
        instrument_id,
        client_order_id,
        OrderSide::Buy,
        Quantity::from("0.001"),
        Price::from("50000.00"),
        TimeInForce::Gtc,
        None,  // expire_time
        true,  // post_only
        false, // reduce_only
        false, // quote_quantity
        None,  // display_qty
        None,  // emulation_trigger
        None,  // trigger_instrument_id
        None,  // contingency_type
        None,  // order_list_id
        None,  // linked_order_ids
        None,  // parent_order_id
        None,  // exec_algorithm_id
        None,  // exec_algorithm_params
        None,  // exec_spawn_id
        None,  // tags
        nautilus_core::UUID4::new(),
        UnixNanos::default(),
    );

    let order_any = OrderAny::Limit(order);

    cache
        .borrow_mut()
        .add_order(order_any.clone(), None, None, false)
        .unwrap();

    let submit_cmd = SubmitOrder::new(
        trader_id,
        Some(ClientId::from("BINANCE")),
        strategy_id,
        instrument_id,
        order_any.client_order_id(),
        order_any.init_event().clone(),
        None, // exec_algorithm_id
        None, // position_id
        None, // params
        nautilus_core::UUID4::new(),
        UnixNanos::default(),
    );

    client.submit_order(submit_cmd).unwrap();

    // Futures HTTP submit emits OrderSubmitted synchronously;
    // OrderAccepted arrives via the WS user data stream
    wait_until_async(
        || {
            let found = rx
                .try_recv()
                .is_ok_and(|e| matches!(e, ExecutionEvent::Order(OrderEventAny::Submitted(_))));
            async move { found }
        },
        Duration::from_secs(5),
    )
    .await;
}

#[rstest]
#[tokio::test]
async fn test_submit_trailing_stop_order_uses_activate_price_and_precise_callback_rate() {
    let (addr, captured_query) = start_exec_test_server_with_algo_capture().await;
    let base_url_http = format!("http://{addr}");
    let base_url_ws = format!("ws://{addr}/ws");

    let (mut client, _rx, cache) = create_test_execution_client(base_url_http, base_url_ws);
    add_test_account_to_cache(&cache, AccountId::from("BINANCE-001"));

    client.start().unwrap();
    client.connect().await.unwrap();

    let instrument_id = InstrumentId::from("BTCUSDT-PERP.BINANCE");
    let client_order_id = ClientOrderId::new("trailing-stop-test-001");
    let trader_id = TraderId::from("TESTER-001");
    let strategy_id = StrategyId::from("TEST-STRATEGY");

    let mut order = TrailingStopMarketOrder::new(
        trader_id,
        strategy_id,
        instrument_id,
        client_order_id,
        OrderSide::Sell,
        Quantity::from("0.001"),
        Price::from("10000.00"),
        TriggerType::MarkPrice,
        rust_decimal::Decimal::from(25),
        TrailingOffsetType::BasisPoints,
        TimeInForce::Gtc,
        None,
        true,
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
        nautilus_core::UUID4::new(),
        UnixNanos::default(),
    );
    order.activation_price = Some(Price::from("10000.00"));

    let order_any = OrderAny::TrailingStopMarket(order);
    cache
        .borrow_mut()
        .add_order(order_any.clone(), None, None, false)
        .unwrap();

    let submit_cmd = SubmitOrder::new(
        trader_id,
        Some(ClientId::from("BINANCE")),
        strategy_id,
        instrument_id,
        client_order_id,
        order_any.init_event().clone(),
        None,
        None,
        None,
        nautilus_core::UUID4::new(),
        UnixNanos::default(),
    );

    client.submit_order(submit_cmd).unwrap();

    wait_until_async(
        || {
            let captured_query = captured_query.clone();

            async move { captured_query.lock().unwrap().is_some() }
        },
        Duration::from_secs(5),
    )
    .await;

    let query = captured_query.lock().unwrap().clone().unwrap();
    assert_eq!(query.get("type"), Some(&"TRAILING_STOP_MARKET".to_string()));
    assert_eq!(query.get("activatePrice"), Some(&"10000.00".to_string()));
    assert_eq!(query.get("callbackRate"), Some(&"0.25".to_string()));
    assert!(!query.contains_key("activationPrice"));
}

#[rstest]
#[tokio::test]
async fn test_cancel_all_orders_completes() {
    let addr = start_exec_test_server().await;
    let base_url_http = format!("http://{addr}");
    let base_url_ws = format!("ws://{addr}/ws");

    let (mut client, _rx, cache) = create_test_execution_client(base_url_http, base_url_ws);
    add_test_account_to_cache(&cache, AccountId::from("BINANCE-001"));

    client.start().unwrap();
    client.connect().await.unwrap();

    let instrument_id = InstrumentId::from("BTCUSDT-PERP.BINANCE");

    let cancel_all_cmd = CancelAllOrders::new(
        TraderId::from("TESTER-001"),
        Some(ClientId::from("BINANCE")),
        StrategyId::from("TEST-STRATEGY"),
        instrument_id,
        OrderSide::NoOrderSide,
        nautilus_core::UUID4::new(),
        UnixNanos::default(),
        None,
    );

    // Futures cancel_all returns success code via HTTP; cancel events arrive through WS
    let result = client.cancel_all_orders(cancel_all_cmd);
    assert!(result.is_ok());
}

#[rstest]
#[tokio::test]
async fn test_cancel_order_completes() {
    let addr = start_exec_test_server().await;
    let base_url_http = format!("http://{addr}");
    let base_url_ws = format!("ws://{addr}/ws");

    let (mut client, _rx, cache) = create_test_execution_client(base_url_http, base_url_ws);
    add_test_account_to_cache(&cache, AccountId::from("BINANCE-001"));

    client.start().unwrap();
    client.connect().await.unwrap();

    let instrument_id = InstrumentId::from("BTCUSDT-PERP.BINANCE");
    let client_order_id = ClientOrderId::new("cancel-test-001");
    let trader_id = TraderId::from("TESTER-001");
    let strategy_id = StrategyId::from("TEST-STRATEGY");

    let order = LimitOrder::new(
        trader_id,
        strategy_id,
        instrument_id,
        client_order_id,
        OrderSide::Buy,
        Quantity::from("0.001"),
        Price::from("50000.00"),
        TimeInForce::Gtc,
        None,
        true,
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
        nautilus_core::UUID4::new(),
        UnixNanos::default(),
    );

    let order_any = OrderAny::Limit(order);
    cache
        .borrow_mut()
        .add_order(order_any, None, None, false)
        .unwrap();

    let cancel_cmd = CancelOrder::new(
        trader_id,
        Some(ClientId::from("BINANCE")),
        strategy_id,
        instrument_id,
        client_order_id,
        Some(VenueOrderId::from("12345")),
        nautilus_core::UUID4::new(),
        UnixNanos::default(),
        None,
    );

    // Futures cancel queues an async HTTP task. The actual OrderCanceled event
    // arrives via the WS user data stream, which this mock does not simulate.
    // We verify the command is accepted and the HTTP request completes without error.
    let result = client.cancel_order(cancel_cmd);
    assert!(result.is_ok());
}

#[rstest]
#[tokio::test]
async fn test_modify_order_completes() {
    let addr = start_exec_test_server().await;
    let base_url_http = format!("http://{addr}");
    let base_url_ws = format!("ws://{addr}/ws");

    let (mut client, mut rx, cache) = create_test_execution_client(base_url_http, base_url_ws);
    add_test_account_to_cache(&cache, AccountId::from("BINANCE-001"));

    client.start().unwrap();
    client.connect().await.unwrap();

    let instrument_id = InstrumentId::from("BTCUSDT-PERP.BINANCE");
    let client_order_id = ClientOrderId::new("modify-test-001");
    let trader_id = TraderId::from("TESTER-001");
    let strategy_id = StrategyId::from("TEST-STRATEGY");

    let order = LimitOrder::new(
        trader_id,
        strategy_id,
        instrument_id,
        client_order_id,
        OrderSide::Buy,
        Quantity::from("0.001"),
        Price::from("50000.00"),
        TimeInForce::Gtc,
        None,
        true,
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
        nautilus_core::UUID4::new(),
        UnixNanos::default(),
    );

    let order_any = OrderAny::Limit(order);
    cache
        .borrow_mut()
        .add_order(order_any, None, None, false)
        .unwrap();

    let modify_cmd = ModifyOrder::new(
        trader_id,
        Some(ClientId::from("BINANCE")),
        strategy_id,
        instrument_id,
        client_order_id,
        Some(VenueOrderId::from("12345")),
        Some(Quantity::from("0.002")),
        Some(Price::from("51000.00")),
        None,
        nautilus_core::UUID4::new(),
        UnixNanos::default(),
        None,
    );

    client.modify_order(modify_cmd).unwrap();

    // Futures modify_order HTTP path emits OrderUpdated on success
    wait_until_async(
        || {
            let found = rx
                .try_recv()
                .is_ok_and(|e| matches!(e, ExecutionEvent::Order(OrderEventAny::Updated(_))));
            async move { found }
        },
        Duration::from_secs(5),
    )
    .await;
}

#[rstest]
#[tokio::test]
async fn test_cancel_order_ws_rejection_emits_cancel_rejected() {
    let addr = start_exec_test_server().await;
    let base_url_http = format!("http://{addr}");
    let base_url_ws = format!("ws://{addr}/ws");
    let base_url_ws_trading = format!("ws://{addr}/ws-fapi/v1");

    let (mut client, mut rx, cache) = create_test_execution_client_with_ws_trading(
        base_url_http,
        base_url_ws,
        base_url_ws_trading,
    );
    add_test_account_to_cache(&cache, AccountId::from("BINANCE-001"));

    client.start().unwrap();
    client.connect().await.unwrap();

    let instrument_id = InstrumentId::from("BTCUSDT-PERP.BINANCE");
    let client_order_id = ClientOrderId::new("cancel-ws-test-001");
    let trader_id = TraderId::from("TESTER-001");
    let strategy_id = StrategyId::from("TEST-STRATEGY");

    let order = LimitOrder::new(
        trader_id,
        strategy_id,
        instrument_id,
        client_order_id,
        OrderSide::Buy,
        Quantity::from("0.001"),
        Price::from("50000.00"),
        TimeInForce::Gtc,
        None,
        true,
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
        nautilus_core::UUID4::new(),
        UnixNanos::default(),
    );

    let order_any = OrderAny::Limit(order);
    cache
        .borrow_mut()
        .add_order(order_any.clone(), None, None, false)
        .unwrap();

    let submit_cmd = SubmitOrder::new(
        trader_id,
        Some(ClientId::from("BINANCE")),
        strategy_id,
        instrument_id,
        client_order_id,
        order_any.init_event().clone(),
        None,
        None,
        None,
        nautilus_core::UUID4::new(),
        UnixNanos::default(),
    );

    client.submit_order(submit_cmd).unwrap();

    let cancel_cmd = CancelOrder::new(
        trader_id,
        Some(ClientId::from("BINANCE")),
        strategy_id,
        instrument_id,
        client_order_id,
        Some(VenueOrderId::from("12345")),
        nautilus_core::UUID4::new(),
        UnixNanos::default(),
        None,
    );

    client.cancel_order(cancel_cmd).unwrap();

    match recv_until(&mut rx, |event| {
        matches!(
            event,
            ExecutionEvent::Order(OrderEventAny::CancelRejected(_))
        )
    })
    .await
    {
        ExecutionEvent::Order(OrderEventAny::CancelRejected(event)) => {
            assert_eq!(event.client_order_id, client_order_id);
            assert!(event.reason.as_str().contains("code=-2011"));
        }
        other => panic!("Expected CancelRejected event, was {other:?}"),
    }
}

#[rstest]
#[tokio::test]
async fn test_modify_order_ws_rejection_emits_modify_rejected() {
    let addr = start_exec_test_server().await;
    let base_url_http = format!("http://{addr}");
    let base_url_ws = format!("ws://{addr}/ws");
    let base_url_ws_trading = format!("ws://{addr}/ws-fapi/v1");

    let (mut client, mut rx, cache) = create_test_execution_client_with_ws_trading(
        base_url_http,
        base_url_ws,
        base_url_ws_trading,
    );
    add_test_account_to_cache(&cache, AccountId::from("BINANCE-001"));

    client.start().unwrap();
    client.connect().await.unwrap();

    let instrument_id = InstrumentId::from("BTCUSDT-PERP.BINANCE");
    let client_order_id = ClientOrderId::new("modify-ws-test-001");
    let trader_id = TraderId::from("TESTER-001");
    let strategy_id = StrategyId::from("TEST-STRATEGY");

    let order = LimitOrder::new(
        trader_id,
        strategy_id,
        instrument_id,
        client_order_id,
        OrderSide::Buy,
        Quantity::from("0.001"),
        Price::from("50000.00"),
        TimeInForce::Gtc,
        None,
        true,
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
        nautilus_core::UUID4::new(),
        UnixNanos::default(),
    );

    let order_any = OrderAny::Limit(order);
    cache
        .borrow_mut()
        .add_order(order_any.clone(), None, None, false)
        .unwrap();

    let submit_cmd = SubmitOrder::new(
        trader_id,
        Some(ClientId::from("BINANCE")),
        strategy_id,
        instrument_id,
        client_order_id,
        order_any.init_event().clone(),
        None,
        None,
        None,
        nautilus_core::UUID4::new(),
        UnixNanos::default(),
    );

    client.submit_order(submit_cmd).unwrap();

    let modify_cmd = ModifyOrder::new(
        trader_id,
        Some(ClientId::from("BINANCE")),
        strategy_id,
        instrument_id,
        client_order_id,
        Some(VenueOrderId::from("12345")),
        Some(Quantity::from("0.002")),
        Some(Price::from("51000.00")),
        None,
        nautilus_core::UUID4::new(),
        UnixNanos::default(),
        None,
    );

    client.modify_order(modify_cmd).unwrap();

    match recv_until(&mut rx, |event| {
        matches!(
            event,
            ExecutionEvent::Order(OrderEventAny::ModifyRejected(_))
        )
    })
    .await
    {
        ExecutionEvent::Order(OrderEventAny::ModifyRejected(event)) => {
            assert_eq!(event.client_order_id, client_order_id);
            assert!(event.reason.as_str().contains("code=-4028"));
        }
        other => panic!("Expected ModifyRejected event, was {other:?}"),
    }
}

#[rstest]
#[tokio::test]
async fn test_connect_disconnect_reconnect() {
    let addr = start_exec_test_server().await;
    let base_url_http = format!("http://{addr}");
    let base_url_ws = format!("ws://{addr}/ws");

    let (mut client, _rx, cache) = create_test_execution_client(base_url_http, base_url_ws);
    add_test_account_to_cache(&cache, AccountId::from("BINANCE-001"));

    client.connect().await.unwrap();
    assert!(client.is_connected());

    client.disconnect().await.unwrap();
    assert!(!client.is_connected());

    // Reconnect
    client.connect().await.unwrap();
    assert!(client.is_connected());
}

#[rstest]
#[tokio::test]
async fn test_submit_order_with_price_match_sends_price_match_and_omits_price() {
    let (addr, captured_query) = start_exec_test_server_with_order_capture().await;
    let base_url_http = format!("http://{addr}");
    let base_url_ws = format!("ws://{addr}/ws");

    let (mut client, mut rx, cache) = create_test_execution_client(base_url_http, base_url_ws);
    add_test_account_to_cache(&cache, AccountId::from("BINANCE-001"));

    client.start().unwrap();
    client.connect().await.unwrap();

    let instrument_id = InstrumentId::from("BTCUSDT-PERP.BINANCE");
    let client_order_id = ClientOrderId::new("price-match-test-001");
    let trader_id = TraderId::from("TESTER-001");
    let strategy_id = StrategyId::from("TEST-STRATEGY");

    let order = LimitOrder::new(
        trader_id,
        strategy_id,
        instrument_id,
        client_order_id,
        OrderSide::Buy,
        Quantity::from("0.001"),
        Price::from("50000.00"),
        TimeInForce::Gtc,
        None,  // expire_time
        false, // post_only (must be false for price_match)
        false, // reduce_only
        false, // quote_quantity
        None,  // display_qty
        None,  // emulation_trigger
        None,  // trigger_instrument_id
        None,  // contingency_type
        None,  // order_list_id
        None,  // linked_order_ids
        None,  // parent_order_id
        None,  // exec_algorithm_id
        None,  // exec_algorithm_params
        None,  // exec_spawn_id
        None,  // tags
        nautilus_core::UUID4::new(),
        UnixNanos::default(),
    );

    let order_any = OrderAny::Limit(order);
    cache
        .borrow_mut()
        .add_order(order_any.clone(), None, None, false)
        .unwrap();

    let mut params = Params::new();
    params.insert(
        "price_match".to_string(),
        serde_json::Value::String("OPPONENT_5".to_string()),
    );

    let submit_cmd = SubmitOrder::new(
        trader_id,
        Some(ClientId::from("BINANCE")),
        strategy_id,
        instrument_id,
        client_order_id,
        order_any.init_event().clone(),
        None, // exec_algorithm_id
        None, // position_id
        Some(params),
        nautilus_core::UUID4::new(),
        UnixNanos::default(),
    );

    client.submit_order(submit_cmd).unwrap();

    wait_until_async(
        || {
            let captured_query = captured_query.clone();
            async move { captured_query.lock().unwrap().is_some() }
        },
        Duration::from_secs(5),
    )
    .await;

    let query = captured_query.lock().unwrap().clone().unwrap();
    assert_eq!(
        query.get("priceMatch"),
        Some(&"OPPONENT_5".to_string()),
        "priceMatch should be OPPONENT_5"
    );
    assert!(
        !query.contains_key("price"),
        "price must be omitted when priceMatch is set"
    );
    assert_eq!(query.get("type"), Some(&"LIMIT".to_string()));
    assert_eq!(query.get("side"), Some(&"BUY".to_string()));
    assert_eq!(query.get("quantity"), Some(&"0.001".to_string()));

    // Drain the submitted event to confirm the order was processed
    wait_until_async(
        || {
            let found = rx
                .try_recv()
                .is_ok_and(|e| matches!(e, ExecutionEvent::Order(OrderEventAny::Submitted(_))));
            async move { found }
        },
        Duration::from_secs(5),
    )
    .await;
}

fn create_test_execution_client_with_ws_trading(
    base_url_http: String,
    base_url_ws: String,
    base_url_ws_trading: String,
) -> (
    BinanceFuturesExecutionClient,
    tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
    Rc<RefCell<Cache>>,
) {
    let trader_id = TraderId::from("TESTER-001");
    let account_id = AccountId::from("BINANCE-001");
    let client_id = ClientId::from("BINANCE");

    let cache = Rc::new(RefCell::new(Cache::default()));

    let core = ExecutionClientCore::new(
        trader_id,
        client_id,
        Venue::from("BINANCE"),
        OmsType::Hedging,
        account_id,
        AccountType::Margin,
        None,
        cache.clone(),
    );

    let config = BinanceExecClientConfig {
        trader_id,
        account_id,
        base_url_http: Some(base_url_http),
        base_url_ws: Some(base_url_ws),
        base_url_ws_trading: Some(base_url_ws_trading),
        use_ws_trading: true,
        api_key: Some("test_api_key".to_string()),
        api_secret: Some("test_api_secret".to_string()),
        ..Default::default()
    };

    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    set_exec_event_sender(tx);

    let client = BinanceFuturesExecutionClient::new(core, config).unwrap();

    (client, rx, cache)
}

async fn recv_until<F>(
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
    predicate: F,
) -> ExecutionEvent
where
    F: Fn(&ExecutionEvent) -> bool,
{
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let event = rx.recv().await.expect("Execution event channel closed");
            if predicate(&event) {
                return event;
            }
        }
    })
    .await
    .expect("Timed out waiting for matching execution event")
}

type WsInjector = Arc<tokio::sync::broadcast::Sender<String>>;

async fn handle_ws_injectable_connection(mut socket: WebSocket, injector: WsInjector) {
    let mut rx = injector.subscribe();

    loop {
        tokio::select! {
            msg = socket.recv() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&text)
                            && parsed.get("method").and_then(|m| m.as_str()) == Some("SUBSCRIBE")
                        {
                            let id = parsed.get("id").and_then(|v| v.as_u64()).unwrap_or(1);
                            let resp = json!({"result": null, "id": id});
                            let _ = socket.send(Message::Text(resp.to_string().into())).await;
                        }
                    }
                    None | Some(Err(_)) => break,
                    _ => {}
                }
            }
            Ok(injected) = rx.recv() => {
                let _ = socket.send(Message::Text(injected.into())).await;
            }
        }
    }
}

async fn start_injectable_test_server() -> (SocketAddr, WsInjector) {
    let (tx, _) = tokio::sync::broadcast::channel::<String>(16);
    let ws_injector: WsInjector = Arc::new(tx);

    let inj = ws_injector.clone();
    let injectable_ws = axum::routing::get(move |ws: axum::extract::WebSocketUpgrade| {
        let inj = inj.clone();
        async move { ws.on_upgrade(move |socket| handle_ws_injectable_connection(socket, inj)) }
    });
    let router = create_exec_test_router().route("/ws-inject", injectable_ws);

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

    (addr, ws_injector)
}

#[rstest]
#[tokio::test]
async fn test_order_trade_update_processed_with_default_precision_on_cache_miss() {
    let (addr, ws_injector) = start_injectable_test_server().await;
    let base_url_http = format!("http://{addr}");
    let base_url_ws = format!("ws://{addr}/ws-inject");

    let (mut client, mut rx, cache) = create_test_execution_client(base_url_http, base_url_ws);
    add_test_account_to_cache(&cache, AccountId::from("BINANCE-001"));

    client.start().unwrap();
    client.connect().await.unwrap();

    // Clear the instrument cache to simulate a cache miss
    let instruments = client.instruments_cache();
    instruments.clear();

    // Give the WS subscription time to establish
    tokio::time::sleep(Duration::from_millis(200)).await;

    // Inject an ORDER_TRADE_UPDATE with execution_type=TRADE for an untracked order.
    // Without the fix this would be silently dropped; with the fix it falls through
    // to the default-precision path and produces an OrderStatusReport.
    let order_update = json!({
        "e": "ORDER_TRADE_UPDATE",
        "T": 1568879465651_i64,
        "E": 1568879465651_i64,
        "o": {
            "s": "BTCUSDT",
            "c": "test-cache-miss",
            "S": "BUY",
            "o": "LIMIT",
            "f": "GTC",
            "q": "0.001",
            "p": "50000.00",
            "ap": "50000.00",
            "sp": "0",
            "x": "TRADE",
            "X": "PARTIALLY_FILLED",
            "i": 9999999,
            "l": "0.001",
            "z": "0.001",
            "L": "50000.00",
            "N": "USDT",
            "n": "0.01000000",
            "T": 1568879465651_i64,
            "t": 12345678,
            "b": "0",
            "a": "0",
            "m": true,
            "R": false,
            "wt": "CONTRACT_PRICE",
            "ot": "LIMIT",
            "ps": "LONG",
            "cp": false,
            "AP": "0",
            "cr": "0",
            "pP": false,
            "si": 0,
            "ss": 0,
            "rp": "0",
            "V": "EXPIRE_TAKER"
        }
    });
    ws_injector.send(order_update.to_string()).unwrap();

    // The untracked order path produces a FillReport then an OrderStatusReport.
    // wait_until_async panics on timeout, so reaching the end means success.
    wait_until_async(
        || {
            let found = rx
                .try_recv()
                .is_ok_and(|e| matches!(e, ExecutionEvent::Report(_)));
            async move { found }
        },
        Duration::from_secs(5),
    )
    .await;
}

#[rstest]
#[tokio::test]
async fn test_query_account_does_not_block_within_runtime() {
    let addr = start_exec_test_server().await;
    let base_url_http = format!("http://{addr}");
    let base_url_ws = format!("ws://{addr}/ws");

    let (mut client, mut rx, cache) = create_test_execution_client(base_url_http, base_url_ws);
    add_test_account_to_cache(&cache, AccountId::from("BINANCE-001"));

    client.start().unwrap();
    client.connect().await.unwrap();

    let cmd = QueryAccount::new(
        TraderId::from("TESTER-001"),
        Some(ClientId::from("BINANCE")),
        AccountId::from("BINANCE-001"),
        nautilus_core::UUID4::new(),
        UnixNanos::default(),
        None,
    );

    let result = client.query_account(cmd);
    assert!(result.is_ok());

    match recv_until(&mut rx, |event| matches!(event, ExecutionEvent::Account(_))).await {
        ExecutionEvent::Account(_) => {}
        other => panic!("Expected Account event, was {other:?}"),
    }
}
