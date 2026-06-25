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

//! Integration tests for the Binance Spot execution client.

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
    body::Body,
    extract::{
        Query, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{delete, get, post},
};
use nautilus_binance::{
    common::consts::{
        BINANCE_CLIENT_ID, BINANCE_STATUS_UNKNOWN_CODE, BINANCE_UNEXPECTED_RESPONSE_CODE,
        BINANCE_VENUE,
    },
    config::BinanceExecClientConfig,
    spot::{
        execution::BinanceSpotExecutionClient,
        sbe::spot::{SBE_SCHEMA_ID, SBE_SCHEMA_VERSION},
    },
};
use nautilus_common::{
    cache::Cache,
    clients::ExecutionClient,
    live::runner::set_exec_event_sender,
    messages::{
        ExecutionEvent, ExecutionReport,
        execution::{
            BatchCancelOrders, CancelAllOrders, CancelOrder, ModifyOrder, QueryAccount, QueryOrder,
            SubmitOrder, SubmitOrderList,
        },
    },
    testing::wait_until_async,
};
use nautilus_core::UnixNanos;
use nautilus_live::ExecutionClientCore;
use nautilus_model::{
    accounts::{AccountAny, CashAccount},
    enums::{AccountType, ContingencyType, OmsType, OrderSide, TimeInForce, TriggerType},
    events::{AccountState, OrderEventAny},
    identifiers::{
        AccountId, ClientOrderId, InstrumentId, OrderListId, StrategyId, TraderId, VenueOrderId,
    },
    orders::{LimitOrder, Order, OrderAny, OrderList, StopLimitOrder},
    types::{AccountBalance, Money, Price, Quantity},
};
use nautilus_network::http::HttpClient;
use rstest::rstest;
use serde_json::json;

// SBE template IDs and block lengths from Binance schema
const PING_TEMPLATE_ID: u16 = 101;
const EXCHANGE_INFO_TEMPLATE_ID: u16 = 103;
const NEW_ORDER_FULL_TEMPLATE_ID: u16 = 302;
const CANCEL_ORDER_TEMPLATE_ID: u16 = 305;
const CANCEL_OPEN_ORDERS_TEMPLATE_ID: u16 = 306;
const ACCOUNT_TEMPLATE_ID: u16 = 400;
const ORDERS_TEMPLATE_ID: u16 = 308;
const SYMBOL_BLOCK_LENGTH: u16 = 19;
const ACCOUNT_BLOCK_LENGTH: u16 = 64;
const BALANCE_BLOCK_LENGTH: u16 = 17;
const NEW_ORDER_FULL_BLOCK_LENGTH: u16 = 153;
const CANCEL_ORDER_BLOCK_LENGTH: u16 = 137;
const ORDERS_GROUP_BLOCK_LENGTH: u16 = 162;
const PRICE_FILTER_TEMPLATE_ID: u16 = 1;
const LOT_SIZE_FILTER_TEMPLATE_ID: u16 = 4;

fn create_sbe_header(block_length: u16, template_id: u16) -> [u8; 8] {
    let mut header = [0u8; 8];
    header[0..2].copy_from_slice(&block_length.to_le_bytes());
    header[2..4].copy_from_slice(&template_id.to_le_bytes());
    header[4..6].copy_from_slice(&SBE_SCHEMA_ID.to_le_bytes());
    header[6..8].copy_from_slice(&SBE_SCHEMA_VERSION.to_le_bytes());
    header
}

fn create_group_header(block_length: u16, count: u32) -> [u8; 6] {
    let mut header = [0u8; 6];
    header[0..2].copy_from_slice(&block_length.to_le_bytes());
    header[2..6].copy_from_slice(&count.to_le_bytes());
    header
}

fn write_var_string(buf: &mut Vec<u8>, s: &str) {
    buf.push(s.len() as u8);
    buf.extend_from_slice(s.as_bytes());
}

fn write_var_bytes(buf: &mut Vec<u8>, data: &[u8]) {
    buf.push(data.len() as u8);
    buf.extend_from_slice(data);
}

fn build_ping_response() -> Vec<u8> {
    create_sbe_header(0, PING_TEMPLATE_ID).to_vec()
}

fn build_sbe_price_filter(exponent: i8, min_price: i64, max_price: i64, tick_size: i64) -> Vec<u8> {
    let mut buf = Vec::new();
    buf.extend_from_slice(&25u16.to_le_bytes());
    buf.extend_from_slice(&PRICE_FILTER_TEMPLATE_ID.to_le_bytes());
    buf.extend_from_slice(&SBE_SCHEMA_ID.to_le_bytes());
    buf.extend_from_slice(&SBE_SCHEMA_VERSION.to_le_bytes());
    buf.push(exponent as u8);
    buf.extend_from_slice(&min_price.to_le_bytes());
    buf.extend_from_slice(&max_price.to_le_bytes());
    buf.extend_from_slice(&tick_size.to_le_bytes());
    buf
}

fn build_sbe_lot_size_filter(exponent: i8, min_qty: i64, max_qty: i64, step_size: i64) -> Vec<u8> {
    let mut buf = Vec::new();
    buf.extend_from_slice(&25u16.to_le_bytes());
    buf.extend_from_slice(&LOT_SIZE_FILTER_TEMPLATE_ID.to_le_bytes());
    buf.extend_from_slice(&SBE_SCHEMA_ID.to_le_bytes());
    buf.extend_from_slice(&SBE_SCHEMA_VERSION.to_le_bytes());
    buf.push(exponent as u8);
    buf.extend_from_slice(&min_qty.to_le_bytes());
    buf.extend_from_slice(&max_qty.to_le_bytes());
    buf.extend_from_slice(&step_size.to_le_bytes());
    buf
}

fn build_exchange_info_response(symbols: &[(&str, &str, &str)]) -> Vec<u8> {
    let header = create_sbe_header(0, EXCHANGE_INFO_TEMPLATE_ID);
    let mut buf = Vec::new();
    buf.extend_from_slice(&header);

    // Empty rate_limits group
    buf.extend_from_slice(&create_group_header(11, 0));

    // Empty exchange_filters group
    buf.extend_from_slice(&create_group_header(0, 0));

    // Symbols group
    buf.extend_from_slice(&create_group_header(
        SYMBOL_BLOCK_LENGTH,
        symbols.len() as u32,
    ));

    for (symbol, base, quote) in symbols {
        buf.push(0); // status (Trading)
        buf.push(8); // base_asset_precision
        buf.push(8); // quote_asset_precision
        buf.push(8); // base_commission_precision
        buf.push(8); // quote_commission_precision
        buf.extend_from_slice(&0b0000_0111u16.to_le_bytes()); // order_types
        buf.push(1); // iceberg_allowed
        buf.push(1); // oco_allowed
        buf.push(0); // oto_allowed
        buf.push(1); // quote_order_qty_market_allowed
        buf.push(1); // allow_trailing_stop
        buf.push(1); // cancel_replace_allowed
        buf.push(0); // amend_allowed
        buf.push(1); // is_spot_trading_allowed
        buf.push(0); // is_margin_trading_allowed
        buf.push(0); // default_self_trade_prevention_mode
        buf.push(0); // allowed_self_trade_prevention_modes
        buf.push(0); // peg_instructions_allowed

        // Filters nested group
        buf.extend_from_slice(&create_group_header(0, 2));
        let price_filter = build_sbe_price_filter(-2, 1, 10_000_000, 1);
        write_var_bytes(&mut buf, &price_filter);
        let lot_filter = build_sbe_lot_size_filter(-5, 1, 900_000_000, 1);
        write_var_bytes(&mut buf, &lot_filter);

        // Empty permission sets
        buf.extend_from_slice(&create_group_header(0, 0));

        write_var_string(&mut buf, symbol);
        write_var_string(&mut buf, base);
        write_var_string(&mut buf, quote);
    }

    buf
}

