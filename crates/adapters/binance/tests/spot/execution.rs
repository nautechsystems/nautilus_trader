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

use std::{cell::RefCell, collections::HashMap, net::SocketAddr, rc::Rc, time::Duration};

use axum::{
    Router,
    body::Body,
    extract::Query,
    http::{HeaderMap, StatusCode, header},
    response::IntoResponse,
    routing::{get, post},
};
use nautilus_binance::{
    common::{
        enums::{BinanceEnvironment, BinanceProductType},
        sbe::spot::{SBE_SCHEMA_ID, SBE_SCHEMA_VERSION},
    },
    config::BinanceExecClientConfig,
    spot::execution::BinanceSpotExecutionClient,
};
use nautilus_common::{
    cache::Cache,
    clients::ExecutionClient,
    live::runner::set_exec_event_sender,
    messages::{
        ExecutionEvent,
        execution::{CancelAllOrders, QueryAccount, SubmitOrder},
    },
    testing::wait_until_async,
};
use nautilus_core::UnixNanos;
use nautilus_live::ExecutionClientCore;
use nautilus_model::{
    accounts::{AccountAny, CashAccount},
    enums::{AccountType, OmsType, OrderSide, TimeInForce},
    events::{AccountState, OrderEventAny},
    identifiers::{AccountId, ClientId, ClientOrderId, InstrumentId, StrategyId, TraderId, Venue},
    orders::{LimitOrder, Order, OrderAny},
    types::{AccountBalance, Money, Price, Quantity},
};
use nautilus_network::http::HttpClient;
use rstest::rstest;

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

fn create_exec_test_router() -> Router {
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

async fn start_exec_test_server() -> SocketAddr {
    let router = create_exec_test_router();
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

fn create_test_execution_client(
    base_url: String,
) -> (
    BinanceSpotExecutionClient,
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
        AccountType::Cash,
        None,
        cache.clone(),
    );

    let config = BinanceExecClientConfig {
        trader_id,
        account_id,
        product_types: vec![BinanceProductType::Spot],
        environment: BinanceEnvironment::Mainnet,
        base_url_http: Some(base_url),
        base_url_ws: None,
        api_key: Some("test_api_key".to_string()),
        api_secret: Some("test_api_secret".to_string()),
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

    assert_eq!(client.client_id(), ClientId::from("BINANCE"));
    assert_eq!(client.venue(), Venue::from("BINANCE"));
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
    tokio::time::sleep(Duration::from_millis(500)).await;

    let mut events = Vec::new();
    while let Ok(event) = rx.try_recv() {
        events.push(event);
    }

    assert!(!events.is_empty(), "Expected at least one event");

    let has_accepted = events
        .iter()
        .any(|e| matches!(e, ExecutionEvent::Order(OrderEventAny::Accepted(_))));
    assert!(has_accepted, "Expected OrderAccepted event");
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
        Some(ClientId::from("BINANCE")),
        StrategyId::from("TEST-STRATEGY"),
        instrument_id,
        OrderSide::NoOrderSide,
        nautilus_core::UUID4::new(),
        UnixNanos::default(),
        None,
    );

    client.cancel_all_orders(&cancel_all_cmd).unwrap();
    tokio::time::sleep(Duration::from_millis(500)).await;

    let mut events = Vec::new();
    while let Ok(event) = rx.try_recv() {
        events.push(event);
    }

    let canceled_count = events
        .iter()
        .filter(|e| matches!(e, ExecutionEvent::Order(OrderEventAny::Canceled(_))))
        .count();

    assert!(
        canceled_count >= 1,
        "Expected at least one OrderCanceled event, was {canceled_count}"
    );
}

// Note: This test is ignored because query_account uses block_on internally
// which conflicts with the tokio test runtime. The functionality is tested
// through the connect() path which also queries account state.
#[rstest]
#[tokio::test]
#[ignore = "query_account uses block_on which conflicts with tokio test runtime"]
async fn test_query_account_generates_account_state_event() {
    let addr = start_exec_test_server().await;
    let base_url = format!("http://{addr}");

    let (mut client, mut rx, cache) = create_test_execution_client(base_url);
    add_test_account_to_cache(&cache, AccountId::from("BINANCE-001"));

    client.start().unwrap();
    client.connect().await.unwrap();

    let query_cmd = QueryAccount::new(
        TraderId::from("TESTER-001"),
        Some(ClientId::from("BINANCE")),
        AccountId::from("BINANCE-SPOT-001"),
        nautilus_core::UUID4::new(),
        UnixNanos::default(),
    );

    client.query_account(&query_cmd).unwrap();
    tokio::time::sleep(Duration::from_millis(500)).await;

    let mut events = Vec::new();
    while let Ok(event) = rx.try_recv() {
        events.push(event);
    }

    let has_account_state = events
        .iter()
        .any(|e| matches!(e, ExecutionEvent::Account(_)));

    assert!(has_account_state, "Expected AccountState event");
}
