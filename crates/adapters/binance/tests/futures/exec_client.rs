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

use std::{cell::RefCell, collections::HashMap, net::SocketAddr, rc::Rc, time::Duration};

use axum::{
    Router,
    extract::ws::{Message, WebSocket},
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
        execution::{CancelAllOrders, SubmitOrder},
    },
    testing::wait_until_async,
};
use nautilus_core::UnixNanos;
use nautilus_live::ExecutionClientCore;
use nautilus_model::{
    accounts::{AccountAny, MarginAccount},
    enums::{AccountType, OmsType, OrderSide, TimeInForce},
    events::{AccountState, OrderEventAny},
    identifiers::{AccountId, ClientId, ClientOrderId, InstrumentId, StrategyId, TraderId, Venue},
    orders::{LimitOrder, Order, OrderAny},
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

    client.submit_order(&submit_cmd).unwrap();

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
    let result = client.cancel_all_orders(&cancel_all_cmd);
    assert!(result.is_ok());
}