fn build_account_response(balances: &[(&str, i64, i64)]) -> Vec<u8> {
    let header = create_sbe_header(ACCOUNT_BLOCK_LENGTH, ACCOUNT_TEMPLATE_ID);
    let mut buf = Vec::new();
    buf.extend_from_slice(&header);

    buf.push((-8i8) as u8); // commission_exponent
    buf.extend_from_slice(&100i64.to_le_bytes()); // maker_commission
    buf.extend_from_slice(&100i64.to_le_bytes()); // taker_commission
    buf.extend_from_slice(&100i64.to_le_bytes()); // buyer_commission
    buf.extend_from_slice(&100i64.to_le_bytes()); // seller_commission
    buf.push(1); // can_trade
    buf.push(1); // can_withdraw
    buf.push(1); // can_deposit
    buf.push(0); // brokered
    buf.push(0); // require_self_trade_prevention
    buf.push(0); // prevent_sor
    buf.extend_from_slice(&1734300000000i64.to_le_bytes()); // update_time
    buf.push(1); // account_type (SPOT)

    while buf.len() < 8 + ACCOUNT_BLOCK_LENGTH as usize {
        buf.push(0);
    }

    buf.extend_from_slice(&create_group_header(
        BALANCE_BLOCK_LENGTH,
        balances.len() as u32,
    ));

    for (asset, free, locked) in balances {
        buf.push((-8i8) as u8);
        buf.extend_from_slice(&free.to_le_bytes());
        buf.extend_from_slice(&locked.to_le_bytes());
        write_var_string(&mut buf, asset);
    }

    buf
}

fn build_new_order_response(
    order_id: i64,
    symbol: &str,
    client_order_id: &str,
    price: i64,
    qty: i64,
    executed_qty: i64,
    status: u8,
) -> Vec<u8> {
    let header = create_sbe_header(NEW_ORDER_FULL_BLOCK_LENGTH, NEW_ORDER_FULL_TEMPLATE_ID);
    let mut buf = Vec::new();
    buf.extend_from_slice(&header);

    buf.push((-8i8) as u8); // price_exponent
    buf.push((-8i8) as u8); // qty_exponent
    buf.extend_from_slice(&order_id.to_le_bytes());
    buf.extend_from_slice(&i64::MIN.to_le_bytes()); // order_list_id (None)
    buf.extend_from_slice(&1734300000000i64.to_le_bytes()); // transact_time
    buf.extend_from_slice(&price.to_le_bytes());
    buf.extend_from_slice(&qty.to_le_bytes());
    buf.extend_from_slice(&executed_qty.to_le_bytes());
    buf.extend_from_slice(&(price * executed_qty).to_le_bytes()); // cummulative_quote_qty
    buf.push(status);
    buf.push(1); // time_in_force (GTC)
    buf.push(1); // order_type (LIMIT)
    buf.push(1); // side (BUY)
    buf.extend_from_slice(&i64::MIN.to_le_bytes()); // stop_price (None)
    buf.extend_from_slice(&[0u8; 16]); // trailing_delta + trailing_time
    buf.extend_from_slice(&1734300000000i64.to_le_bytes()); // working_time
    buf.extend_from_slice(&[0u8; 23]); // iceberg to used_sor
    buf.push(0); // self_trade_prevention_mode
    buf.extend_from_slice(&[0u8; 16]); // trade_group_id + prevented_quantity
    buf.push((-8i8) as u8); // commission_exponent
    buf.extend_from_slice(&[0u8; 18]); // padding

    // Empty fills group
    buf.extend_from_slice(&create_group_header(42, 0));

    // Empty prevented matches group
    buf.extend_from_slice(&create_group_header(40, 0));

    write_var_string(&mut buf, symbol);
    write_var_string(&mut buf, client_order_id);

    buf
}

fn build_cancel_order_response(
    order_id: i64,
    symbol: &str,
    client_order_id: &str,
    orig_client_order_id: &str,
    price: i64,
    qty: i64,
    executed_qty: i64,
) -> Vec<u8> {
    let header = create_sbe_header(CANCEL_ORDER_BLOCK_LENGTH, CANCEL_ORDER_TEMPLATE_ID);
    let mut buf = Vec::new();
    buf.extend_from_slice(&header);

    buf.push((-8i8) as u8); // price_exponent
    buf.push((-8i8) as u8); // qty_exponent
    buf.extend_from_slice(&order_id.to_le_bytes());
    buf.extend_from_slice(&i64::MIN.to_le_bytes()); // order_list_id (None)
    buf.extend_from_slice(&1734300000000i64.to_le_bytes()); // transact_time
    buf.extend_from_slice(&price.to_le_bytes());
    buf.extend_from_slice(&qty.to_le_bytes());
    buf.extend_from_slice(&executed_qty.to_le_bytes());
    buf.extend_from_slice(&(price * executed_qty).to_le_bytes()); // cummulative_quote_qty
    buf.push(4); // status (CANCELED)
    buf.push(1); // time_in_force (GTC)
    buf.push(1); // order_type (LIMIT)
    buf.push(1); // side (BUY)
    buf.push(0); // self_trade_prevention_mode

    let current_len = buf.len() - 8;
    buf.extend_from_slice(&vec![0u8; CANCEL_ORDER_BLOCK_LENGTH as usize - current_len]);

    write_var_string(&mut buf, symbol);
    write_var_string(&mut buf, orig_client_order_id);
    write_var_string(&mut buf, client_order_id);

    buf
}

fn build_cancel_open_orders_response(orders: &[(i64, &str, &str, &str, i64, i64)]) -> Vec<u8> {
    let header = create_sbe_header(0, CANCEL_OPEN_ORDERS_TEMPLATE_ID);
    let mut buf = Vec::new();
    buf.extend_from_slice(&header);

    buf.extend_from_slice(&create_group_header(0, orders.len() as u32));

    for (order_id, symbol, client_order_id, orig_client_order_id, price, qty) in orders {
        let embedded = build_cancel_order_response(
            *order_id,
            symbol,
            client_order_id,
            orig_client_order_id,
            *price,
            *qty,
            0,
        );
        buf.extend_from_slice(&(embedded.len() as u16).to_le_bytes());
        buf.extend_from_slice(&embedded);
    }

    buf
}

