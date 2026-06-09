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
    cell::RefCell,
    collections::HashMap,
    net::SocketAddr,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use axum::{
    Router,
    extract::{
        Query, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{delete, get, post},
};
use nautilus_binance::{
    common::{
        consts::{
            BINANCE_CLIENT_ID, BINANCE_NAUTILUS_FUTURES_BROKER_ID, BINANCE_STATUS_UNKNOWN_CODE,
            BINANCE_UNEXPECTED_RESPONSE_CODE, BINANCE_VENUE,
        },
        encoder::encode_broker_id,
        enums::BinanceProductType,
        parse::parse_usdm_instrument,
    },
    config::BinanceExecClientConfig,
    futures::{
        execution::BinanceFuturesExecutionClient, http::models::BinanceFuturesUsdExchangeInfo,
    },
};
use nautilus_common::{
    cache::Cache,
    clients::ExecutionClient,
    live::runner::set_exec_event_sender,
    messages::{
        ExecutionEvent,
        execution::{
            BatchCancelOrders, CancelAllOrders, CancelOrder, GenerateFillReports,
            GenerateOrderStatusReport, GenerateOrderStatusReports, GeneratePositionStatusReports,
            ModifyOrder, QueryAccount, QueryOrder, SubmitOrder,
        },
    },
    testing::wait_until_async,
};
use nautilus_core::{Params, UnixNanos};
use nautilus_live::ExecutionClientCore;
use nautilus_model::{
    accounts::{AccountAny, MarginAccount},
    enums::{AccountType, OmsType, OrderSide, TimeInForce, TrailingOffsetType, TriggerType},
    events::{AccountState, OrderEventAny},
    identifiers::{AccountId, ClientOrderId, InstrumentId, StrategyId, TraderId, VenueOrderId},
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

fn exchange_info_response() -> serde_json::Value {
    json!({
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
    })
}

#[derive(Clone, Copy)]
enum CommandResponse {
    Success,
    AmbiguousFailure,
    BatchPerOrderReject { code: i64, msg: &'static str },
    VenueReject { code: i64, msg: &'static str },
}

#[derive(Clone, Copy)]
struct CommandResponses {
    submit: CommandResponse,
    cancel: CommandResponse,
    modify: CommandResponse,
    batch_cancel: CommandResponse,
}

impl Default for CommandResponses {
    fn default() -> Self {
        Self {
            submit: CommandResponse::Success,
            cancel: CommandResponse::Success,
            modify: CommandResponse::Success,
            batch_cancel: CommandResponse::Success,
        }
    }
}

#[derive(Clone)]
struct CommandResponseState {
    responses: CommandResponses,
    request_count: Arc<AtomicUsize>,
    captured_queries: Option<CapturedQueries>,
    captured_ws_trading_messages: Option<CapturedWsTradingMessages>,
    report_fixture_mode: ReportFixtureMode,
    hedge_mode: bool,
}

#[derive(Clone)]
struct CapturedQuery {
    path: &'static str,
    query: HashMap<String, String>,
}

type CapturedQueries = Arc<std::sync::Mutex<Vec<CapturedQuery>>>;
type CapturedWsTradingMessages = Arc<std::sync::Mutex<Vec<serde_json::Value>>>;

#[derive(Clone, Copy)]
enum ReportFixtureMode {
    Empty,
    Populated,
}

fn record_query(state: &CommandResponseState, path: &'static str, query: HashMap<String, String>) {
    if let Some(captured_queries) = &state.captured_queries {
        captured_queries
            .lock()
            .unwrap()
            .push(CapturedQuery { path, query });
    }
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
            let _result = socket.send(Message::Text(resp.to_string().into())).await;
        }
    }
}

async fn handle_ws_trading(
    State(state): State<CommandResponseState>,
    ws: WebSocketUpgrade,
) -> Response {
    let captured_ws_trading_messages = state.captured_ws_trading_messages;
    ws.on_upgrade(move |socket| handle_ws_trading_connection(socket, captured_ws_trading_messages))
}

async fn handle_ws_trading_connection(
    mut socket: WebSocket,
    captured_ws_trading_messages: Option<CapturedWsTradingMessages>,
) {
    while let Some(Ok(msg)) = socket.recv().await {
        let Message::Text(text) = msg else {
            continue;
        };

        let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&text) else {
            continue;
        };

        let request_id = parsed.get("id").and_then(|v| v.as_str()).unwrap_or("");
        let method = parsed.get("method").and_then(|v| v.as_str());

        if matches!(method, Some("order.place" | "order.cancel"))
            && let Some(captured) = &captured_ws_trading_messages
        {
            captured.lock().unwrap().push(parsed.clone());
        }

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
            Some("order.cancel")
                if parsed
                    .get("params")
                    .and_then(|params| params.get("orderId"))
                    .is_none() =>
            {
                let mut order = load_fixture("order_response.json");
                order["status"] = json!("CANCELED");
                order["clientOrderId"] = parsed
                    .get("params")
                    .and_then(|params| params.get("origClientOrderId"))
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

        let _result = socket
            .send(Message::Text(response.to_string().into()))
            .await;
    }
}

fn create_exec_test_router() -> Router {
    create_exec_test_router_with_command_responses(CommandResponseState {
        responses: CommandResponses::default(),
        request_count: Arc::new(AtomicUsize::new(0)),
        captured_queries: None,
        captured_ws_trading_messages: None,
        report_fixture_mode: ReportFixtureMode::Empty,
        hedge_mode: false,
    })
}

fn create_exec_test_router_with_command_responses(state: CommandResponseState) -> Router {
    Router::new()
        .route("/fapi/v1/ping", get(|| async { json_response(&json!({})) }))
        .route(
            "/fapi/v1/exchangeInfo",
            get(|| async { json_response(&exchange_info_response()) }),
        )
        .route(
            "/fapi/v1/positionSide/dual",
            get(
                |State(state): State<CommandResponseState>, headers: HeaderMap| async move {
                    if !has_auth_headers(&headers) {
                        return unauthorized_response();
                    }
                    json_response(&json!({"dualSidePosition": state.hedge_mode}))
                },
            ),
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
        .route("/fapi/v2/positionRisk", get(handle_position_risk_query))
        .route("/fapi/v1/openOrders", get(handle_open_orders_query))
        .route(
            "/fapi/v1/order",
            post(handle_order_submit)
                .delete(handle_order_cancel)
                .put(handle_order_modify)
                .get(handle_order_query),
        )
        .route("/fapi/v1/batchOrders", delete(handle_batch_cancel))
        .route(
            "/fapi/v1/allOpenOrders",
            delete(|headers: HeaderMap| async move {
                if !has_auth_headers(&headers) {
                    return unauthorized_response();
                }
                json_response(
                    &json!({"code": 200, "msg": "The operation of cancel all open order is done."}),
                )
            }),
        )
        .route(
            "/fapi/v1/openAlgoOrders",
            get(handle_open_algo_orders_query),
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
        .route("/fapi/v1/allOrders", get(handle_all_orders_query))
        .route("/fapi/v1/userTrades", get(handle_user_trades_query))
        .route("/ws", get(handle_ws))
        .route("/ws-fapi/v1", get(handle_ws_trading))
        .with_state(state)
}

async fn handle_position_risk_query(
    State(state): State<CommandResponseState>,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
) -> Response {
    if !has_auth_headers(&headers) {
        return unauthorized_response();
    }
    record_query(&state, "positionRisk", query);
    json_response(&load_fixture("position_risk.json"))
}

async fn handle_open_orders_query(
    State(state): State<CommandResponseState>,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
) -> Response {
    if !has_auth_headers(&headers) {
        return unauthorized_response();
    }
    record_query(&state, "openOrders", query);
    match state.report_fixture_mode {
        ReportFixtureMode::Empty => json_response(&json!([])),
        ReportFixtureMode::Populated => {
            json_response(&json!([load_fixture("order_response.json")]))
        }
    }
}

async fn handle_open_algo_orders_query(
    State(state): State<CommandResponseState>,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
) -> Response {
    if !has_auth_headers(&headers) {
        return unauthorized_response();
    }
    record_query(&state, "openAlgoOrders", query);
    match state.report_fixture_mode {
        ReportFixtureMode::Empty => json_response(&json!([])),
        ReportFixtureMode::Populated => json_response(&load_fixture("open_algo_orders.json")),
    }
}

async fn handle_all_orders_query(
    State(state): State<CommandResponseState>,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
) -> Response {
    if !has_auth_headers(&headers) {
        return unauthorized_response();
    }
    record_query(&state, "allOrders", query);
    json_response(&json!([]))
}

async fn handle_user_trades_query(
    State(state): State<CommandResponseState>,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
) -> Response {
    if !has_auth_headers(&headers) {
        return unauthorized_response();
    }
    record_query(&state, "userTrades", query);
    json_response(&json!([]))
}

async fn handle_order_submit(
    State(state): State<CommandResponseState>,
    headers: HeaderMap,
) -> Response {
    if !has_auth_headers(&headers) {
        return unauthorized_response();
    }
    state.request_count.fetch_add(1, Ordering::Relaxed);
    command_response(state.responses.submit, &load_fixture("order_response.json"))
}

async fn handle_order_cancel(
    State(state): State<CommandResponseState>,
    headers: HeaderMap,
) -> Response {
    if !has_auth_headers(&headers) {
        return unauthorized_response();
    }
    state.request_count.fetch_add(1, Ordering::Relaxed);
    let mut response = load_fixture("order_response.json");
    response["status"] = json!("CANCELED");
    command_response(state.responses.cancel, &response)
}

async fn handle_order_modify(
    State(state): State<CommandResponseState>,
    headers: HeaderMap,
) -> Response {
    if !has_auth_headers(&headers) {
        return unauthorized_response();
    }
    state.request_count.fetch_add(1, Ordering::Relaxed);
    command_response(state.responses.modify, &load_fixture("order_response.json"))
}

async fn handle_order_query(
    State(state): State<CommandResponseState>,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
) -> Response {
    if !has_auth_headers(&headers) {
        return unauthorized_response();
    }
    record_query(&state, "order", query);
    state.request_count.fetch_add(1, Ordering::Relaxed);
    command_response(state.responses.submit, &load_fixture("order_response.json"))
}

async fn handle_batch_cancel(
    State(state): State<CommandResponseState>,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
) -> Response {
    if !has_auth_headers(&headers) {
        return unauthorized_response();
    }
    let count = batch_cancel_item_count(&query).max(1);
    record_query(&state, "batchOrders", query);
    state.request_count.fetch_add(1, Ordering::Relaxed);
    if let CommandResponse::BatchPerOrderReject { code, msg } = state.responses.batch_cancel {
        let errors = (0..count)
            .map(|_| json!({"code": code, "msg": msg}))
            .collect::<Vec<_>>();
        return json_response(&json!(errors));
    }

    command_response(state.responses.batch_cancel, &json!([]))
}

fn batch_cancel_item_count(query: &HashMap<String, String>) -> usize {
    ["orderIdList", "origClientOrderIdList"]
        .into_iter()
        .filter_map(|key| query.get(key))
        .filter_map(|value| serde_json::from_str::<Vec<serde_json::Value>>(value).ok())
        .map(|values| values.len())
        .sum()
}

fn command_response(response: CommandResponse, success: &serde_json::Value) -> Response {
    match response {
        CommandResponse::Success => json_response(success),
        CommandResponse::AmbiguousFailure => (
            StatusCode::SERVICE_UNAVAILABLE,
            [("content-type", "text/plain")],
            "temporary gateway failure",
        )
            .into_response(),
        CommandResponse::BatchPerOrderReject { code, msg } => {
            json_response(&json!([{"code": code, "msg": msg}]))
        }
        CommandResponse::VenueReject { code, msg } => (
            StatusCode::BAD_REQUEST,
            [("content-type", "application/json")],
            json!({"code": code, "msg": msg}).to_string(),
        )
            .into_response(),
    }
}

fn create_exec_test_router_with_algo_capture_and_hedge_mode(
    captured_query: &Arc<std::sync::Mutex<Option<HashMap<String, String>>>>,
    hedge_mode: bool,
) -> Router {
    create_exec_test_router_with_command_responses(CommandResponseState {
        responses: CommandResponses::default(),
        request_count: Arc::new(AtomicUsize::new(0)),
        captured_queries: None,
        captured_ws_trading_messages: None,
        report_fixture_mode: ReportFixtureMode::Empty,
        hedge_mode,
    })
    .route(
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

async fn start_exec_test_server_with_command_responses(
    responses: CommandResponses,
) -> (SocketAddr, Arc<AtomicUsize>) {
    let request_count = Arc::new(AtomicUsize::new(0));
    let router = create_exec_test_router_with_command_responses(CommandResponseState {
        responses,
        request_count: request_count.clone(),
        captured_queries: None,
        captured_ws_trading_messages: None,
        report_fixture_mode: ReportFixtureMode::Empty,
        hedge_mode: false,
    });
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

    (addr, request_count)
}

async fn start_exec_test_server_with_query_capture() -> (SocketAddr, CapturedQueries) {
    start_exec_test_server_with_query_capture_and_responses(
        CommandResponses::default(),
        ReportFixtureMode::Empty,
    )
    .await
}

async fn start_exec_test_server_with_query_capture_and_responses(
    responses: CommandResponses,
    report_fixture_mode: ReportFixtureMode,
) -> (SocketAddr, CapturedQueries) {
    let captured_queries = Arc::new(std::sync::Mutex::new(Vec::new()));
    let router = create_exec_test_router_with_command_responses(CommandResponseState {
        responses,
        request_count: Arc::new(AtomicUsize::new(0)),
        captured_queries: Some(captured_queries.clone()),
        captured_ws_trading_messages: None,
        report_fixture_mode,
        hedge_mode: false,
    });
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

    (addr, captured_queries)
}

async fn start_exec_test_server_with_ws_trading_capture() -> (SocketAddr, CapturedWsTradingMessages)
{
    start_exec_test_server_with_ws_trading_capture_and_hedge_mode(false).await
}

async fn start_exec_test_server_with_ws_trading_capture_and_hedge_mode(
    hedge_mode: bool,
) -> (SocketAddr, CapturedWsTradingMessages) {
    let captured_ws_trading_messages = Arc::new(std::sync::Mutex::new(Vec::new()));
    let router = create_exec_test_router_with_command_responses(CommandResponseState {
        responses: CommandResponses::default(),
        request_count: Arc::new(AtomicUsize::new(0)),
        captured_queries: None,
        captured_ws_trading_messages: Some(captured_ws_trading_messages.clone()),
        report_fixture_mode: ReportFixtureMode::Empty,
        hedge_mode,
    });
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

    (addr, captured_ws_trading_messages)
}

async fn start_exec_test_server_with_algo_capture() -> (
    SocketAddr,
    Arc<std::sync::Mutex<Option<HashMap<String, String>>>>,
) {
    start_exec_test_server_with_algo_capture_and_hedge_mode(false).await
}

async fn start_exec_test_server_with_algo_capture_and_hedge_mode(
    hedge_mode: bool,
) -> (
    SocketAddr,
    Arc<std::sync::Mutex<Option<HashMap<String, String>>>>,
) {
    let captured_query = Arc::new(std::sync::Mutex::new(None));
    let router =
        create_exec_test_router_with_algo_capture_and_hedge_mode(&captured_query, hedge_mode);
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
    hedge_mode: bool,
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
            get(move |headers: HeaderMap| async move {
                if !has_auth_headers(&headers) {
                    return unauthorized_response();
                }
                json_response(&json!({"dualSidePosition": hedge_mode}))
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
    start_exec_test_server_with_order_capture_and_hedge_mode(false).await
}

async fn start_exec_test_server_with_order_capture_and_hedge_mode(
    hedge_mode: bool,
) -> (
    SocketAddr,
    Arc<std::sync::Mutex<Option<HashMap<String, String>>>>,
) {
    let captured_query = Arc::new(std::sync::Mutex::new(None));
    let router = create_exec_test_router_with_order_capture(&captured_query, hedge_mode);
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
    let client_id = *BINANCE_CLIENT_ID;

    let cache = Rc::new(RefCell::new(Cache::default()));

    let core = ExecutionClientCore::new(
        trader_id,
        client_id,
        *BINANCE_VENUE,
        OmsType::Hedging,
        account_id,
        AccountType::Margin,
        None,
        cache.clone(),
    );

    let config = BinanceExecClientConfig {
        trader_id,
        account_id,
        product_type: BinanceProductType::UsdM,
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

fn add_test_instrument_to_cache(cache: &Rc<RefCell<Cache>>) {
    let exchange_info: BinanceFuturesUsdExchangeInfo =
        serde_json::from_value(exchange_info_response()).unwrap();
    let symbol = exchange_info.symbols.first().unwrap();
    let instrument =
        parse_usdm_instrument(symbol, UnixNanos::default(), UnixNanos::default()).unwrap();

    cache.borrow_mut().add_instrument(instrument).unwrap();
}

#[rstest]
#[tokio::test]
async fn test_client_creation() {
    let addr = start_exec_test_server().await;
    let base_url_http = format!("http://{addr}");
    let base_url_ws = format!("ws://{addr}/ws");

    let (client, _rx, _cache) = create_test_execution_client(base_url_http, base_url_ws);

    assert_eq!(client.client_id(), *BINANCE_CLIENT_ID);
    assert_eq!(client.venue(), *BINANCE_VENUE);
    assert_eq!(client.oms_type(), OmsType::Hedging);
    assert!(!client.is_connected());
}

#[rstest]
fn test_client_creation_rejects_spot_product_type() {
    let trader_id = TraderId::from("TESTER-001");
    let account_id = AccountId::from("BINANCE-001");
    let cache = Rc::new(RefCell::new(Cache::default()));
    let core = ExecutionClientCore::new(
        trader_id,
        *BINANCE_CLIENT_ID,
        *BINANCE_VENUE,
        OmsType::Hedging,
        account_id,
        AccountType::Margin,
        None,
        cache,
    );
    let config = BinanceExecClientConfig {
        trader_id,
        account_id,
        product_type: BinanceProductType::Spot,
        ..Default::default()
    };

    let result = BinanceFuturesExecutionClient::new(core, config);

    let Err(e) = result else {
        panic!("futures execution client should reject Spot product type");
    };
    assert_eq!(
        e.to_string(),
        "BinanceFuturesExecutionClient requires UsdM or CoinM product type, was Spot",
    );
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
        Some(*BINANCE_CLIENT_ID),
        strategy_id,
        instrument_id,
        order_any.client_order_id(),
        order_any.init_event().clone(),
        None, // exec_algorithm_id
        None, // position_id
        None, // params
        nautilus_core::UUID4::new(),
        UnixNanos::default(),
        None, // correlation_id
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
        Some(*BINANCE_CLIENT_ID),
        strategy_id,
        instrument_id,
        client_order_id,
        order_any.init_event().clone(),
        None,
        None,
        None,
        nautilus_core::UUID4::new(),
        UnixNanos::default(),
        None, // correlation_id
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
    assert_eq!(query.get("reduceOnly"), Some(&"true".to_string()));
    assert!(!query.contains_key("triggerPrice"));
    assert!(!query.contains_key("activationPrice"));
}

#[rstest]
#[tokio::test]
async fn test_submit_algo_order_in_hedge_mode_omits_reduce_only() {
    let (addr, captured_query) =
        start_exec_test_server_with_algo_capture_and_hedge_mode(true).await;
    let base_url_http = format!("http://{addr}");
    let base_url_ws = format!("ws://{addr}/ws");

    let (mut client, _rx, cache) = create_test_execution_client(base_url_http, base_url_ws);
    add_test_account_to_cache(&cache, AccountId::from("BINANCE-001"));

    client.start().unwrap();
    client.connect().await.unwrap();

    let instrument_id = InstrumentId::from("BTCUSDT-PERP.BINANCE");
    let client_order_id = ClientOrderId::new("hedge-trailing-stop-test-001");
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
        Some(*BINANCE_CLIENT_ID),
        strategy_id,
        instrument_id,
        client_order_id,
        order_any.init_event().clone(),
        None,
        None,
        None,
        nautilus_core::UUID4::new(),
        UnixNanos::default(),
        None,
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
    assert_eq!(query.get("positionSide"), Some(&"LONG".to_string()));
    assert!(!query.contains_key("reduceOnly"));
}

#[rstest]
#[case::one_way(false, None, Some("true"))]
#[case::hedge(true, Some("LONG"), None)]
#[tokio::test]
async fn test_submit_reduce_only_limit_order_respects_position_mode(
    #[case] hedge_mode: bool,
    #[case] expected_position_side: Option<&'static str>,
    #[case] expected_reduce_only: Option<&'static str>,
) {
    let (addr, captured_query) =
        start_exec_test_server_with_order_capture_and_hedge_mode(hedge_mode).await;
    let base_url_http = format!("http://{addr}");
    let base_url_ws = format!("ws://{addr}/ws");

    let (mut client, _rx, cache) = create_test_execution_client(base_url_http, base_url_ws);
    add_test_account_to_cache(&cache, AccountId::from("BINANCE-001"));

    client.start().unwrap();
    client.connect().await.unwrap();

    let client_order_id = ClientOrderId::new("reduce-only-limit-test-001");
    let order_any = add_reduce_only_limit_order_to_cache(&cache, client_order_id);

    client
        .submit_order(submit_order_command(&order_any))
        .unwrap();

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
        query.get("positionSide").map(String::as_str),
        expected_position_side
    );
    assert_eq!(
        query.get("reduceOnly").map(String::as_str),
        expected_reduce_only
    );
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
        Some(*BINANCE_CLIENT_ID),
        StrategyId::from("TEST-STRATEGY"),
        instrument_id,
        OrderSide::NoOrderSide,
        nautilus_core::UUID4::new(),
        UnixNanos::default(),
        None,
        None, // correlation_id
    );

    // Futures cancel_all returns success code via HTTP; cancel events arrive through WS
    let result = client.cancel_all_orders(cancel_all_cmd);
    result.unwrap();
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
        Some(*BINANCE_CLIENT_ID),
        strategy_id,
        instrument_id,
        client_order_id,
        Some(VenueOrderId::from("12345")),
        nautilus_core::UUID4::new(),
        UnixNanos::default(),
        None,
        None, // correlation_id
    );

    // Futures cancel queues an async HTTP task. The actual OrderCanceled event
    // arrives via the WS user data stream, which this mock does not simulate.
    // We verify the command is accepted and the HTTP request completes without error.
    let result = client.cancel_order(cancel_cmd);
    result.unwrap();
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
        Some(*BINANCE_CLIENT_ID),
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
        None, // correlation_id
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
async fn test_ambiguous_submit_failure_does_not_emit_order_rejected() {
    let (client, mut rx, cache, request_count) =
        connected_client_with_command_responses(CommandResponses {
            submit: CommandResponse::AmbiguousFailure,
            ..Default::default()
        })
        .await;

    let client_order_id = ClientOrderId::new("ambiguous-submit-test-001");
    let order_any = add_limit_order_to_cache(&cache, client_order_id);

    client
        .submit_order(submit_order_command(&order_any))
        .unwrap();

    wait_for_command_requests(&request_count, 1).await;

    assert_no_order_event_matching(&mut rx, |event| {
        matches!(
            event,
            OrderEventAny::Rejected(event) if event.client_order_id == client_order_id
        )
    })
    .await;
}

#[rstest]
#[case(
    BINANCE_UNEXPECTED_RESPONSE_CODE,
    "An unexpected response was received from the message bus"
)]
#[case(
    BINANCE_STATUS_UNKNOWN_CODE,
    "Timeout waiting for response from backend server"
)]
#[tokio::test]
async fn test_unknown_status_submit_rejection_does_not_emit_order_rejected(
    #[case] code: i64,
    #[case] msg: &'static str,
) {
    let (client, mut rx, cache, request_count) =
        connected_client_with_command_responses(CommandResponses {
            submit: CommandResponse::VenueReject { code, msg },
            ..Default::default()
        })
        .await;

    let client_order_id = ClientOrderId::new("status-unknown-submit-test-001");
    let order_any = add_limit_order_to_cache(&cache, client_order_id);

    client
        .submit_order(submit_order_command(&order_any))
        .unwrap();

    wait_for_command_requests(&request_count, 1).await;

    assert_no_order_event_matching(&mut rx, |event| {
        matches!(
            event,
            OrderEventAny::Rejected(event) if event.client_order_id == client_order_id
        )
    })
    .await;
}

#[rstest]
#[tokio::test]
async fn test_explicit_venue_submit_rejection_emits_order_rejected() {
    let (client, mut rx, cache, _request_count) =
        connected_client_with_command_responses(CommandResponses {
            submit: CommandResponse::VenueReject {
                code: -2010,
                msg: "Order would immediately match and take",
            },
            ..Default::default()
        })
        .await;

    let client_order_id = ClientOrderId::new("venue-submit-reject-test-001");
    let order_any = add_limit_order_to_cache(&cache, client_order_id);

    client
        .submit_order(submit_order_command(&order_any))
        .unwrap();

    match recv_until(&mut rx, |event| {
        matches!(
            event,
            ExecutionEvent::Order(OrderEventAny::Rejected(event))
                if event.client_order_id == client_order_id
        )
    })
    .await
    {
        ExecutionEvent::Order(OrderEventAny::Rejected(event)) => {
            assert_eq!(event.client_order_id, client_order_id);
            assert!(
                event
                    .reason
                    .as_str()
                    .contains("Order would immediately match")
            );
        }
        other => panic!("Expected Rejected event, was {other:?}"),
    }
}

#[rstest]
#[tokio::test]
async fn test_ambiguous_cancel_failure_does_not_emit_cancel_rejected() {
    let (client, mut rx, cache, request_count) =
        connected_client_with_command_responses(CommandResponses {
            cancel: CommandResponse::AmbiguousFailure,
            ..Default::default()
        })
        .await;

    let client_order_id = ClientOrderId::new("ambiguous-cancel-test-001");
    add_limit_order_to_cache(&cache, client_order_id);

    client
        .cancel_order(cancel_order_command(client_order_id))
        .unwrap();

    wait_for_command_requests(&request_count, 1).await;

    assert_no_order_event_matching(&mut rx, |event| {
        matches!(
            event,
            OrderEventAny::CancelRejected(event) if event.client_order_id == client_order_id
        )
    })
    .await;
}

#[rstest]
#[tokio::test]
async fn test_explicit_venue_cancel_rejection_emits_cancel_rejected() {
    let (client, mut rx, cache, _request_count) =
        connected_client_with_command_responses(CommandResponses {
            cancel: CommandResponse::VenueReject {
                code: -2011,
                msg: "Unknown order sent",
            },
            ..Default::default()
        })
        .await;

    let client_order_id = ClientOrderId::new("venue-cancel-reject-test-001");
    add_limit_order_to_cache(&cache, client_order_id);

    client
        .cancel_order(cancel_order_command(client_order_id))
        .unwrap();

    match recv_until(&mut rx, |event| {
        matches!(
            event,
            ExecutionEvent::Order(OrderEventAny::CancelRejected(event))
                if event.client_order_id == client_order_id
        )
    })
    .await
    {
        ExecutionEvent::Order(OrderEventAny::CancelRejected(event)) => {
            assert_eq!(event.client_order_id, client_order_id);
            assert!(event.reason.as_str().contains("Unknown order sent"));
        }
        other => panic!("Expected CancelRejected event, was {other:?}"),
    }
}

#[rstest]
#[tokio::test]
async fn test_cancel_order_http_local_venue_order_id_parse_failure_falls_back_to_client_order_id() {
    let (client, mut rx, cache, request_count) =
        connected_client_with_command_responses(CommandResponses::default()).await;

    let client_order_id = ClientOrderId::new("cancel-http-local-invalid-test-001");
    add_limit_order_to_cache(&cache, client_order_id);

    let cancel_cmd = CancelOrder::new(
        test_trader_id(),
        Some(*BINANCE_CLIENT_ID),
        test_strategy_id(),
        test_instrument_id(),
        client_order_id,
        Some(VenueOrderId::from("not-a-number")),
        nautilus_core::UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );

    client.cancel_order(cancel_cmd).unwrap();

    wait_for_command_requests(&request_count, 1).await;

    assert_no_order_event_matching(&mut rx, |event| {
        matches!(
            event,
            OrderEventAny::CancelRejected(event) if event.client_order_id == client_order_id
        )
    })
    .await;
}

#[rstest]
#[tokio::test]
async fn test_ambiguous_modify_failure_does_not_emit_modify_rejected() {
    let (client, mut rx, cache, request_count) =
        connected_client_with_command_responses(CommandResponses {
            modify: CommandResponse::AmbiguousFailure,
            ..Default::default()
        })
        .await;

    let client_order_id = ClientOrderId::new("ambiguous-modify-test-001");
    add_limit_order_to_cache(&cache, client_order_id);

    client
        .modify_order(modify_order_command(client_order_id))
        .unwrap();

    wait_for_command_requests(&request_count, 1).await;

    assert_no_order_event_matching(&mut rx, |event| {
        matches!(
            event,
            OrderEventAny::ModifyRejected(event) if event.client_order_id == client_order_id
        )
    })
    .await;
}

#[rstest]
#[tokio::test]
async fn test_explicit_venue_modify_rejection_emits_modify_rejected() {
    let (client, mut rx, cache, _request_count) =
        connected_client_with_command_responses(CommandResponses {
            modify: CommandResponse::VenueReject {
                code: -4028,
                msg: "Price or quantity not changed",
            },
            ..Default::default()
        })
        .await;

    let client_order_id = ClientOrderId::new("venue-modify-reject-test-001");
    add_limit_order_to_cache(&cache, client_order_id);

    client
        .modify_order(modify_order_command(client_order_id))
        .unwrap();

    match recv_until(&mut rx, |event| {
        matches!(
            event,
            ExecutionEvent::Order(OrderEventAny::ModifyRejected(event))
                if event.client_order_id == client_order_id
        )
    })
    .await
    {
        ExecutionEvent::Order(OrderEventAny::ModifyRejected(event)) => {
            assert_eq!(event.client_order_id, client_order_id);
            assert!(
                event
                    .reason
                    .as_str()
                    .contains("Price or quantity not changed")
            );
        }
        other => panic!("Expected ModifyRejected event, was {other:?}"),
    }
}

#[rstest]
#[tokio::test]
async fn test_whole_batch_cancel_failure_does_not_emit_per_order_cancel_rejected() {
    let (client, mut rx, _cache, request_count) =
        connected_client_with_command_responses(CommandResponses {
            batch_cancel: CommandResponse::AmbiguousFailure,
            ..Default::default()
        })
        .await;

    let first_client_order_id = ClientOrderId::new("batch-cancel-fail-test-001");
    let second_client_order_id = ClientOrderId::new("batch-cancel-fail-test-002");

    client
        .batch_cancel_orders(batch_cancel_order_command(vec![
            first_client_order_id,
            second_client_order_id,
        ]))
        .unwrap();

    wait_for_command_requests(&request_count, 1).await;

    assert_no_order_event_matching(&mut rx, |event| {
        matches!(event, OrderEventAny::CancelRejected(_))
    })
    .await;
}

#[rstest]
#[tokio::test]
async fn test_per_order_batch_cancel_rejection_emits_cancel_rejected() {
    let (client, mut rx, _cache, _request_count) =
        connected_client_with_command_responses(CommandResponses {
            batch_cancel: CommandResponse::BatchPerOrderReject {
                code: -2011,
                msg: "Unknown order sent",
            },
            ..Default::default()
        })
        .await;

    let client_order_id = ClientOrderId::new("batch-cancel-reject-test-001");

    client
        .batch_cancel_orders(batch_cancel_order_command(vec![client_order_id]))
        .unwrap();

    match recv_until(&mut rx, |event| {
        matches!(
            event,
            ExecutionEvent::Order(OrderEventAny::CancelRejected(event))
                if event.client_order_id == client_order_id
        )
    })
    .await
    {
        ExecutionEvent::Order(OrderEventAny::CancelRejected(event)) => {
            assert_eq!(event.client_order_id, client_order_id);
            assert!(event.reason.as_str().contains("code=-2011"));
            assert!(event.reason.as_str().contains("Unknown order sent"));
        }
        other => panic!("Expected CancelRejected event, was {other:?}"),
    }
}

#[rstest]
#[tokio::test]
async fn test_query_order_uses_binance_symbol_for_futures_symbol() {
    let (addr, captured_queries) = start_exec_test_server_with_query_capture().await;
    let base_url_http = format!("http://{addr}");
    let base_url_ws = format!("ws://{addr}/ws");

    let (mut client, _rx, cache) = create_test_execution_client(base_url_http, base_url_ws);
    add_test_account_to_cache(&cache, AccountId::from("BINANCE-001"));

    client.start().unwrap();
    client.connect().await.unwrap();

    let cmd = QueryOrder::new(
        test_trader_id(),
        Some(*BINANCE_CLIENT_ID),
        test_strategy_id(),
        test_instrument_id(),
        ClientOrderId::new("query-symbol-test-001"),
        Some(VenueOrderId::from("12345")),
        nautilus_core::UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );

    client.query_order(cmd).unwrap();

    let captured = wait_for_query(&captured_queries, "order").await;
    assert_query_symbol(&captured.query);
}

#[rstest]
#[tokio::test]
async fn test_report_generation_uses_binance_symbol_for_futures_symbol() {
    let (addr, captured_queries) = start_exec_test_server_with_query_capture().await;
    let base_url_http = format!("http://{addr}");
    let base_url_ws = format!("ws://{addr}/ws");

    let (mut client, _rx, cache) = create_test_execution_client(base_url_http, base_url_ws);
    add_test_account_to_cache(&cache, AccountId::from("BINANCE-001"));

    client.start().unwrap();
    client.connect().await.unwrap();

    let instrument_id = test_instrument_id();
    let order_report = GenerateOrderStatusReport::new(
        nautilus_core::UUID4::new(),
        UnixNanos::default(),
        Some(instrument_id),
        Some(ClientOrderId::new("order-report-symbol-test-001")),
        Some(VenueOrderId::from("12345")),
        None,
        None,
    );

    client
        .generate_order_status_report(&order_report)
        .await
        .unwrap();
    assert_query_symbol(&wait_for_query(&captured_queries, "order").await.query);

    let open_orders = GenerateOrderStatusReports::new(
        nautilus_core::UUID4::new(),
        UnixNanos::default(),
        true,
        Some(instrument_id),
        None,
        None,
        None,
        None,
    );

    client
        .generate_order_status_reports(&open_orders)
        .await
        .unwrap();
    assert_query_symbol(&wait_for_query(&captured_queries, "openOrders").await.query);
    assert_query_symbol(
        &wait_for_query(&captured_queries, "openAlgoOrders")
            .await
            .query,
    );

    let all_orders = GenerateOrderStatusReports::new(
        nautilus_core::UUID4::new(),
        UnixNanos::default(),
        false,
        Some(instrument_id),
        None,
        None,
        None,
        None,
    );

    client
        .generate_order_status_reports(&all_orders)
        .await
        .unwrap();
    assert_query_symbol(&wait_for_query(&captured_queries, "allOrders").await.query);

    let fills = GenerateFillReports::new(
        nautilus_core::UUID4::new(),
        UnixNanos::default(),
        Some(instrument_id),
        Some(VenueOrderId::from("12345")),
        None,
        None,
        None,
        None,
    );

    client.generate_fill_reports(fills).await.unwrap();
    assert_query_symbol(&wait_for_query(&captured_queries, "userTrades").await.query);

    let positions = GeneratePositionStatusReports::new(
        nautilus_core::UUID4::new(),
        UnixNanos::default(),
        Some(instrument_id),
        None,
        None,
        None,
        None,
    );

    client
        .generate_position_status_reports(&positions)
        .await
        .unwrap();
    assert_query_symbol(
        &wait_for_query(&captured_queries, "positionRisk")
            .await
            .query,
    );
}

#[rstest]
#[tokio::test]
async fn test_report_generation_without_instrument_matches_raw_symbol_responses() {
    let (addr, _captured_queries) = start_exec_test_server_with_query_capture_and_responses(
        CommandResponses::default(),
        ReportFixtureMode::Populated,
    )
    .await;
    let base_url_http = format!("http://{addr}");
    let base_url_ws = format!("ws://{addr}/ws");

    let (mut client, _rx, cache) = create_test_execution_client(base_url_http, base_url_ws);
    add_test_account_to_cache(&cache, AccountId::from("BINANCE-001"));

    client.start().unwrap();
    client.connect().await.unwrap();
    add_test_instrument_to_cache(&cache);

    let open_orders = GenerateOrderStatusReports::new(
        nautilus_core::UUID4::new(),
        UnixNanos::default(),
        true,
        None,
        None,
        None,
        None,
        None,
    );

    let order_reports = client
        .generate_order_status_reports(&open_orders)
        .await
        .unwrap();

    assert_eq!(order_reports.len(), 2);
    assert!(
        order_reports
            .iter()
            .all(|report| report.instrument_id == test_instrument_id())
    );

    let positions = GeneratePositionStatusReports::new(
        nautilus_core::UUID4::new(),
        UnixNanos::default(),
        None,
        None,
        None,
        None,
        None,
    );

    let position_reports = client
        .generate_position_status_reports(&positions)
        .await
        .unwrap();

    assert_eq!(position_reports.len(), 1);
    assert_eq!(position_reports[0].instrument_id, test_instrument_id());
}

#[rstest]
#[tokio::test]
async fn test_batch_cancel_uses_binance_symbol_for_futures_symbol() {
    let (addr, captured_queries) = start_exec_test_server_with_query_capture().await;
    let base_url_http = format!("http://{addr}");
    let base_url_ws = format!("ws://{addr}/ws");

    let (mut client, _rx, cache) = create_test_execution_client(base_url_http, base_url_ws);
    add_test_account_to_cache(&cache, AccountId::from("BINANCE-001"));

    client.start().unwrap();
    client.connect().await.unwrap();

    client
        .batch_cancel_orders(batch_cancel_order_command(vec![ClientOrderId::new(
            "batch-cancel-symbol-test-001",
        )]))
        .unwrap();

    let captured = wait_for_query(&captured_queries, "batchOrders").await;
    assert_query_symbol(&captured.query);
    assert_eq!(
        captured.query.get("orderIdList").map(String::as_str),
        Some("[12345]")
    );
    assert!(!captured.query.contains_key("batchOrders"));
}

#[rstest]
#[tokio::test]
async fn test_mixed_batch_cancel_splits_id_lists_and_maps_rejections() {
    let (addr, captured_queries) = start_exec_test_server_with_query_capture_and_responses(
        CommandResponses {
            batch_cancel: CommandResponse::BatchPerOrderReject {
                code: -2011,
                msg: "Unknown order sent",
            },
            ..Default::default()
        },
        ReportFixtureMode::Empty,
    )
    .await;
    let base_url_http = format!("http://{addr}");
    let base_url_ws = format!("ws://{addr}/ws");

    let (mut client, mut rx, cache) = create_test_execution_client(base_url_http, base_url_ws);
    add_test_account_to_cache(&cache, AccountId::from("BINANCE-001"));

    client.start().unwrap();
    client.connect().await.unwrap();

    let client_id_only = ClientOrderId::new("batch-client-id-001");
    let order_id = ClientOrderId::new("batch-order-id-001");

    client
        .batch_cancel_orders(batch_cancel_order_command_from_cancels(vec![
            cancel_order_command_without_venue_id(client_id_only),
            cancel_order_command(order_id),
        ]))
        .unwrap();

    let captured = wait_for_queries(&captured_queries, "batchOrders", 2).await;
    let order_id_query = &captured[0].query;
    assert_query_symbol(order_id_query);
    assert_eq!(
        order_id_query.get("orderIdList").map(String::as_str),
        Some("[12345]")
    );
    assert!(!order_id_query.contains_key("origClientOrderIdList"));
    assert!(!order_id_query.contains_key("batchOrders"));

    let client_order_id_query = &captured[1].query;
    let expected_client_order_id = format!(
        "[\"{}\"]",
        encode_broker_id(&client_id_only, BINANCE_NAUTILUS_FUTURES_BROKER_ID)
    );
    assert_query_symbol(client_order_id_query);
    assert_eq!(
        client_order_id_query
            .get("origClientOrderIdList")
            .map(String::as_str),
        Some(expected_client_order_id.as_str())
    );
    assert!(!client_order_id_query.contains_key("orderIdList"));
    assert!(!client_order_id_query.contains_key("batchOrders"));

    match recv_until(&mut rx, |event| {
        matches!(
            event,
            ExecutionEvent::Order(OrderEventAny::CancelRejected(event))
                if event.client_order_id == order_id
        )
    })
    .await
    {
        ExecutionEvent::Order(OrderEventAny::CancelRejected(event)) => {
            assert_eq!(event.client_order_id, order_id);
            assert_eq!(event.venue_order_id, Some(VenueOrderId::from("12345")));
        }
        other => panic!("Expected CancelRejected event, was {other:?}"),
    }

    match recv_until(&mut rx, |event| {
        matches!(
            event,
            ExecutionEvent::Order(OrderEventAny::CancelRejected(event))
                if event.client_order_id == client_id_only
        )
    })
    .await
    {
        ExecutionEvent::Order(OrderEventAny::CancelRejected(event)) => {
            assert_eq!(event.client_order_id, client_id_only);
            assert_eq!(event.venue_order_id, None);
        }
        other => panic!("Expected CancelRejected event, was {other:?}"),
    }
}

async fn connected_client_with_command_responses(
    responses: CommandResponses,
) -> (
    BinanceFuturesExecutionClient,
    tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
    Rc<RefCell<Cache>>,
    Arc<AtomicUsize>,
) {
    let (addr, request_count) = start_exec_test_server_with_command_responses(responses).await;
    let base_url_http = format!("http://{addr}");
    let base_url_ws = format!("ws://{addr}/ws");

    let (mut client, rx, cache) = create_test_execution_client(base_url_http, base_url_ws);
    add_test_account_to_cache(&cache, AccountId::from("BINANCE-001"));

    client.start().unwrap();
    client.connect().await.unwrap();

    (client, rx, cache, request_count)
}

fn add_limit_order_to_cache(
    cache: &Rc<RefCell<Cache>>,
    client_order_id: ClientOrderId,
) -> OrderAny {
    let order = LimitOrder::new(
        test_trader_id(),
        test_strategy_id(),
        test_instrument_id(),
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
    order_any
}

fn add_reduce_only_limit_order_to_cache(
    cache: &Rc<RefCell<Cache>>,
    client_order_id: ClientOrderId,
) -> OrderAny {
    let order = LimitOrder::new(
        test_trader_id(),
        test_strategy_id(),
        test_instrument_id(),
        client_order_id,
        OrderSide::Sell,
        Quantity::from("0.001"),
        Price::from("50000.00"),
        TimeInForce::Gtc,
        None,
        false,
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

    let order_any = OrderAny::Limit(order);
    cache
        .borrow_mut()
        .add_order(order_any.clone(), None, None, false)
        .unwrap();
    order_any
}

fn submit_order_command(order: &OrderAny) -> SubmitOrder {
    SubmitOrder::new(
        test_trader_id(),
        Some(*BINANCE_CLIENT_ID),
        test_strategy_id(),
        test_instrument_id(),
        order.client_order_id(),
        order.init_event().clone(),
        None,
        None,
        None,
        nautilus_core::UUID4::new(),
        UnixNanos::default(),
        None,
    )
}

fn cancel_order_command(client_order_id: ClientOrderId) -> CancelOrder {
    CancelOrder::new(
        test_trader_id(),
        Some(*BINANCE_CLIENT_ID),
        test_strategy_id(),
        test_instrument_id(),
        client_order_id,
        Some(VenueOrderId::from("12345")),
        nautilus_core::UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    )
}

fn cancel_order_command_without_venue_id(client_order_id: ClientOrderId) -> CancelOrder {
    CancelOrder::new(
        test_trader_id(),
        Some(*BINANCE_CLIENT_ID),
        test_strategy_id(),
        test_instrument_id(),
        client_order_id,
        None,
        nautilus_core::UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    )
}

fn modify_order_command(client_order_id: ClientOrderId) -> ModifyOrder {
    ModifyOrder::new(
        test_trader_id(),
        Some(*BINANCE_CLIENT_ID),
        test_strategy_id(),
        test_instrument_id(),
        client_order_id,
        Some(VenueOrderId::from("12345")),
        Some(Quantity::from("0.002")),
        Some(Price::from("51000.00")),
        None,
        nautilus_core::UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    )
}

fn batch_cancel_order_command(client_order_ids: Vec<ClientOrderId>) -> BatchCancelOrders {
    let cancels = client_order_ids
        .into_iter()
        .map(cancel_order_command)
        .collect();

    batch_cancel_order_command_from_cancels(cancels)
}

fn batch_cancel_order_command_from_cancels(cancels: Vec<CancelOrder>) -> BatchCancelOrders {
    BatchCancelOrders::new(
        test_trader_id(),
        Some(*BINANCE_CLIENT_ID),
        test_strategy_id(),
        test_instrument_id(),
        cancels,
        nautilus_core::UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    )
}

async fn wait_for_command_requests(request_count: &AtomicUsize, expected: usize) {
    wait_until_async(
        || async { request_count.load(Ordering::Relaxed) >= expected },
        Duration::from_secs(5),
    )
    .await;
}

async fn wait_for_query(captured_queries: &CapturedQueries, path: &'static str) -> CapturedQuery {
    wait_until_async(
        || {
            let captured_queries = captured_queries.clone();
            async move {
                captured_queries
                    .lock()
                    .unwrap()
                    .iter()
                    .any(|entry| entry.path == path)
            }
        },
        Duration::from_secs(5),
    )
    .await;

    captured_queries
        .lock()
        .unwrap()
        .iter()
        .find(|entry| entry.path == path)
        .cloned()
        .unwrap()
}

async fn wait_for_queries(
    captured_queries: &CapturedQueries,
    path: &'static str,
    expected: usize,
) -> Vec<CapturedQuery> {
    wait_until_async(
        || {
            let captured_queries = captured_queries.clone();
            async move {
                captured_queries
                    .lock()
                    .unwrap()
                    .iter()
                    .filter(|entry| entry.path == path)
                    .count()
                    >= expected
            }
        },
        Duration::from_secs(5),
    )
    .await;

    captured_queries
        .lock()
        .unwrap()
        .iter()
        .filter(|entry| entry.path == path)
        .take(expected)
        .cloned()
        .collect()
}

async fn wait_for_ws_trading_method(
    captured_messages: &CapturedWsTradingMessages,
    method: &'static str,
) -> serde_json::Value {
    wait_until_async(
        || {
            let captured_messages = captured_messages.clone();
            async move {
                captured_messages.lock().unwrap().iter().any(|message| {
                    message.get("method").and_then(|value| value.as_str()) == Some(method)
                })
            }
        },
        Duration::from_secs(5),
    )
    .await;

    captured_messages
        .lock()
        .unwrap()
        .iter()
        .find(|message| message.get("method").and_then(|value| value.as_str()) == Some(method))
        .cloned()
        .unwrap()
}

fn assert_query_symbol(query: &HashMap<String, String>) {
    assert_eq!(query.get("symbol").map(String::as_str), Some("BTCUSDT"));
    assert!(!query.values().any(|value| value.contains("BTCUSDT-PERP")));
}

async fn assert_no_order_event_matching<F>(
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
    predicate: F,
) where
    F: Fn(&OrderEventAny) -> bool,
{
    let unexpected = tokio::time::timeout(Duration::from_millis(500), async {
        loop {
            let event = rx.recv().await.expect("Execution event channel closed");
            if let ExecutionEvent::Order(order_event) = &event
                && predicate(order_event)
            {
                return event;
            }
        }
    })
    .await;

    if let Ok(event) = unexpected {
        panic!("Unexpected order event: {event:?}");
    }
}

fn test_trader_id() -> TraderId {
    TraderId::from("TESTER-001")
}

fn test_strategy_id() -> StrategyId {
    StrategyId::from("TEST-STRATEGY")
}

fn test_instrument_id() -> InstrumentId {
    InstrumentId::from("BTCUSDT-PERP.BINANCE")
}

#[rstest]
#[tokio::test]
async fn test_cancel_order_ws_uses_binance_symbol_for_futures_symbol() {
    let (addr, captured_ws_trading_messages) =
        start_exec_test_server_with_ws_trading_capture().await;
    let base_url_http = format!("http://{addr}");
    let base_url_ws = format!("ws://{addr}/ws");
    let base_url_ws_trading = format!("ws://{addr}/ws-fapi/v1");

    let (mut client, _rx, cache) = create_test_execution_client_with_ws_trading(
        base_url_http,
        base_url_ws,
        base_url_ws_trading,
    );
    add_test_account_to_cache(&cache, AccountId::from("BINANCE-001"));

    client.start().unwrap();
    client.connect().await.unwrap();

    let client_order_id = ClientOrderId::new("cancel-ws-symbol-test-001");
    add_limit_order_to_cache(&cache, client_order_id);

    let cancel_cmd = CancelOrder::new(
        test_trader_id(),
        Some(*BINANCE_CLIENT_ID),
        test_strategy_id(),
        test_instrument_id(),
        client_order_id,
        Some(VenueOrderId::from("not-a-number")),
        nautilus_core::UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );

    client.cancel_order(cancel_cmd).unwrap();

    let message = wait_for_ws_trading_method(&captured_ws_trading_messages, "order.cancel").await;
    let params = message.get("params").unwrap();
    assert_eq!(
        params.get("symbol").and_then(|value| value.as_str()),
        Some("BTCUSDT")
    );
    assert!(!params.as_object().unwrap().values().any(|value| {
        value
            .as_str()
            .is_some_and(|text| text.contains("BTCUSDT-PERP"))
    }));
}

#[rstest]
#[case::one_way(false, None, Some(true))]
#[case::hedge(true, Some("LONG"), None)]
#[tokio::test]
async fn test_submit_order_ws_reduce_only_respects_position_mode(
    #[case] hedge_mode: bool,
    #[case] expected_position_side: Option<&'static str>,
    #[case] expected_reduce_only: Option<bool>,
) {
    let (addr, captured_ws_trading_messages) =
        start_exec_test_server_with_ws_trading_capture_and_hedge_mode(hedge_mode).await;
    let base_url_http = format!("http://{addr}");
    let base_url_ws = format!("ws://{addr}/ws");
    let base_url_ws_trading = format!("ws://{addr}/ws-fapi/v1");

    let (mut client, _rx, cache) = create_test_execution_client_with_ws_trading(
        base_url_http,
        base_url_ws,
        base_url_ws_trading,
    );
    add_test_account_to_cache(&cache, AccountId::from("BINANCE-001"));

    client.start().unwrap();
    client.connect().await.unwrap();

    let client_order_id = ClientOrderId::new("submit-ws-hedge-reduce-test-001");
    let order_any = add_reduce_only_limit_order_to_cache(&cache, client_order_id);

    client
        .submit_order(submit_order_command(&order_any))
        .unwrap();

    let message = wait_for_ws_trading_method(&captured_ws_trading_messages, "order.place").await;
    let params = message
        .get("params")
        .and_then(|value| value.as_object())
        .unwrap();
    assert_eq!(
        params.get("positionSide").and_then(|value| value.as_str()),
        expected_position_side
    );
    assert_eq!(
        params.get("reduceOnly").and_then(|value| value.as_bool()),
        expected_reduce_only
    );
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
        Some(*BINANCE_CLIENT_ID),
        strategy_id,
        instrument_id,
        client_order_id,
        order_any.init_event().clone(),
        None,
        None,
        None,
        nautilus_core::UUID4::new(),
        UnixNanos::default(),
        None, // correlation_id
    );

    client.submit_order(submit_cmd).unwrap();

    let cancel_cmd = CancelOrder::new(
        trader_id,
        Some(*BINANCE_CLIENT_ID),
        strategy_id,
        instrument_id,
        client_order_id,
        Some(VenueOrderId::from("12345")),
        nautilus_core::UUID4::new(),
        UnixNanos::default(),
        None,
        None, // correlation_id
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
async fn test_cancel_order_ws_local_venue_order_id_parse_failure_does_not_emit_cancel_rejected() {
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

    let client_order_id = ClientOrderId::new("cancel-ws-local-invalid-test-001");
    add_limit_order_to_cache(&cache, client_order_id);

    let cancel_cmd = CancelOrder::new(
        test_trader_id(),
        Some(*BINANCE_CLIENT_ID),
        test_strategy_id(),
        test_instrument_id(),
        client_order_id,
        Some(VenueOrderId::from("not-a-number")),
        nautilus_core::UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );

    client.cancel_order(cancel_cmd).unwrap();

    assert_no_order_event_matching(&mut rx, |event| {
        matches!(
            event,
            OrderEventAny::CancelRejected(event) if event.client_order_id == client_order_id
        )
    })
    .await;
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
        Some(*BINANCE_CLIENT_ID),
        strategy_id,
        instrument_id,
        client_order_id,
        order_any.init_event().clone(),
        None,
        None,
        None,
        nautilus_core::UUID4::new(),
        UnixNanos::default(),
        None, // correlation_id
    );

    client.submit_order(submit_cmd).unwrap();

    let modify_cmd = ModifyOrder::new(
        trader_id,
        Some(*BINANCE_CLIENT_ID),
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
        None, // correlation_id
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
        Some(*BINANCE_CLIENT_ID),
        strategy_id,
        instrument_id,
        client_order_id,
        order_any.init_event().clone(),
        None, // exec_algorithm_id
        None, // position_id
        Some(params),
        nautilus_core::UUID4::new(),
        UnixNanos::default(),
        None, // correlation_id
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
    let client_id = *BINANCE_CLIENT_ID;

    let cache = Rc::new(RefCell::new(Cache::default()));

    let core = ExecutionClientCore::new(
        trader_id,
        client_id,
        *BINANCE_VENUE,
        OmsType::Hedging,
        account_id,
        AccountType::Margin,
        None,
        cache.clone(),
    );

    let config = BinanceExecClientConfig {
        trader_id,
        account_id,
        product_type: BinanceProductType::UsdM,
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
                            let _result = socket.send(Message::Text(resp.to_string().into())).await;
                        }
                    }
                    None | Some(Err(_)) => break,
                    _ => {}
                }
            }
            Ok(injected) = rx.recv() => {
                let _result = socket.send(Message::Text(injected.into())).await;
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
        Some(*BINANCE_CLIENT_ID),
        AccountId::from("BINANCE-001"),
        nautilus_core::UUID4::new(),
        UnixNanos::default(),
        None,
        None, // correlation_id
    );

    let result = client.query_account(cmd);
    result.unwrap();

    match recv_until(&mut rx, |event| matches!(event, ExecutionEvent::Account(_))).await {
        ExecutionEvent::Account(_) => {}
        other => panic!("Expected Account event, was {other:?}"),
    }
}