fn build_orders_response(orders: &[(i64, &str, &str, i64, i64)]) -> Vec<u8> {
    let header = create_sbe_header(0, ORDERS_TEMPLATE_ID);
    let mut buf = Vec::new();
    buf.extend_from_slice(&header);

    buf.extend_from_slice(&create_group_header(
        ORDERS_GROUP_BLOCK_LENGTH,
        orders.len() as u32,
    ));

    for (order_id, symbol, client_order_id, price, qty) in orders {
        let order_start = buf.len();

        buf.push((-8i8) as u8); // price_exponent
        buf.push((-8i8) as u8); // qty_exponent
        buf.extend_from_slice(&order_id.to_le_bytes());
        buf.extend_from_slice(&i64::MIN.to_le_bytes()); // order_list_id (None)
        buf.extend_from_slice(&price.to_le_bytes());
        buf.extend_from_slice(&qty.to_le_bytes());
        buf.extend_from_slice(&0i64.to_le_bytes()); // executed_qty
        buf.extend_from_slice(&0i64.to_le_bytes()); // cummulative_quote_qty
        buf.push(1); // status (NEW)
        buf.push(1); // time_in_force (GTC)
        buf.push(1); // order_type (LIMIT)
        buf.push(1); // side (BUY)
        buf.extend_from_slice(&i64::MIN.to_le_bytes()); // stop_price (None)
        buf.extend_from_slice(&[0u8; 16]); // trailing_delta + trailing_time
        buf.extend_from_slice(&i64::MIN.to_le_bytes()); // iceberg_qty (None)
        buf.extend_from_slice(&1734300000000i64.to_le_bytes()); // time
        buf.extend_from_slice(&1734300000000i64.to_le_bytes()); // update_time
        buf.push(1); // is_working
        buf.extend_from_slice(&1734300000000i64.to_le_bytes()); // working_time
        buf.extend_from_slice(&0i64.to_le_bytes()); // orig_quote_order_qty

        while buf.len() - order_start < ORDERS_GROUP_BLOCK_LENGTH as usize {
            buf.push(0);
        }

        write_var_string(&mut buf, symbol);
        write_var_string(&mut buf, client_order_id);
    }

    buf
}

fn has_auth_headers(headers: &HeaderMap) -> bool {
    headers.contains_key("x-mbx-apikey")
}

fn sbe_response(body: Vec<u8>) -> impl IntoResponse {
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "application/sbe")],
        Body::from(body),
    )
}

fn unauthorized_response() -> impl IntoResponse {
    (
        StatusCode::UNAUTHORIZED,
        [(header::CONTENT_TYPE, "application/json")],
        Body::from(r#"{"code":-2015,"msg":"Invalid API-key, IP, or permissions for action"}"#),
    )
}

fn no_such_order_response() -> impl IntoResponse {
    (
        StatusCode::BAD_REQUEST,
        [(header::CONTENT_TYPE, "application/json")],
        Body::from(r#"{"code":-2013,"msg":"Order does not exist."}"#),
    )
}

fn json_response(body: &serde_json::Value) -> Response {
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "application/json")],
        body.to_string(),
    )
        .into_response()
}

fn ambiguous_failure_response() -> Response {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        [(header::CONTENT_TYPE, "text/plain")],
        "temporary gateway failure",
    )
        .into_response()
}

fn venue_reject_response(code: i64, msg: &str) -> Response {
    (
        StatusCode::BAD_REQUEST,
        [(header::CONTENT_TYPE, "application/json")],
        json!({"code": code, "msg": msg}).to_string(),
    )
        .into_response()
}

fn command_response(response: CommandResponse, success: impl IntoResponse) -> Response {
    match response {
        CommandResponse::Success => success.into_response(),
        CommandResponse::AmbiguousFailure => ambiguous_failure_response(),
        CommandResponse::MalformedSuccess => (
            StatusCode::OK,
            [(header::CONTENT_TYPE, "application/sbe")],
            Body::from(vec![0_u8; 4]),
        )
            .into_response(),
        CommandResponse::BatchPerOrderReject { code, msg } => {
            json_response(&json!([{"code": code, "msg": msg}]))
        }
        CommandResponse::VenueReject { code, msg } => venue_reject_response(code, msg),
    }
}

#[derive(Clone, Copy)]
enum CommandResponse {
    Success,
    AmbiguousFailure,
    MalformedSuccess,
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
}

#[derive(Clone, Copy)]
enum WsSetupBehavior {
    CompleteSetup,
    RejectFirstSessionLogon,
    RejectSessionLogon,
    IgnoreSessionLogon,
    RejectUserDataSubscribe,
}

#[derive(Clone)]
struct WsSetupState {
    behavior: WsSetupBehavior,
    received_methods: Arc<tokio::sync::Mutex<Vec<String>>>,
    session_logon_rejections: Arc<AtomicUsize>,
}

impl WsSetupState {
    fn new(behavior: WsSetupBehavior) -> Self {
        Self {
            behavior,
            received_methods: Arc::new(tokio::sync::Mutex::new(Vec::new())),
            session_logon_rejections: Arc::new(AtomicUsize::new(0)),
        }
    }

    async fn received_methods(&self) -> Vec<String> {
        self.received_methods.lock().await.clone()
    }
}

fn create_exec_test_router(order_query_count: Option<Arc<AtomicUsize>>) -> Router {
    let order_query_count_for_order_route = order_query_count;

    Router::new()
        .route(
            "/api/v3/ping",
            get(|| async { sbe_response(build_ping_response()).into_response() }),
        )
        .route(
            "/api/v3/order/cancelReplace",
            post(
                |headers: HeaderMap, Query(params): Query<HashMap<String, String>>| async move {
                    if !has_auth_headers(&headers) {
                        return unauthorized_response().into_response();
                    }
                    let symbol = params
                        .get("symbol")
                        .cloned()
                        .unwrap_or_else(|| "BTCUSDT".to_string());
                    let client_order_id = params
                        .get("newClientOrderId")
                        .cloned()
                        .unwrap_or_else(|| "replace-order".to_string());
                    sbe_response(build_new_order_response(
                        99998,
                        &symbol,
                        &client_order_id,
                        100_000_000_000,
                        10_000_000,
                        0,
                        1, // NEW
                    ))
                    .into_response()
                },
            ),
        )
        .route(
            "/api/v3/exchangeInfo",
            get(|| async {
                let symbols = vec![("BTCUSDT", "BTC", "USDT")];
                sbe_response(build_exchange_info_response(&symbols)).into_response()
            }),
        )
        .route(
            "/api/v3/account",
            get(|headers: HeaderMap| async move {
                if !has_auth_headers(&headers) {
                    return unauthorized_response().into_response();
                }
                let balances = vec![
                    ("BTC", 100_000_000i64, 0i64),
                    ("USDT", 10_000_000_000_000i64, 0i64),
                ];
                sbe_response(build_account_response(&balances)).into_response()
            }),
        )
        .route(
            "/api/v3/openOrders",
            get(|headers: HeaderMap| async move {
                if !has_auth_headers(&headers) {
                    return unauthorized_response().into_response();
                }
                let orders: Vec<(i64, &str, &str, i64, i64)> = vec![];
                sbe_response(build_orders_response(&orders)).into_response()
            })
            .delete(
                |headers: HeaderMap, Query(params): Query<HashMap<String, String>>| async move {
                    if !has_auth_headers(&headers) {
                        return unauthorized_response().into_response();
                    }
                    let symbol = params
                        .get("symbol")
                        .cloned()
                        .unwrap_or_else(|| "BTCUSDT".to_string());
                    let orders = vec![(
                        12345i64,
                        symbol.as_str(),
                        "cancel-1",
                        "order-1",
                        100_000_000_000i64,
                        10_000_000i64,
                    )];
                    sbe_response(build_cancel_open_orders_response(&orders)).into_response()
                },
            ),
        )
        .route(
            "/api/v3/order",
            post(
                |headers: HeaderMap, Query(params): Query<HashMap<String, String>>| async move {
                    if !has_auth_headers(&headers) {
                        return unauthorized_response().into_response();
                    }
                    let symbol = params
                        .get("symbol")
                        .cloned()
                        .unwrap_or_else(|| "BTCUSDT".to_string());
                    let client_order_id = params
                        .get("newClientOrderId")
                        .cloned()
                        .unwrap_or_else(|| "test-order".to_string());
                    sbe_response(build_new_order_response(
                        99999,
                        &symbol,
                        &client_order_id,
                        100_000_000_000,
                        10_000_000,
                        0,
                        1, // NEW
                    ))
                    .into_response()
                },
            )
            .get(move |headers: HeaderMap| {
                let order_query_count = order_query_count_for_order_route.clone();
                async move {
                    if !has_auth_headers(&headers) {
                        return unauthorized_response().into_response();
                    }

                    if let Some(count) = order_query_count {
                        count.fetch_add(1, Ordering::SeqCst);
                    }

                    no_such_order_response().into_response()
                }
            })
            .delete(
                |headers: HeaderMap, Query(params): Query<HashMap<String, String>>| async move {
                    if !has_auth_headers(&headers) {
                        return unauthorized_response().into_response();
                    }
                    let symbol = params
                        .get("symbol")
                        .cloned()
                        .unwrap_or_else(|| "BTCUSDT".to_string());
                    let order_id = params
                        .get("orderId")
                        .and_then(|s| s.parse().ok())
                        .unwrap_or(12345);
                    let orig_client_order_id = params
                        .get("origClientOrderId")
                        .cloned()
                        .unwrap_or_else(|| "orig-order".to_string());
                    sbe_response(build_cancel_order_response(
                        order_id,
                        &symbol,
                        "cancel-req",
                        &orig_client_order_id,
                        100_000_000_000,
                        10_000_000,
                        0,
                    ))
                    .into_response()
                },
            ),
        )
}

fn create_exec_test_router_with_command_responses(state: CommandResponseState) -> Router {
    Router::new()
        .route(
            "/api/v3/ping",
            get(|| async { sbe_response(build_ping_response()).into_response() }),
        )
        .route(
            "/api/v3/exchangeInfo",
            get(|| async {
                let symbols = vec![("BTCUSDT", "BTC", "USDT")];
                sbe_response(build_exchange_info_response(&symbols)).into_response()
            }),
        )
        .route(
            "/api/v3/account",
            get(|headers: HeaderMap| async move {
                if !has_auth_headers(&headers) {
                    return unauthorized_response().into_response();
                }
                let balances = vec![
                    ("BTC", 100_000_000i64, 0i64),
                    ("USDT", 10_000_000_000_000i64, 0i64),
                ];
                sbe_response(build_account_response(&balances)).into_response()
            }),
        )
        .route(
            "/api/v3/openOrders",
            get(|headers: HeaderMap| async move {
                if !has_auth_headers(&headers) {
                    return unauthorized_response().into_response();
                }
                let orders: Vec<(i64, &str, &str, i64, i64)> = vec![];
                sbe_response(build_orders_response(&orders)).into_response()
            }),
        )
        .route("/api/v3/order/cancelReplace", post(handle_order_modify))
        .route(
            "/api/v3/order",
            post(handle_order_submit)
                .get(|headers: HeaderMap| async move {
                    if !has_auth_headers(&headers) {
                        return unauthorized_response().into_response();
                    }
                    no_such_order_response().into_response()
                })
                .delete(handle_order_cancel),
        )
        .route("/api/v3/orderList/oco", post(handle_oco_order_list_submit))
        .route("/api/v3/batchOrders", delete(handle_batch_cancel))
        .with_state(state)
}

async fn handle_order_submit(
    State(state): State<CommandResponseState>,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
) -> Response {
    if !has_auth_headers(&headers) {
        return unauthorized_response().into_response();
    }
    state.request_count.fetch_add(1, Ordering::Relaxed);
    let symbol = params
        .get("symbol")
        .cloned()
        .unwrap_or_else(|| "BTCUSDT".to_string());
    let client_order_id = params
        .get("newClientOrderId")
        .cloned()
        .unwrap_or_else(|| "test-order".to_string());
    command_response(
        state.responses.submit,
        sbe_response(build_new_order_response(
            99999,
            &symbol,
            &client_order_id,
            100_000_000_000,
            10_000_000,
            0,
            1,
        )),
    )
}

async fn handle_order_cancel(
    State(state): State<CommandResponseState>,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
) -> Response {
    if !has_auth_headers(&headers) {
        return unauthorized_response().into_response();
    }
    state.request_count.fetch_add(1, Ordering::Relaxed);
    let symbol = params
        .get("symbol")
        .cloned()
        .unwrap_or_else(|| "BTCUSDT".to_string());
    let order_id = params
        .get("orderId")
        .and_then(|s| s.parse().ok())
        .unwrap_or(12345);
    let orig_client_order_id = params
        .get("origClientOrderId")
        .cloned()
        .unwrap_or_else(|| "orig-order".to_string());
    command_response(
        state.responses.cancel,
        sbe_response(build_cancel_order_response(
            order_id,
            &symbol,
            "cancel-req",
            &orig_client_order_id,
            100_000_000_000,
            10_000_000,
            0,
        )),
    )
}

async fn handle_order_modify(
    State(state): State<CommandResponseState>,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
) -> Response {
    if !has_auth_headers(&headers) {
        return unauthorized_response().into_response();
    }
    state.request_count.fetch_add(1, Ordering::Relaxed);
    let symbol = params
        .get("symbol")
        .cloned()
        .unwrap_or_else(|| "BTCUSDT".to_string());
    let client_order_id = params
        .get("newClientOrderId")
        .cloned()
        .unwrap_or_else(|| "replace-order".to_string());
    command_response(
        state.responses.modify,
        sbe_response(build_new_order_response(
            99998,
            &symbol,
            &client_order_id,
            100_000_000_000,
            10_000_000,
            0,
            1,
        )),
    )
}

async fn handle_batch_cancel(
    State(state): State<CommandResponseState>,
    headers: HeaderMap,
) -> Response {
    if !has_auth_headers(&headers) {
        return unauthorized_response().into_response();
    }
    state.request_count.fetch_add(1, Ordering::Relaxed);
    command_response(state.responses.batch_cancel, json_response(&json!([])))
}

async fn handle_oco_order_list_submit(
    State(state): State<CommandResponseState>,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
) -> Response {
    if !has_auth_headers(&headers) {
        return unauthorized_response().into_response();
    }
    state.request_count.fetch_add(1, Ordering::Relaxed);

    let valid_oco = params.get("symbol").is_some_and(|value| value == "BTCUSDT")
        && params.get("side").is_some_and(|value| value == "SELL")
        && params.get("quantity").is_some_and(|value| value == "0.001")
        && params
            .get("aboveType")
            .is_some_and(|value| value == "LIMIT_MAKER")
        && params
            .get("belowType")
            .is_some_and(|value| value == "STOP_LOSS_LIMIT")
        && params
            .get("aboveClientOrderId")
            .is_some_and(|value| value.contains("spot-oco-tp"))
        && params
            .get("belowClientOrderId")
            .is_some_and(|value| value.contains("spot-oco-sl"));

    if !valid_oco {
        return venue_reject_response(-1102, "invalid OCO test request");
    }

    let above_client_order_id = params
        .get("aboveClientOrderId")
        .cloned()
        .unwrap_or_else(|| "above".to_string());
    let below_client_order_id = params
        .get("belowClientOrderId")
        .cloned()
        .unwrap_or_else(|| "below".to_string());
    let list_client_order_id = params
        .get("listClientOrderId")
        .cloned()
        .unwrap_or_else(|| "list".to_string());

    command_response(
        state.responses.submit,
        json_response(&json!({
            "orderListId": 42,
            "contingencyType": "OCO",
            "listStatusType": "EXEC_STARTED",
            "listOrderStatus": "EXECUTING",
            "listClientOrderId": list_client_order_id,
            "transactionTime": 1710485608839_i64,
            "symbol": "BTCUSDT",
            "orders": [
                {
                    "symbol": "BTCUSDT",
                    "orderId": 1001,
                    "clientOrderId": above_client_order_id
                },
                {
                    "symbol": "BTCUSDT",
                    "orderId": 1002,
                    "clientOrderId": below_client_order_id
                }
            ]
        })),
    )
}

async fn handle_ws_setup(ws: WebSocketUpgrade, State(state): State<WsSetupState>) -> Response {
    ws.on_upgrade(|socket| handle_ws_setup_socket(socket, state))
}

async fn handle_ws_setup_socket(mut socket: WebSocket, state: WsSetupState) {
    while let Some(Ok(msg)) = socket.recv().await {
        match msg {
            Message::Text(text) => {
                let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
                    continue;
                };
                let method = value
                    .get("method")
                    .and_then(|value| value.as_str())
                    .unwrap_or_default();
                let request_id = value
                    .get("id")
                    .and_then(|value| value.as_str())
                    .unwrap_or_default();

                state.received_methods.lock().await.push(method.to_string());

                match (method, state.behavior) {
                    ("session.logon", WsSetupBehavior::RejectFirstSessionLogon) => {
                        if state
                            .session_logon_rejections
                            .fetch_add(1, Ordering::Relaxed)
                            == 0
                        {
                            if send_ws_setup_error(&mut socket, request_id, -2015, "auth rejected")
                                .await
                                .is_err()
                            {
                                break;
                            }
                        } else if send_ws_setup_result(&mut socket, request_id, json!({}))
                            .await
                            .is_err()
                        {
                            break;
                        }
                    }
                    ("session.logon", WsSetupBehavior::RejectSessionLogon) => {
                        if send_ws_setup_error(&mut socket, request_id, -2015, "auth rejected")
                            .await
                            .is_err()
                        {
                            break;
                        }
                    }
                    ("session.logon", WsSetupBehavior::IgnoreSessionLogon) => {}
                    ("session.logon", _) => {
                        if send_ws_setup_result(&mut socket, request_id, json!({}))
                            .await
                            .is_err()
                        {
                            break;
                        }
                    }
                    ("userDataStream.subscribe", WsSetupBehavior::RejectUserDataSubscribe) => {
                        if send_ws_setup_error(&mut socket, request_id, -1000, "subscribe rejected")
                            .await
                            .is_err()
                        {
                            break;
                        }
                    }
                    ("userDataStream.subscribe", _) => {
                        match send_ws_setup_result(
                            &mut socket,
                            request_id,
                            json!({"subscriptionId": 1}),
                        )
                        .await
                        {
                            Ok(()) => {}
                            Err(_) => break,
                        }
                    }
                    _ => {}
                }
            }
            Message::Ping(payload) => {
                if socket.send(Message::Pong(payload)).await.is_err() {
                    break;
                }
            }
            Message::Close(_) => break,
            _ => {}
        }
    }
}

async fn send_ws_setup_result(
    socket: &mut WebSocket,
    request_id: &str,
    result: serde_json::Value,
) -> Result<(), axum::Error> {
    socket
        .send(Message::Text(
            json!({"id": request_id, "result": result})
                .to_string()
                .into(),
        ))
        .await
}

async fn send_ws_setup_error(
    socket: &mut WebSocket,
    request_id: &str,
    code: i64,
    msg: &str,
) -> Result<(), axum::Error> {
    socket
        .send(Message::Text(
            json!({"id": request_id, "error": {"code": code, "msg": msg}})
                .to_string()
                .into(),
        ))
        .await
}

async fn start_exec_test_server() -> SocketAddr {
    start_exec_test_server_with_order_query_count(None).await
}

async fn start_exec_test_server_with_order_query_count(
    order_query_count: Option<Arc<AtomicUsize>>,
) -> SocketAddr {
    let router = create_exec_test_router(order_query_count);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        axum::serve(listener, router.into_make_service())
            .await
            .unwrap();
    });

    // Wait for server to be ready
    let health_url = format!("http://{addr}/api/v3/ping");
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
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        axum::serve(listener, router.into_make_service())
            .await
            .unwrap();
    });

    let health_url = format!("http://{addr}/api/v3/ping");
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

async fn start_ws_setup_test_server(behavior: WsSetupBehavior) -> (SocketAddr, WsSetupState) {
    let state = WsSetupState::new(behavior);
    let router = Router::new()
        .route("/ws-api/v3", get(handle_ws_setup))
        .with_state(state.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        axum::serve(listener, router.into_make_service())
            .await
            .unwrap();
    });

    (addr, state)
}

fn create_test_execution_client(
    base_url: String,
) -> (
    BinanceSpotExecutionClient,
    tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
    Rc<RefCell<Cache>>,
) {
    create_test_execution_client_with_transport(base_url, false, None)
}

fn create_test_execution_client_with_ws_trading(
    base_url_http: String,
    base_url_ws_trading: String,
) -> (
    BinanceSpotExecutionClient,
    tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
    Rc<RefCell<Cache>>,
) {
    create_test_execution_client_with_transport(base_url_http, true, Some(base_url_ws_trading))
}

fn create_test_execution_client_with_transport(
    base_url_http: String,
    use_ws_trading: bool,
    base_url_ws_trading: Option<String>,
) -> (
    BinanceSpotExecutionClient,
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
        AccountType::Cash,
        None,
        cache.clone(),
    );

    let config = BinanceExecClientConfig {
        trader_id,
        account_id,
        base_url_http: Some(base_url_http),
        base_url_ws_trading,
        use_ws_trading,
        api_key: Some("test_api_key".to_string()),
        api_secret: Some("test_api_secret".to_string()),
        ..Default::default()
    };

    // Set up event channel (must be set before creating client)
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    set_exec_event_sender(tx);

    let client = BinanceSpotExecutionClient::new(core, config).unwrap();

    (client, rx, cache)
}

fn add_test_account_to_cache(cache: &Rc<RefCell<Cache>>, account_id: AccountId) {
    let account_state = AccountState::new(
        account_id,
        AccountType::Cash,
        vec![AccountBalance::new(
            Money::from("1.0 BTC"),
            Money::from("0 BTC"),
            Money::from("1.0 BTC"),
        )],
        vec![],
        true,
        nautilus_core::UUID4::new(),
        UnixNanos::default(),
        UnixNanos::default(),
        None,
    );

    let account = AccountAny::Cash(CashAccount::new(account_state, true, false));
    cache.borrow_mut().add_account(account).unwrap();
}

#[rstest]
#[tokio::test]
async fn test_client_creation() {
    let addr = start_exec_test_server().await;
    let base_url = format!("http://{addr}");

    let (client, _rx, _cache) = create_test_execution_client(base_url);

    assert_eq!(client.client_id(), *BINANCE_CLIENT_ID);
    assert_eq!(client.venue(), *BINANCE_VENUE);
    assert_eq!(client.oms_type(), OmsType::Hedging);
    assert!(!client.is_connected());
}

#[rstest]
#[tokio::test]
async fn test_connect_loads_instruments_and_account() {
    let addr = start_exec_test_server().await;
    let base_url = format!("http://{addr}");

    let (mut client, _rx, cache) = create_test_execution_client(base_url);

    // Pre-populate cache with account (simulates what runner does)
    add_test_account_to_cache(&cache, AccountId::from("BINANCE-001"));

    client.connect().await.unwrap();

    assert!(client.is_connected());
}

#[rstest]
#[tokio::test]
async fn test_ws_trading_success_routes_orders_over_ws() {
    let (http_addr, request_count) =
        start_exec_test_server_with_command_responses(CommandResponses::default()).await;
    let (ws_addr, ws_state) = start_ws_setup_test_server(WsSetupBehavior::CompleteSetup).await;
    let base_url_http = format!("http://{http_addr}");
    let base_url_ws_trading = format!("ws://{ws_addr}/ws-api/v3");

    let (mut client, _rx, cache) =
        create_test_execution_client_with_ws_trading(base_url_http, base_url_ws_trading);
    add_test_account_to_cache(&cache, AccountId::from("BINANCE-001"));

    client.start().unwrap();
    client.connect().await.unwrap();
    assert!(client.is_connected());

    let client_order_id = ClientOrderId::new("ws-setup-success-test-001");
    let order_any = add_limit_order_to_cache(&cache, client_order_id);

    client
        .submit_order(submit_order_command(&order_any))
        .unwrap();

    wait_for_ws_method(&ws_state, "order.place").await;

    assert_eq!(request_count.load(Ordering::Relaxed), 0);
    assert_eq!(
        ws_state.received_methods().await,
        ["session.logon", "userDataStream.subscribe", "order.place"]
            .into_iter()
            .map(str::to_string)
            .collect::<Vec<_>>()
    );

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_ws_trading_reconnect_retries_ws_after_setup_failure() {
    let (http_addr, request_count) =
        start_exec_test_server_with_command_responses(CommandResponses::default()).await;
    let (ws_addr, ws_state) =
        start_ws_setup_test_server(WsSetupBehavior::RejectFirstSessionLogon).await;
    let base_url_http = format!("http://{http_addr}");
    let base_url_ws_trading = format!("ws://{ws_addr}/ws-api/v3");

    let (mut client, _rx, cache) =
        create_test_execution_client_with_ws_trading(base_url_http, base_url_ws_trading);
    add_test_account_to_cache(&cache, AccountId::from("BINANCE-001"));

    client.start().unwrap();
    client.connect().await.unwrap();
    assert!(client.is_connected());

    let fallback_order_id = ClientOrderId::new("ws-setup-retry-fallback-001");
    let fallback_order = add_limit_order_to_cache(&cache, fallback_order_id);

    client
        .submit_order(submit_order_command(&fallback_order))
        .unwrap();

    wait_for_command_requests(&request_count, 1).await;

    client.disconnect().await.unwrap();
    client.connect().await.unwrap();

    let retry_order_id = ClientOrderId::new("ws-setup-retry-success-001");
    let retry_order = add_limit_order_to_cache(&cache, retry_order_id);

    client
        .submit_order(submit_order_command(&retry_order))
        .unwrap();

    wait_for_ws_method(&ws_state, "order.place").await;

    assert_eq!(request_count.load(Ordering::Relaxed), 1);
    assert_eq!(
        ws_state.received_methods().await,
        [
            "session.logon",
            "session.logon",
            "userDataStream.subscribe",
            "order.place",
        ]
        .into_iter()
        .map(str::to_string)
        .collect::<Vec<_>>()
    );

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_ws_trading_session_logon_rejection_uses_http_only_mode() {
    assert_ws_setup_failure_uses_http(WsSetupBehavior::RejectSessionLogon, &["session.logon"])
        .await;
}

#[rstest]
#[tokio::test]
async fn test_ws_trading_auth_timeout_uses_http_only_mode() {
    assert_ws_setup_failure_uses_http(WsSetupBehavior::IgnoreSessionLogon, &["session.logon"])
        .await;
}

#[rstest]
#[tokio::test]
async fn test_ws_trading_user_data_subscribe_rejection_uses_http_only_mode() {
    assert_ws_setup_failure_uses_http(
        WsSetupBehavior::RejectUserDataSubscribe,
        &["session.logon", "userDataStream.subscribe"],
    )
    .await;
}

#[rstest]
#[tokio::test]
async fn test_disconnect_sets_state() {
    let addr = start_exec_test_server().await;
    let base_url = format!("http://{addr}");

    let (mut client, _rx, cache) = create_test_execution_client(base_url);
    add_test_account_to_cache(&cache, AccountId::from("BINANCE-001"));

    client.connect().await.unwrap();
    assert!(client.is_connected());

    client.disconnect().await.unwrap();
    assert!(!client.is_connected());
}

#[rstest]
#[tokio::test]
async fn test_submit_order_generates_submitted_and_accepted_events() {
    let addr = start_exec_test_server().await;
    let base_url = format!("http://{addr}");

    let (mut client, mut rx, cache) = create_test_execution_client(base_url);
    add_test_account_to_cache(&cache, AccountId::from("BINANCE-001"));

    client.start().unwrap();
    client.connect().await.unwrap();

    let instrument_id = InstrumentId::from("BTCUSDT.BINANCE");
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

    // SubmitOrder references by ID, so order must be in cache first
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

    wait_until_async(
        || {
            let found = rx
                .try_recv()
                .is_ok_and(|e| matches!(e, ExecutionEvent::Order(OrderEventAny::Accepted(_))));
            async move { found }
        },
        Duration::from_secs(5),
    )
    .await;
}

#[rstest]
#[tokio::test]
async fn test_submit_oco_order_list_routes_to_order_list_oco_endpoint() {
    let (client, mut rx, cache, request_count) =
        connected_client_with_command_responses(CommandResponses::default()).await;

    let orders = add_spot_oco_orders_to_cache(&cache);
    let submit_cmd = submit_order_list_command(&orders);

    client.submit_order_list(submit_cmd).unwrap();

    wait_for_command_requests(&request_count, 1).await;

    let mut accepted_count = 0;
    wait_until_async(
        || {
            while let Ok(event) = rx.try_recv() {
                if matches!(event, ExecutionEvent::Order(OrderEventAny::Accepted(_))) {
                    accepted_count += 1;
                }
            }
            let done = accepted_count == 2;
            async move { done }
        },
        Duration::from_secs(5),
    )
    .await;
}

#[rstest]
#[tokio::test]
async fn test_oco_order_list_response_parse_failure_does_not_emit_order_rejected() {
    let (client, mut rx, cache, request_count) =
        connected_client_with_command_responses(CommandResponses {
            submit: CommandResponse::MalformedSuccess,
            ..Default::default()
        })
        .await;

    let orders = add_spot_oco_orders_to_cache(&cache);
    let submit_cmd = submit_order_list_command(&orders);
    let client_order_ids = orders
        .iter()
        .map(|order| order.client_order_id())
        .collect::<Vec<_>>();

    client.submit_order_list(submit_cmd).unwrap();

    wait_for_command_requests(&request_count, 1).await;

    assert_no_order_event_matching(&mut rx, |event| {
        matches!(
            event,
            OrderEventAny::Rejected(event) if client_order_ids.contains(&event.client_order_id)
        )
    })
    .await;
}

#[rstest]
#[tokio::test]
async fn test_submit_independent_order_list_is_denied_without_http_request() {
    let (client, mut rx, cache, request_count) =
        connected_client_with_command_responses(CommandResponses::default()).await;

    let orders = [
        add_limit_order_to_cache(&cache, ClientOrderId::new("spot-batch-001")),
        add_limit_order_to_cache(&cache, ClientOrderId::new("spot-batch-002")),
    ];
    let submit_cmd = submit_order_list_command(&orders);

    client.submit_order_list(submit_cmd).unwrap();

    let mut denied_count = 0;
    wait_until_async(
        || {
            while let Ok(event) = rx.try_recv() {
                if matches!(event, ExecutionEvent::Order(OrderEventAny::Denied(_))) {
                    denied_count += 1;
                }
            }
            let done = denied_count == 2;
            async move { done }
        },
        Duration::from_secs(5),
    )
    .await;

    assert_eq!(request_count.load(Ordering::Relaxed), 0);
}

#[rstest]
#[tokio::test]
async fn test_cancel_all_orders_generates_canceled_events() {
    let addr = start_exec_test_server().await;
    let base_url = format!("http://{addr}");

    let (mut client, mut rx, cache) = create_test_execution_client(base_url);
    add_test_account_to_cache(&cache, AccountId::from("BINANCE-001"));

    client.start().unwrap();
    client.connect().await.unwrap();

    let instrument_id = InstrumentId::from("BTCUSDT.BINANCE");

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

    client.cancel_all_orders(cancel_all_cmd).unwrap();

    wait_until_async(
        || {
            let found = rx
                .try_recv()
                .is_ok_and(|e| matches!(e, ExecutionEvent::Order(OrderEventAny::Canceled(_))));
            async move { found }
        },
        Duration::from_secs(5),
    )
    .await;
}

#[rstest]
#[tokio::test]
async fn test_cancel_order_generates_canceled_event() {
    let addr = start_exec_test_server().await;
    let base_url = format!("http://{addr}");

    let (mut client, mut rx, cache) = create_test_execution_client(base_url);
    add_test_account_to_cache(&cache, AccountId::from("BINANCE-001"));

    client.start().unwrap();
    client.connect().await.unwrap();

    let instrument_id = InstrumentId::from("BTCUSDT.BINANCE");
    let client_order_id = ClientOrderId::new("cancel-test-001");
    let trader_id = TraderId::from("TESTER-001");
    let strategy_id = StrategyId::from("TEST-STRATEGY");

    // Create and cache an order first (cancel needs it in cache)
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

    client.cancel_order(cancel_cmd).unwrap();

    wait_until_async(
        || {
            let found = rx
                .try_recv()
                .is_ok_and(|e| matches!(e, ExecutionEvent::Order(OrderEventAny::Canceled(_))));
            async move { found }
        },
        Duration::from_secs(5),
    )
    .await;
}

#[rstest]
#[tokio::test]
async fn test_modify_order_generates_events() {
    let addr = start_exec_test_server().await;
    let base_url = format!("http://{addr}");

    let (mut client, mut rx, cache) = create_test_execution_client(base_url);
    add_test_account_to_cache(&cache, AccountId::from("BINANCE-001"));

    client.start().unwrap();
    client.connect().await.unwrap();

    let instrument_id = InstrumentId::from("BTCUSDT.BINANCE");
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

    // Modify uses cancel-replace on Binance Spot, which generates cancel + new events
    let result = client.modify_order(modify_cmd);
    result.unwrap();

    // Should get at least one execution event (cancel or accepted for the replacement)
    wait_until_async(
        || {
            let found = rx
                .try_recv()
                .is_ok_and(|e| matches!(e, ExecutionEvent::Order(_)));
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
async fn test_submit_response_parse_failure_does_not_emit_order_rejected() {
    let (client, mut rx, cache, request_count) =
        connected_client_with_command_responses(CommandResponses {
            submit: CommandResponse::MalformedSuccess,
            ..Default::default()
        })
        .await;

    let client_order_id = ClientOrderId::new("parse-fail-submit-test-001");
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
async fn test_local_submit_failure_emits_order_rejected() {
    let (mut client, mut rx, cache) =
        create_test_execution_client("http://127.0.0.1:1".to_string());
    client.start().unwrap();

    let client_order_id = ClientOrderId::new("local-submit-reject-test-001");
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
            assert!(event.reason.as_str().contains("not in cache"));
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
async fn test_local_cancel_failure_does_not_emit_cancel_rejected() {
    let (client, mut rx, cache, request_count) =
        connected_client_with_command_responses(CommandResponses::default()).await;

    let client_order_id = ClientOrderId::new("local-cancel-invalid-test-001");
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
async fn test_modify_response_parse_failure_does_not_emit_modify_rejected() {
    let (client, mut rx, cache, request_count) =
        connected_client_with_command_responses(CommandResponses {
            modify: CommandResponse::MalformedSuccess,
            ..Default::default()
        })
        .await;

    let client_order_id = ClientOrderId::new("parse-fail-modify-test-001");
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
                code: -2010,
                msg: "Cancel replace order failed",
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
                    .contains("Cancel replace order failed")
            );
        }
        other => panic!("Expected ModifyRejected event, was {other:?}"),
    }
}

#[rstest]
#[tokio::test]
async fn test_local_modify_failure_emits_modify_rejected() {
    let (client, mut rx, cache, _request_count) =
        connected_client_with_command_responses(CommandResponses::default()).await;

    let client_order_id = ClientOrderId::new("local-modify-reject-test-001");
    add_limit_order_to_cache(&cache, client_order_id);

    let modify_cmd = ModifyOrder::new(
        test_trader_id(),
        Some(*BINANCE_CLIENT_ID),
        test_strategy_id(),
        test_instrument_id(),
        client_order_id,
        None,
        Some(Quantity::from("0.002")),
        Some(Price::from("51000.00")),
        None,
        nautilus_core::UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );

    client.modify_order(modify_cmd).unwrap();

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
            assert!(event.reason.as_str().contains("venue_order_id required"));
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
async fn test_connect_disconnect_reconnect() {
    let addr = start_exec_test_server().await;
    let base_url = format!("http://{addr}");

    let (mut client, _rx, cache) = create_test_execution_client(base_url);
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
async fn test_query_account_does_not_block_within_runtime() {
    let addr = start_exec_test_server().await;
    let base_url = format!("http://{addr}");

    let (mut client, mut rx, cache) = create_test_execution_client(base_url);
    add_test_account_to_cache(&cache, AccountId::from("BINANCE-001"));

    client.start().unwrap();
    client.connect().await.unwrap();

    let query_cmd = QueryAccount::new(
        TraderId::from("TESTER-001"),
        Some(*BINANCE_CLIENT_ID),
        AccountId::from("BINANCE-001"),
        nautilus_core::UUID4::new(),
        UnixNanos::default(),
        None,
        None, // correlation_id
    );

    let result = client.query_account(query_cmd);
    result.unwrap();

    wait_until_async(
        || {
            let found = rx
                .try_recv()
                .is_ok_and(|e| matches!(e, ExecutionEvent::Account(_)));
            async move { found }
        },
        Duration::from_secs(5),
    )
    .await;
}

#[rstest]
#[tokio::test]
async fn test_query_order_missing_order_emits_no_order_report() {
    let order_query_count = Arc::new(AtomicUsize::new(0));
    let addr = start_exec_test_server_with_order_query_count(Some(order_query_count.clone())).await;
    let base_url = format!("http://{addr}");

    let (mut client, mut rx, cache) = create_test_execution_client(base_url);
    add_test_account_to_cache(&cache, AccountId::from("BINANCE-001"));

    client.start().unwrap();
    client.connect().await.unwrap();

    while rx.try_recv().is_ok() {}

    let query_cmd = QueryOrder::new(
        TraderId::from("TESTER-001"),
        Some(*BINANCE_CLIENT_ID),
        StrategyId::from("TEST-STRATEGY"),
        InstrumentId::from("BTCUSDT.BINANCE"),
        ClientOrderId::new("missing-order-001"),
        Some(VenueOrderId::from("99999")),
        nautilus_core::UUID4::new(),
        UnixNanos::default(),
        None,
        None, // correlation_id
    );

    client.query_order(query_cmd).unwrap();

    wait_until_async(
        || {
            let order_query_count = order_query_count.clone();
            async move { order_query_count.load(Ordering::SeqCst) > 0 }
        },
        Duration::from_secs(5),
    )
    .await;

    tokio::time::sleep(Duration::from_millis(50)).await;

    let mut emitted_order_report = false;

    while let Ok(event) = rx.try_recv() {
        if matches!(event, ExecutionEvent::Report(ExecutionReport::Order(_))) {
            emitted_order_report = true;
        }
    }

    assert!(!emitted_order_report);
}

async fn connected_client_with_command_responses(
    responses: CommandResponses,
) -> (
    BinanceSpotExecutionClient,
    tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
    Rc<RefCell<Cache>>,
    Arc<AtomicUsize>,
) {
    let (addr, request_count) = start_exec_test_server_with_command_responses(responses).await;
    let base_url = format!("http://{addr}");

    let (mut client, rx, cache) = create_test_execution_client(base_url);
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

fn add_spot_oco_orders_to_cache(cache: &Rc<RefCell<Cache>>) -> Vec<OrderAny> {
    let order_list_id = OrderListId::from("OL-SPOT-OCO");
    let take_profit_id = ClientOrderId::new("spot-oco-tp");
    let stop_loss_id = ClientOrderId::new("spot-oco-sl");

    let take_profit = OrderAny::Limit(LimitOrder::new(
        test_trader_id(),
        test_strategy_id(),
        test_instrument_id(),
        take_profit_id,
        OrderSide::Sell,
        Quantity::from("0.001"),
        Price::from("60000.00"),
        TimeInForce::Gtc,
        None,
        true,
        false,
        false,
        None,
        None,
        None,
        Some(ContingencyType::Oco),
        Some(order_list_id),
        Some(vec![stop_loss_id]),
        None,
        None,
        None,
        None,
        None,
        nautilus_core::UUID4::new(),
        UnixNanos::default(),
    ));

    let stop_loss = OrderAny::StopLimit(StopLimitOrder::new(
        test_trader_id(),
        test_strategy_id(),
        test_instrument_id(),
        stop_loss_id,
        OrderSide::Sell,
        Quantity::from("0.001"),
        Price::from("49000.00"),
        Price::from("50000.00"),
        TriggerType::Default,
        TimeInForce::Gtc,
        None,
        false,
        false,
        false,
        None,
        None,
        None,
        Some(ContingencyType::Oco),
        Some(order_list_id),
        Some(vec![take_profit_id]),
        None,
        None,
        None,
        None,
        None,
        nautilus_core::UUID4::new(),
        UnixNanos::default(),
    ));

    let orders = vec![take_profit, stop_loss];
    for order in &orders {
        cache
            .borrow_mut()
            .add_order(order.clone(), None, None, false)
            .unwrap();
    }

    orders
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

fn submit_order_list_command(orders: &[OrderAny]) -> SubmitOrderList {
    let order_list = OrderList::new(
        OrderListId::from("OL-SPOT-TEST"),
        test_instrument_id(),
        test_strategy_id(),
        orders.iter().map(|order| order.client_order_id()).collect(),
        UnixNanos::default(),
    );
    let order_inits = orders
        .iter()
        .map(|order| order.init_event().clone())
        .collect();

    SubmitOrderList::new(
        test_trader_id(),
        Some(*BINANCE_CLIENT_ID),
        test_strategy_id(),
        order_list,
        order_inits,
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

async fn wait_for_ws_method(state: &WsSetupState, expected_method: &str) {
    wait_until_async(
        || {
            let state = state.clone();
            async move {
                state
                    .received_methods()
                    .await
                    .iter()
                    .any(|method| method == expected_method)
            }
        },
        Duration::from_secs(5),
    )
    .await;
}

async fn assert_ws_setup_failure_uses_http(behavior: WsSetupBehavior, expected_methods: &[&str]) {
    let (http_addr, request_count) =
        start_exec_test_server_with_command_responses(CommandResponses::default()).await;
    let (ws_addr, ws_state) = start_ws_setup_test_server(behavior).await;
    let base_url_http = format!("http://{http_addr}");
    let base_url_ws_trading = format!("ws://{ws_addr}/ws-api/v3");

    let (mut client, mut rx, cache) =
        create_test_execution_client_with_ws_trading(base_url_http, base_url_ws_trading);
    add_test_account_to_cache(&cache, AccountId::from("BINANCE-001"));

    client.start().unwrap();
    let connect_timeout = match behavior {
        WsSetupBehavior::CompleteSetup | WsSetupBehavior::RejectFirstSessionLogon => {
            unreachable!("complete setup is not a setup failure")
        }
        WsSetupBehavior::IgnoreSessionLogon => Duration::from_secs(12),
        WsSetupBehavior::RejectSessionLogon | WsSetupBehavior::RejectUserDataSubscribe => {
            Duration::from_secs(5)
        }
    };
    tokio::time::timeout(connect_timeout, client.connect())
        .await
        .expect("Connect should finish before setup timeout")
        .unwrap();
    assert!(client.is_connected());

    let client_order_id = ClientOrderId::new("ws-setup-fallback-test-001");
    let order_any = add_limit_order_to_cache(&cache, client_order_id);

    client
        .submit_order(submit_order_command(&order_any))
        .unwrap();

    wait_for_command_requests(&request_count, 1).await;

    match recv_until(&mut rx, |event| {
        matches!(
            event,
            ExecutionEvent::Order(OrderEventAny::Accepted(event))
                if event.client_order_id == client_order_id
        )
    })
    .await
    {
        ExecutionEvent::Order(OrderEventAny::Accepted(event)) => {
            assert_eq!(event.client_order_id, client_order_id);
        }
        other => panic!("Expected Accepted event, was {other:?}"),
    }

    let expected_methods = expected_methods
        .iter()
        .map(|method| (*method).to_string())
        .collect::<Vec<_>>();
    assert_eq!(ws_state.received_methods().await, expected_methods);

    client.disconnect().await.unwrap();
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

fn test_trader_id() -> TraderId {
    TraderId::from("TESTER-001")
}

fn test_strategy_id() -> StrategyId {
    StrategyId::from("TEST-STRATEGY")
}

fn test_instrument_id() -> InstrumentId {
    InstrumentId::from("BTCUSDT.BINANCE")
}
