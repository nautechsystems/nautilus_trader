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

//! Integration tests for the Binance Spot HTTP client using a mock Axum server with SBE encoding.

use std::{collections::HashMap, net::SocketAddr, sync::Arc, time::Duration};

use axum::{
    Router,
    body::Body,
    extract::Query,
    http::{HeaderMap, StatusCode, header},
    response::IntoResponse,
    routing::{delete, get, post},
};
use nautilus_binance::{
    common::{
        enums::{BinanceEnvironment, BinanceSide, BinanceTimeInForce},
        sbe::spot::{SBE_SCHEMA_ID, SBE_SCHEMA_VERSION},
    },
    spot::{
        enums::BinanceSpotOrderType,
        http::{
            client::{BinanceRawSpotHttpClient, BinanceSpotHttpClient},
            query::{AccountInfoParams, DepthParams},
        },
    },
};
use nautilus_common::testing::wait_until_async;
use nautilus_model::{
    data::BarType,
    enums::{AggregationSource, OrderSide, OrderType, TimeInForce},
    identifiers::{AccountId, ClientOrderId, InstrumentId, VenueOrderId},
    types::{Price, Quantity},
};
use nautilus_network::http::HttpClient;
use rstest::rstest;

const PING_TEMPLATE_ID: u16 = 101;
const SERVER_TIME_TEMPLATE_ID: u16 = 102;
const DEPTH_TEMPLATE_ID: u16 = 200;
const TRADES_TEMPLATE_ID: u16 = 201;
const KLINES_TEMPLATE_ID: u16 = 203;
const EXCHANGE_INFO_TEMPLATE_ID: u16 = 103;
const NEW_ORDER_FULL_TEMPLATE_ID: u16 = 302;
const ORDER_TEMPLATE_ID: u16 = 304;
const CANCEL_ORDER_TEMPLATE_ID: u16 = 305;
const CANCEL_OPEN_ORDERS_TEMPLATE_ID: u16 = 306;
const ACCOUNT_TEMPLATE_ID: u16 = 400;
const ORDERS_TEMPLATE_ID: u16 = 308;
const ACCOUNT_TRADES_TEMPLATE_ID: u16 = 401;
const SYMBOL_BLOCK_LENGTH: u16 = 19;
const ORDERS_GROUP_BLOCK_LENGTH: u16 = 162;
const ORDER_BLOCK_LENGTH: u16 = 153;
const KLINES_BLOCK_LENGTH: u16 = 120;
const ACCOUNT_BLOCK_LENGTH: u16 = 64;
const BALANCE_BLOCK_LENGTH: u16 = 17;
const ACCOUNT_TRADE_BLOCK_LENGTH: u16 = 70;
const NEW_ORDER_FULL_BLOCK_LENGTH: u16 = 153;
const CANCEL_ORDER_BLOCK_LENGTH: u16 = 137;

// Filter template IDs (from Binance SBE schema)
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

/// Builds SBE binary data for a PRICE_FILTER.
fn build_sbe_price_filter(exponent: i8, min_price: i64, max_price: i64, tick_size: i64) -> Vec<u8> {
    let mut buf = Vec::new();

    // SBE message header (8 bytes)
    buf.extend_from_slice(&25u16.to_le_bytes()); // block_length (1 + 8 + 8 + 8 = 25)
    buf.extend_from_slice(&PRICE_FILTER_TEMPLATE_ID.to_le_bytes());
    buf.extend_from_slice(&SBE_SCHEMA_ID.to_le_bytes());
    buf.extend_from_slice(&SBE_SCHEMA_VERSION.to_le_bytes());

    // Filter body
    buf.push(exponent as u8);
    buf.extend_from_slice(&min_price.to_le_bytes());
    buf.extend_from_slice(&max_price.to_le_bytes());
    buf.extend_from_slice(&tick_size.to_le_bytes());

    buf
}

/// Builds SBE binary data for a LOT_SIZE filter.
fn build_sbe_lot_size_filter(exponent: i8, min_qty: i64, max_qty: i64, step_size: i64) -> Vec<u8> {
    let mut buf = Vec::new();

    // SBE message header (8 bytes)
    buf.extend_from_slice(&25u16.to_le_bytes()); // block_length (1 + 8 + 8 + 8 = 25)
    buf.extend_from_slice(&LOT_SIZE_FILTER_TEMPLATE_ID.to_le_bytes());
    buf.extend_from_slice(&SBE_SCHEMA_ID.to_le_bytes());
    buf.extend_from_slice(&SBE_SCHEMA_VERSION.to_le_bytes());

    // Filter body
    buf.push(exponent as u8);
    buf.extend_from_slice(&min_qty.to_le_bytes());
    buf.extend_from_slice(&max_qty.to_le_bytes());
    buf.extend_from_slice(&step_size.to_le_bytes());

    buf
}

fn build_ping_response() -> Vec<u8> {
    create_sbe_header(0, PING_TEMPLATE_ID).to_vec()
}

fn build_server_time_response(time_us: i64) -> Vec<u8> {
    let header = create_sbe_header(8, SERVER_TIME_TEMPLATE_ID);
    let mut buf = Vec::new();
    buf.extend_from_slice(&header);
    buf.extend_from_slice(&time_us.to_le_bytes());
    buf
}

fn build_depth_response(last_update_id: i64, bids: &[(i64, i64)], asks: &[(i64, i64)]) -> Vec<u8> {
    let header = create_sbe_header(10, DEPTH_TEMPLATE_ID);
    let mut buf = Vec::new();
    buf.extend_from_slice(&header);

    // Fixed block: last_update_id, price_exponent, qty_exponent (total 10 bytes)
    buf.extend_from_slice(&last_update_id.to_le_bytes());
    buf.push((-8i8) as u8); // price_exponent
    buf.push((-8i8) as u8); // qty_exponent

    // Bids group
    buf.extend_from_slice(&create_group_header(16, bids.len() as u32));
    for (price, qty) in bids {
        buf.extend_from_slice(&price.to_le_bytes());
        buf.extend_from_slice(&qty.to_le_bytes());
    }

    // Asks group
    buf.extend_from_slice(&create_group_header(16, asks.len() as u32));
    for (price, qty) in asks {
        buf.extend_from_slice(&price.to_le_bytes());
        buf.extend_from_slice(&qty.to_le_bytes());
    }

    buf
}

fn build_trades_response(trades: &[(i64, i64, i64, i64, bool)]) -> Vec<u8> {
    let header = create_sbe_header(2, TRADES_TEMPLATE_ID);
    let mut buf = Vec::new();
    buf.extend_from_slice(&header);

    // Fixed block
    buf.push((-8i8) as u8); // price_exponent
    buf.push((-8i8) as u8); // qty_exponent

    // Trades group (block_length=42 for trade entries)
    buf.extend_from_slice(&create_group_header(42, trades.len() as u32));
    for (id, price, qty, quote_qty, is_buyer_maker) in trades {
        buf.extend_from_slice(&id.to_le_bytes()); // id
        buf.extend_from_slice(&price.to_le_bytes()); // price
        buf.extend_from_slice(&qty.to_le_bytes()); // qty
        buf.extend_from_slice(&quote_qty.to_le_bytes()); // quoteQty
        buf.extend_from_slice(&1734300000000i64.to_le_bytes()); // time
        buf.push(u8::from(*is_buyer_maker)); // isBuyerMaker
        buf.push(1); // isBestMatch
    }

    buf
}

#[allow(clippy::too_many_arguments)]
fn build_klines_response(klines: &[(i64, i64, i64, i64, i64, i64, i64)]) -> Vec<u8> {
    // Each tuple: (open_time, open, high, low, close, volume, close_time)
    let header = create_sbe_header(2, KLINES_TEMPLATE_ID);
    let mut buf = Vec::new();
    buf.extend_from_slice(&header);

    buf.push((-8i8) as u8); // price_exponent
    buf.push((-8i8) as u8); // qty_exponent

    buf.extend_from_slice(&create_group_header(
        KLINES_BLOCK_LENGTH,
        klines.len() as u32,
    ));

    for (open_time, open, high, low, close, volume, close_time) in klines {
        buf.extend_from_slice(&open_time.to_le_bytes()); // open_time
        buf.extend_from_slice(&open.to_le_bytes()); // open_price
        buf.extend_from_slice(&high.to_le_bytes()); // high_price
        buf.extend_from_slice(&low.to_le_bytes()); // low_price
        buf.extend_from_slice(&close.to_le_bytes()); // close_price
        buf.extend_from_slice(&(*volume as i128).to_le_bytes()); // volume (i128)
        buf.extend_from_slice(&close_time.to_le_bytes()); // close_time
        buf.extend_from_slice(&(*volume as i128).to_le_bytes()); // quote_volume (i128)
        buf.extend_from_slice(&100i64.to_le_bytes()); // num_trades
        buf.extend_from_slice(&((*volume / 2) as i128).to_le_bytes()); // taker_buy_base_volume
        buf.extend_from_slice(&((*volume / 2) as i128).to_le_bytes()); // taker_buy_quote_volume
    }

    buf
}

fn build_single_order_response(
    order_id: i64,
    symbol: &str,
    client_order_id: &str,
    price: i64,
    qty: i64,
) -> Vec<u8> {
    let header = create_sbe_header(ORDER_BLOCK_LENGTH, ORDER_TEMPLATE_ID);
    let mut buf = Vec::new();
    buf.extend_from_slice(&header);

    buf.push((-8i8) as u8); // price_exponent
    buf.push((-8i8) as u8); // qty_exponent
    buf.extend_from_slice(&order_id.to_le_bytes()); // order_id
    buf.extend_from_slice(&i64::MIN.to_le_bytes()); // order_list_id (None)
    buf.extend_from_slice(&price.to_le_bytes()); // price_mantissa
    buf.extend_from_slice(&qty.to_le_bytes()); // orig_qty
    buf.extend_from_slice(&0i64.to_le_bytes()); // executed_qty
    buf.extend_from_slice(&0i64.to_le_bytes()); // cummulative_quote_qty
    buf.push(1); // status (NEW)
    buf.push(1); // time_in_force (GTC)
    buf.push(1); // order_type (LIMIT)
    buf.push(1); // side (BUY)
    buf.extend_from_slice(&i64::MIN.to_le_bytes()); // stop_price (None)
    buf.extend_from_slice(&i64::MIN.to_le_bytes()); // iceberg_qty (None)
    buf.extend_from_slice(&1734300000000i64.to_le_bytes()); // time
    buf.extend_from_slice(&1734300000000i64.to_le_bytes()); // update_time
    buf.push(1); // is_working
    buf.extend_from_slice(&1734300000000i64.to_le_bytes()); // working_time
    buf.extend_from_slice(&0i64.to_le_bytes()); // orig_quote_order_qty
    buf.push(0); // self_trade_prevention_mode

    // Pad to ORDER_BLOCK_LENGTH (153 bytes) - we've written 104 bytes of fixed data
    let fixed_written = 104;
    buf.extend(std::iter::repeat_n(
        0u8,
        ORDER_BLOCK_LENGTH as usize - fixed_written,
    ));

    write_var_string(&mut buf, symbol);
    write_var_string(&mut buf, client_order_id);

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
        // Fixed block (19 bytes)
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

        // Filters nested group: 2 SBE-encoded filters (PRICE_FILTER and LOT_SIZE)
        buf.extend_from_slice(&create_group_header(0, 2));

        // PRICE_FILTER: exponent=-2, min=1 (0.01), max=10_000_000 (100000), tick=1 (0.01)
        let price_filter = build_sbe_price_filter(-2, 1, 10_000_000, 1);
        write_var_bytes(&mut buf, &price_filter);

        // LOT_SIZE: exponent=-5, min=1 (0.00001), max=900_000_000 (9000), step=1 (0.00001)
        let lot_filter = build_sbe_lot_size_filter(-5, 1, 900_000_000, 1);
        write_var_bytes(&mut buf, &lot_filter);

        // Empty permission sets nested group
        buf.extend_from_slice(&create_group_header(0, 0));

        // Variable-length strings
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

    // Fixed block
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

    // Pad to 64 bytes
    while buf.len() < 8 + ACCOUNT_BLOCK_LENGTH as usize {
        buf.push(0);
    }

    // Balances group
    buf.extend_from_slice(&create_group_header(
        BALANCE_BLOCK_LENGTH,
        balances.len() as u32,
    ));

    for (asset, free, locked) in balances {
        buf.push((-8i8) as u8); // exponent
        buf.extend_from_slice(&free.to_le_bytes()); // free
        buf.extend_from_slice(&locked.to_le_bytes()); // locked
        write_var_string(&mut buf, asset);
    }

    buf
}

fn build_orders_response(orders: &[(i64, &str, &str, i64, i64)]) -> Vec<u8> {
    let header = create_sbe_header(0, ORDERS_TEMPLATE_ID);
    let mut buf = Vec::new();
    buf.extend_from_slice(&header);

    // Group header
    buf.extend_from_slice(&create_group_header(
        ORDERS_GROUP_BLOCK_LENGTH,
        orders.len() as u32,
    ));

    for (order_id, symbol, client_order_id, price, qty) in orders {
        let order_start = buf.len();

        buf.push((-8i8) as u8); // price_exponent
        buf.push((-8i8) as u8); // qty_exponent
        buf.extend_from_slice(&order_id.to_le_bytes()); // order_id
        buf.extend_from_slice(&i64::MIN.to_le_bytes()); // order_list_id (None)
        buf.extend_from_slice(&price.to_le_bytes()); // price_mantissa
        buf.extend_from_slice(&qty.to_le_bytes()); // orig_qty
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

        // Pad to block length
        while buf.len() - order_start < ORDERS_GROUP_BLOCK_LENGTH as usize {
            buf.push(0);
        }

        write_var_string(&mut buf, symbol);
        write_var_string(&mut buf, client_order_id);
    }

    buf
}

#[allow(clippy::type_complexity)]
fn build_account_trades_response(
    trades: &[(i64, i64, &str, &str, i64, i64, i64, bool, bool)],
) -> Vec<u8> {
    let header = create_sbe_header(0, ACCOUNT_TRADES_TEMPLATE_ID);
    let mut buf = Vec::new();
    buf.extend_from_slice(&header);

    // Group header
    buf.extend_from_slice(&create_group_header(
        ACCOUNT_TRADE_BLOCK_LENGTH,
        trades.len() as u32,
    ));

    for (id, order_id, symbol, commission_asset, price, qty, commission, is_buyer, is_maker) in
        trades
    {
        buf.push((-8i8) as u8); // price_exponent
        buf.push((-8i8) as u8); // qty_exponent
        buf.push((-8i8) as u8); // commission_exponent
        buf.extend_from_slice(&id.to_le_bytes()); // id
        buf.extend_from_slice(&order_id.to_le_bytes()); // order_id
        buf.extend_from_slice(&i64::MIN.to_le_bytes()); // order_list_id (None)
        buf.extend_from_slice(&price.to_le_bytes()); // price
        buf.extend_from_slice(&qty.to_le_bytes()); // qty
        buf.extend_from_slice(&(price * qty / 100_000_000).to_le_bytes()); // quote_qty
        buf.extend_from_slice(&commission.to_le_bytes()); // commission
        buf.extend_from_slice(&1734300000000i64.to_le_bytes()); // time
        buf.push(u8::from(*is_buyer)); // is_buyer
        buf.push(u8::from(*is_maker)); // is_maker
        buf.push(1); // is_best_match
        write_var_string(&mut buf, symbol);
        write_var_string(&mut buf, commission_asset);
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

    // Fixed block (153 bytes)
    buf.push((-8i8) as u8); // price_exponent
    buf.push((-8i8) as u8); // qty_exponent
    buf.extend_from_slice(&order_id.to_le_bytes()); // order_id
    buf.extend_from_slice(&i64::MIN.to_le_bytes()); // order_list_id (None)
    buf.extend_from_slice(&1734300000000i64.to_le_bytes()); // transact_time
    buf.extend_from_slice(&price.to_le_bytes()); // price_mantissa
    buf.extend_from_slice(&qty.to_le_bytes()); // orig_qty_mantissa
    buf.extend_from_slice(&executed_qty.to_le_bytes()); // executed_qty_mantissa
    buf.extend_from_slice(&(price * executed_qty).to_le_bytes()); // cummulative_quote_qty
    buf.push(status); // status (1=NEW, 2=FILLED)
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
    buf.extend_from_slice(&[0u8; 18]); // padding to end of fixed block

    // Fills group (empty) - block length is 42
    buf.extend_from_slice(&create_group_header(42, 0));

    // Prevented matches group (empty) - block length is 40
    buf.extend_from_slice(&create_group_header(40, 0));

    // Variable strings
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

    // Fixed block (137 bytes)
    buf.push((-8i8) as u8); // price_exponent
    buf.push((-8i8) as u8); // qty_exponent
    buf.extend_from_slice(&order_id.to_le_bytes()); // order_id
    buf.extend_from_slice(&i64::MIN.to_le_bytes()); // order_list_id (None)
    buf.extend_from_slice(&1734300000000i64.to_le_bytes()); // transact_time
    buf.extend_from_slice(&price.to_le_bytes()); // price_mantissa
    buf.extend_from_slice(&qty.to_le_bytes()); // orig_qty_mantissa
    buf.extend_from_slice(&executed_qty.to_le_bytes()); // executed_qty_mantissa
    buf.extend_from_slice(&(price * executed_qty).to_le_bytes()); // cummulative_quote_qty
    buf.push(4); // status (CANCELED)
    buf.push(1); // time_in_force (GTC)
    buf.push(1); // order_type (LIMIT)
    buf.push(1); // side (BUY)
    buf.push(0); // self_trade_prevention_mode

    // Pad to end of fixed block (137 - 63 = 74 bytes remaining)
    let current_len = buf.len() - 8; // Subtract header
    buf.extend_from_slice(&vec![0u8; CANCEL_ORDER_BLOCK_LENGTH as usize - current_len]);

    // Variable strings
    write_var_string(&mut buf, symbol);
    write_var_string(&mut buf, orig_client_order_id);
    write_var_string(&mut buf, client_order_id);

    buf
}

fn build_cancel_open_orders_response(orders: &[(i64, &str, &str, &str, i64, i64)]) -> Vec<u8> {
    let header = create_sbe_header(0, CANCEL_OPEN_ORDERS_TEMPLATE_ID);
    let mut buf = Vec::new();
    buf.extend_from_slice(&header);

    // Group header with block_length=0 (embedded messages)
    buf.extend_from_slice(&create_group_header(0, orders.len() as u32));

    // Each item is: u16 length prefix + embedded cancel_order SBE message
    for (order_id, symbol, client_order_id, orig_client_order_id, price, qty) in orders {
        let embedded = build_cancel_order_response(
            *order_id,
            symbol,
            client_order_id,
            orig_client_order_id,
            *price,
            *qty,
            0, // executed_qty
        );
        buf.extend_from_slice(&(embedded.len() as u16).to_le_bytes());
        buf.extend_from_slice(&embedded);
    }

    buf
}

#[derive(Clone, Default)]
struct TestServerState {
    request_count: Arc<std::sync::Mutex<usize>>,
    rate_limit_after: usize,
}

impl TestServerState {
    fn with_rate_limit(limit: usize) -> Self {
        Self {
            request_count: Arc::new(std::sync::Mutex::new(0)),
            rate_limit_after: limit,
        }
    }

    fn increment_and_check(&self) -> bool {
        let mut count = self.request_count.lock().unwrap();
        *count += 1;
        self.rate_limit_after > 0 && *count > self.rate_limit_after
    }
}

fn has_auth_headers(headers: &HeaderMap) -> bool {
    headers.contains_key("x-mbx-apikey")
}

async fn wait_for_server(addr: SocketAddr, path: &str) {
    let health_url = format!("http://{addr}{path}");
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

fn sbe_response(body: Vec<u8>) -> impl IntoResponse {
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "application/sbe")],
        Body::from(body),
    )
}

fn rate_limit_response() -> impl IntoResponse {
    (
        StatusCode::TOO_MANY_REQUESTS,
        [(header::CONTENT_TYPE, "application/json")],
        Body::from(r#"{"code":-1015,"msg":"Too many requests"}"#),
    )
}

fn unauthorized_response() -> impl IntoResponse {
    (
        StatusCode::UNAUTHORIZED,
        [(header::CONTENT_TYPE, "application/json")],
        Body::from(r#"{"code":-2015,"msg":"Invalid API-key, IP, or permissions for action"}"#),
    )
}

fn create_router(state: Arc<TestServerState>) -> Router {
    let ping_state = state.clone();
    let time_state = state.clone();
    let depth_state = state.clone();
    let trades_state = state.clone();
    let klines_state = state.clone();
    let exchange_info_state = state.clone();
    let account_state = state.clone();
    let open_orders_state = state.clone();
    let all_orders_state = state.clone();
    let order_query_state = state.clone();
    let my_trades_state = state.clone();
    let new_order_state = state.clone();
    let cancel_order_state = state.clone();
    let cancel_all_orders_state = state;

    Router::new()
        .route(
            "/api/v3/ping",
            get(move || {
                let state = ping_state.clone();
                async move {
                    if state.increment_and_check() {
                        return rate_limit_response().into_response();
                    }
                    sbe_response(build_ping_response()).into_response()
                }
            }),
        )
        .route(
            "/api/v3/time",
            get(move || {
                let state = time_state.clone();
                async move {
                    if state.increment_and_check() {
                        return rate_limit_response().into_response();
                    }
                    // Current time in microseconds
                    let time_us = chrono::Utc::now().timestamp_micros();
                    sbe_response(build_server_time_response(time_us)).into_response()
                }
            }),
        )
        .route(
            "/api/v3/depth",
            get(move |Query(params): Query<HashMap<String, String>>| {
                let state = depth_state.clone();
                async move {
                    if state.increment_and_check() {
                        return rate_limit_response().into_response();
                    }

                    let _symbol = params.get("symbol").cloned().unwrap_or_default();
                    let bids = vec![
                        (100_000_000_000i64, 10_000_000i64), // 1000.00 @ 0.1
                        (99_900_000_000i64, 20_000_000i64),  // 999.00 @ 0.2
                    ];
                    let asks = vec![
                        (100_100_000_000i64, 15_000_000i64), // 1001.00 @ 0.15
                        (100_200_000_000i64, 25_000_000i64), // 1002.00 @ 0.25
                    ];
                    sbe_response(build_depth_response(12345, &bids, &asks)).into_response()
                }
            }),
        )
        .route(
            "/api/v3/trades",
            get(move |Query(params): Query<HashMap<String, String>>| {
                let state = trades_state.clone();
                async move {
                    if state.increment_and_check() {
                        return rate_limit_response().into_response();
                    }

                    let _symbol = params.get("symbol").cloned().unwrap_or_default();
                    let trades = vec![
                        (
                            1001i64,
                            100_000_000_000i64,
                            10_000_000i64,
                            1_000_000_000_000i64,
                            true,
                        ),
                        (
                            1002i64,
                            100_100_000_000i64,
                            5_000_000i64,
                            500_500_000_000i64,
                            false,
                        ),
                    ];
                    sbe_response(build_trades_response(&trades)).into_response()
                }
            }),
        )
        .route(
            "/api/v3/klines",
            get(move |Query(params): Query<HashMap<String, String>>| {
                let state = klines_state.clone();
                async move {
                    if state.increment_and_check() {
                        return rate_limit_response().into_response();
                    }
                    let _symbol = params.get("symbol").cloned().unwrap_or_default();
                    let _interval = params.get("interval").cloned().unwrap_or_default();

                    // Two 1-minute bars: open_time, open, high, low, close, volume, close_time
                    let klines = vec![
                        (
                            1734300000000i64,   // open_time
                            100_000_000_000i64, // open
                            101_000_000_000i64, // high
                            99_000_000_000i64,  // low
                            100_500_000_000i64, // close
                            1_000_000_000i64,   // volume
                            1734300059999i64,   // close_time
                        ),
                        (
                            1734300060000i64,
                            100_500_000_000i64,
                            102_000_000_000i64,
                            100_000_000_000i64,
                            101_500_000_000i64,
                            1_500_000_000i64,
                            1734300119999i64,
                        ),
                    ];
                    sbe_response(build_klines_response(&klines)).into_response()
                }
            }),
        )
        .route(
            "/api/v3/exchangeInfo",
            get(move || {
                let state = exchange_info_state.clone();
                async move {
                    if state.increment_and_check() {
                        return rate_limit_response().into_response();
                    }
                    let symbols = vec![
                        ("BTCUSDT", "BTC", "USDT"),
                        ("ETHUSDT", "ETH", "USDT"),
                        ("SOLUSDT", "SOL", "USDT"),
                    ];
                    sbe_response(build_exchange_info_response(&symbols)).into_response()
                }
            }),
        )
        .route(
            "/api/v3/account",
            get(move |headers: HeaderMap| {
                let state = account_state.clone();
                async move {
                    if !has_auth_headers(&headers) {
                        return unauthorized_response().into_response();
                    }
                    if state.increment_and_check() {
                        return rate_limit_response().into_response();
                    }
                    let balances = vec![
                        ("BTC", 100_000_000i64, 50_000_000i64), // 1.0 free, 0.5 locked
                        ("USDT", 1_000_000_000_000i64, 0i64),   // 10000 free, 0 locked
                    ];
                    sbe_response(build_account_response(&balances)).into_response()
                }
            }),
        )
        .route(
            "/api/v3/openOrders",
            get(
                move |headers: HeaderMap, Query(params): Query<HashMap<String, String>>| {
                    let state = open_orders_state.clone();
                    async move {
                        if !has_auth_headers(&headers) {
                            return unauthorized_response().into_response();
                        }
                        if state.increment_and_check() {
                            return rate_limit_response().into_response();
                        }
                        let _symbol = params.get("symbol").cloned().unwrap_or_default();
                        let orders = vec![
                            (
                                12345i64,
                                "BTCUSDT",
                                "order-1",
                                100_000_000_000i64,
                                10_000_000i64,
                            ),
                            (
                                12346i64,
                                "BTCUSDT",
                                "order-2",
                                99_000_000_000i64,
                                20_000_000i64,
                            ),
                        ];
                        sbe_response(build_orders_response(&orders)).into_response()
                    }
                },
            ),
        )
        .route(
            "/api/v3/myTrades",
            get(
                move |headers: HeaderMap, Query(params): Query<HashMap<String, String>>| {
                    let state = my_trades_state.clone();
                    async move {
                        if !has_auth_headers(&headers) {
                            return unauthorized_response().into_response();
                        }
                        if state.increment_and_check() {
                            return rate_limit_response().into_response();
                        }
                        let _symbol = params.get("symbol").cloned().unwrap_or_default();
                        let trades = vec![
                            (
                                1001i64,
                                12345i64,
                                "BTCUSDT",
                                "BNB",
                                100_000_000_000i64,
                                10_000_000i64,
                                100_000i64,
                                true,
                                false,
                            ),
                            (
                                1002i64,
                                12345i64,
                                "BTCUSDT",
                                "BNB",
                                100_000_000_000i64,
                                5_000_000i64,
                                50_000i64,
                                true,
                                true,
                            ),
                        ];
                        sbe_response(build_account_trades_response(&trades)).into_response()
                    }
                },
            ),
        )
        .route(
            "/api/v3/allOrders",
            get(
                move |headers: HeaderMap, Query(params): Query<HashMap<String, String>>| {
                    let state = all_orders_state.clone();
                    async move {
                        if !has_auth_headers(&headers) {
                            return unauthorized_response().into_response();
                        }
                        if state.increment_and_check() {
                            return rate_limit_response().into_response();
                        }
                        let _symbol = params.get("symbol").cloned().unwrap_or_default();

                        // Return a mix of open and filled orders for history
                        let orders = vec![
                            (
                                12345i64,
                                "BTCUSDT",
                                "order-1",
                                100_000_000_000i64,
                                10_000_000i64,
                            ),
                            (
                                12346i64,
                                "BTCUSDT",
                                "order-2",
                                99_000_000_000i64,
                                20_000_000i64,
                            ),
                            (
                                12347i64,
                                "BTCUSDT",
                                "order-3",
                                101_000_000_000i64,
                                15_000_000i64,
                            ),
                        ];
                        sbe_response(build_orders_response(&orders)).into_response()
                    }
                },
            ),
        )
        .route(
            "/api/v3/order",
            post(
                move |headers: HeaderMap, Query(params): Query<HashMap<String, String>>| {
                    let state = new_order_state.clone();
                    async move {
                        if !has_auth_headers(&headers) {
                            return unauthorized_response().into_response();
                        }
                        if state.increment_and_check() {
                            return rate_limit_response().into_response();
                        }
                        let symbol = params
                            .get("symbol")
                            .cloned()
                            .unwrap_or_else(|| "BTCUSDT".to_string());
                        let client_order_id = params
                            .get("newClientOrderId")
                            .cloned()
                            .unwrap_or_else(|| "test-order-1".to_string());
                        sbe_response(build_new_order_response(
                            99999,
                            &symbol,
                            &client_order_id,
                            100_000_000_000, // price: 1000.00
                            10_000_000,      // qty: 0.1
                            0,               // executed: 0
                            1,               // status: NEW
                        ))
                        .into_response()
                    }
                },
            )
            .get(
                move |headers: HeaderMap, Query(params): Query<HashMap<String, String>>| {
                    let state = order_query_state.clone();
                    async move {
                        if !has_auth_headers(&headers) {
                            return unauthorized_response().into_response();
                        }
                        if state.increment_and_check() {
                            return rate_limit_response().into_response();
                        }
                        let symbol = params
                            .get("symbol")
                            .cloned()
                            .unwrap_or_else(|| "BTCUSDT".to_string());
                        let order_id = params
                            .get("orderId")
                            .and_then(|s| s.parse().ok())
                            .unwrap_or(12345);
                        let client_order_id = params
                            .get("origClientOrderId")
                            .cloned()
                            .unwrap_or_else(|| "order-1".to_string());
                        sbe_response(build_single_order_response(
                            order_id,
                            &symbol,
                            &client_order_id,
                            100_000_000_000, // price
                            10_000_000,      // qty
                        ))
                        .into_response()
                    }
                },
            )
            .delete(
                move |headers: HeaderMap, Query(params): Query<HashMap<String, String>>| {
                    let state = cancel_order_state.clone();
                    async move {
                        if !has_auth_headers(&headers) {
                            return unauthorized_response().into_response();
                        }
                        if state.increment_and_check() {
                            return rate_limit_response().into_response();
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
                            .unwrap_or_else(|| "orig-order-1".to_string());
                        sbe_response(build_cancel_order_response(
                            order_id,
                            &symbol,
                            "cancel-req-1",
                            &orig_client_order_id,
                            100_000_000_000, // price
                            10_000_000,      // qty
                            0,               // executed
                        ))
                        .into_response()
                    }
                },
            ),
        )
        .route(
            "/api/v3/openOrders",
            delete(
                move |headers: HeaderMap, Query(params): Query<HashMap<String, String>>| {
                    let state = cancel_all_orders_state.clone();
                    async move {
                        if !has_auth_headers(&headers) {
                            return unauthorized_response().into_response();
                        }
                        if state.increment_and_check() {
                            return rate_limit_response().into_response();
                        }
                        let symbol = params
                            .get("symbol")
                            .cloned()
                            .unwrap_or_else(|| "BTCUSDT".to_string());
                        let orders = vec![
                            (
                                12345i64,
                                symbol.as_str(),
                                "cancel-1",
                                "order-1",
                                100_000_000_000i64,
                                10_000_000i64,
                            ),
                            (
                                12346i64,
                                symbol.as_str(),
                                "cancel-2",
                                "order-2",
                                99_000_000_000i64,
                                20_000_000i64,
                            ),
                        ];
                        sbe_response(build_cancel_open_orders_response(&orders)).into_response()
                    }
                },
            ),
        )
}

async fn start_test_server(state: Arc<TestServerState>) -> SocketAddr {
    let router = create_router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        axum::serve(listener, router.into_make_service())
            .await
            .unwrap();
    });

    wait_for_server(addr, "/api/v3/ping").await;
    addr
}

#[rstest]
#[tokio::test]
async fn test_ping_returns_success() {
    let addr = start_test_server(Arc::new(TestServerState::default())).await;
    let base_url = format!("http://{addr}");

    let client = BinanceRawSpotHttpClient::new(
        BinanceEnvironment::Mainnet,
        None,
        None,
        Some(base_url),
        None,
        Some(60),
        None,
    )
    .unwrap();

    let result = client.ping().await;
    assert!(result.is_ok());
}

#[rstest]
#[tokio::test]
async fn test_server_time_returns_valid_timestamp() {
    let addr = start_test_server(Arc::new(TestServerState::default())).await;
    let base_url = format!("http://{addr}");

    let client = BinanceRawSpotHttpClient::new(
        BinanceEnvironment::Mainnet,
        None,
        None,
        Some(base_url),
        None,
        Some(60),
        None,
    )
    .unwrap();

    let time = client.server_time().await.unwrap();
    assert!(time > 0);
}

#[rstest]
#[tokio::test]
async fn test_depth_returns_order_book() {
    let addr = start_test_server(Arc::new(TestServerState::default())).await;
    let base_url = format!("http://{addr}");

    let client = BinanceRawSpotHttpClient::new(
        BinanceEnvironment::Mainnet,
        None,
        None,
        Some(base_url),
        None,
        Some(60),
        None,
    )
    .unwrap();

    let params = DepthParams {
        symbol: "BTCUSDT".to_string(),
        limit: Some(5),
    };
    let depth = client.depth(&params).await.unwrap();

    assert_eq!(depth.last_update_id, 12345);
    assert_eq!(depth.bids.len(), 2);
    assert_eq!(depth.asks.len(), 2);
    assert_eq!(depth.bids[0].price_mantissa, 100_000_000_000);
    assert_eq!(depth.asks[0].price_mantissa, 100_100_000_000);
}

#[rstest]
#[tokio::test]
async fn test_trades_returns_recent_trades() {
    let addr = start_test_server(Arc::new(TestServerState::default())).await;
    let base_url = format!("http://{addr}");

    let client = BinanceRawSpotHttpClient::new(
        BinanceEnvironment::Mainnet,
        None,
        None,
        Some(base_url),
        None,
        Some(60),
        None,
    )
    .unwrap();

    let trades = client.trades("BTCUSDT", Some(10)).await.unwrap();

    assert_eq!(trades.trades.len(), 2);
    assert_eq!(trades.trades[0].id, 1001);
    assert!(trades.trades[0].is_buyer_maker);
    assert_eq!(trades.trades[1].id, 1002);
    assert!(!trades.trades[1].is_buyer_maker);
}

#[rstest]
#[tokio::test]
async fn test_exchange_info_returns_symbols() {
    let addr = start_test_server(Arc::new(TestServerState::default())).await;
    let base_url = format!("http://{addr}");

    let client = BinanceRawSpotHttpClient::new(
        BinanceEnvironment::Mainnet,
        None,
        None,
        Some(base_url),
        None,
        Some(60),
        None,
    )
    .unwrap();

    let info = client.exchange_info().await.unwrap();

    assert_eq!(info.symbols.len(), 3);
    assert_eq!(info.symbols[0].symbol, "BTCUSDT");
    assert_eq!(info.symbols[0].base_asset, "BTC");
    assert_eq!(info.symbols[0].quote_asset, "USDT");
    assert_eq!(info.symbols[1].symbol, "ETHUSDT");
    assert_eq!(info.symbols[2].symbol, "SOLUSDT");
}

#[rstest]
#[tokio::test]
async fn test_account_requires_credentials() {
    let addr = start_test_server(Arc::new(TestServerState::default())).await;
    let base_url = format!("http://{addr}");

    let client = BinanceRawSpotHttpClient::new(
        BinanceEnvironment::Mainnet,
        None,
        None,
        Some(base_url),
        None,
        Some(60),
        None,
    )
    .unwrap();

    let params = AccountInfoParams {
        omit_zero_balances: None,
    };
    let result = client.account(&params).await;

    assert!(result.is_err());
}

#[rstest]
#[tokio::test]
async fn test_account_with_credentials_succeeds() {
    let addr = start_test_server(Arc::new(TestServerState::default())).await;
    let base_url = format!("http://{addr}");

    let client = BinanceRawSpotHttpClient::new(
        BinanceEnvironment::Mainnet,
        Some("test_api_key".to_string()),
        Some("test_api_secret".to_string()),
        Some(base_url),
        None,
        Some(60),
        None,
    )
    .unwrap();

    let params = AccountInfoParams {
        omit_zero_balances: None,
    };
    let account = client.account(&params).await.unwrap();

    assert!(account.can_trade);
    assert!(account.can_withdraw);
    assert!(account.can_deposit);
    assert_eq!(account.balances.len(), 2);
    assert_eq!(account.balances[0].asset, "BTC");
    assert_eq!(account.balances[1].asset, "USDT");
}

#[rstest]
#[tokio::test]
async fn test_open_orders_requires_credentials() {
    let addr = start_test_server(Arc::new(TestServerState::default())).await;
    let base_url = format!("http://{addr}");

    let client = BinanceRawSpotHttpClient::new(
        BinanceEnvironment::Mainnet,
        None,
        None,
        Some(base_url),
        None,
        Some(60),
        None,
    )
    .unwrap();

    let result = client.open_orders(Some("BTCUSDT")).await;

    assert!(result.is_err());
}

#[rstest]
#[tokio::test]
async fn test_open_orders_with_credentials_succeeds() {
    let addr = start_test_server(Arc::new(TestServerState::default())).await;
    let base_url = format!("http://{addr}");

    let client = BinanceRawSpotHttpClient::new(
        BinanceEnvironment::Mainnet,
        Some("test_api_key".to_string()),
        Some("test_api_secret".to_string()),
        Some(base_url),
        None,
        Some(60),
        None,
    )
    .unwrap();

    let orders = client.open_orders(Some("BTCUSDT")).await.unwrap();

    assert_eq!(orders.len(), 2);
    assert_eq!(orders[0].order_id, 12345);
    assert_eq!(orders[0].symbol, "BTCUSDT");
    assert_eq!(orders[0].client_order_id, "order-1");
    assert_eq!(orders[1].order_id, 12346);
}

#[rstest]
#[tokio::test]
async fn test_my_trades_requires_credentials() {
    let addr = start_test_server(Arc::new(TestServerState::default())).await;
    let base_url = format!("http://{addr}");

    let client = BinanceRawSpotHttpClient::new(
        BinanceEnvironment::Mainnet,
        None,
        None,
        Some(base_url),
        None,
        Some(60),
        None,
    )
    .unwrap();

    let result = client
        .account_trades("BTCUSDT", None, None, None, None)
        .await;

    assert!(result.is_err());
}

#[rstest]
#[tokio::test]
async fn test_my_trades_with_credentials_succeeds() {
    let addr = start_test_server(Arc::new(TestServerState::default())).await;
    let base_url = format!("http://{addr}");

    let client = BinanceRawSpotHttpClient::new(
        BinanceEnvironment::Mainnet,
        Some("test_api_key".to_string()),
        Some("test_api_secret".to_string()),
        Some(base_url),
        None,
        Some(60),
        None,
    )
    .unwrap();

    let trades = client
        .account_trades("BTCUSDT", None, None, None, None)
        .await
        .unwrap();

    assert_eq!(trades.len(), 2);
    assert_eq!(trades[0].id, 1001);
    assert_eq!(trades[0].order_id, 12345);
    assert!(trades[0].is_buyer);
    assert!(!trades[0].is_maker);
    assert_eq!(trades[1].id, 1002);
    assert!(trades[1].is_maker);
}

#[rstest]
#[tokio::test]
async fn test_rate_limit_triggers_after_threshold() {
    // wait_for_server calls ping once, so limit=3 allows 2 test pings before rate limit
    let state = Arc::new(TestServerState::with_rate_limit(3));
    let addr = start_test_server(state).await;
    let base_url = format!("http://{addr}");

    let client = BinanceRawSpotHttpClient::new(
        BinanceEnvironment::Mainnet,
        None,
        None,
        Some(base_url),
        None,
        Some(60),
        None,
    )
    .unwrap();

    assert!(client.ping().await.is_ok());
    assert!(client.ping().await.is_ok());

    let result = client.ping().await;
    assert!(result.is_err());
}

#[rstest]
#[tokio::test]
async fn test_domain_client_request_instruments() {
    let addr = start_test_server(Arc::new(TestServerState::default())).await;
    let base_url = format!("http://{addr}");

    let client = BinanceSpotHttpClient::new(
        BinanceEnvironment::Mainnet,
        None,
        None,
        Some(base_url),
        None,
        Some(60),
        None,
    )
    .unwrap();

    let instruments = client.request_instruments().await.unwrap();

    assert_eq!(instruments.len(), 3);
}

#[rstest]
#[tokio::test]
async fn test_new_order_requires_credentials() {
    let addr = start_test_server(Arc::new(TestServerState::default())).await;
    let base_url = format!("http://{addr}");

    let client = BinanceRawSpotHttpClient::new(
        BinanceEnvironment::Mainnet,
        None,
        None,
        Some(base_url),
        None,
        Some(60),
        None,
    )
    .unwrap();

    let result = client
        .new_order(
            "BTCUSDT",
            BinanceSide::Buy,
            BinanceSpotOrderType::Limit,
            Some(BinanceTimeInForce::Gtc),
            Some("0.1"),
            Some("50000.00"),
            None,
            None,
        )
        .await;

    assert!(result.is_err());
}

#[rstest]
#[tokio::test]
async fn test_new_order_with_credentials_succeeds() {
    let addr = start_test_server(Arc::new(TestServerState::default())).await;
    let base_url = format!("http://{addr}");

    let client = BinanceRawSpotHttpClient::new(
        BinanceEnvironment::Mainnet,
        Some("test_api_key".to_string()),
        Some("test_api_secret".to_string()),
        Some(base_url),
        None,
        Some(60),
        None,
    )
    .unwrap();

    let order = client
        .new_order(
            "BTCUSDT",
            BinanceSide::Buy,
            BinanceSpotOrderType::Limit,
            Some(BinanceTimeInForce::Gtc),
            Some("0.1"),
            Some("50000.00"),
            Some("my-order-123"),
            None,
        )
        .await
        .unwrap();

    assert_eq!(order.order_id, 99999);
    assert_eq!(order.symbol, "BTCUSDT");
    assert_eq!(order.client_order_id, "my-order-123");
}

#[rstest]
#[tokio::test]
async fn test_cancel_order_requires_credentials() {
    let addr = start_test_server(Arc::new(TestServerState::default())).await;
    let base_url = format!("http://{addr}");

    let client = BinanceRawSpotHttpClient::new(
        BinanceEnvironment::Mainnet,
        None,
        None,
        Some(base_url),
        None,
        Some(60),
        None,
    )
    .unwrap();

    let result = client.cancel_order("BTCUSDT", Some(12345), None).await;

    assert!(result.is_err());
}

#[rstest]
#[tokio::test]
async fn test_cancel_order_with_credentials_succeeds() {
    let addr = start_test_server(Arc::new(TestServerState::default())).await;
    let base_url = format!("http://{addr}");

    let client = BinanceRawSpotHttpClient::new(
        BinanceEnvironment::Mainnet,
        Some("test_api_key".to_string()),
        Some("test_api_secret".to_string()),
        Some(base_url),
        None,
        Some(60),
        None,
    )
    .unwrap();

    let order = client
        .cancel_order("BTCUSDT", Some(12345), None)
        .await
        .unwrap();

    assert_eq!(order.order_id, 12345);
    assert_eq!(order.symbol, "BTCUSDT");
}

#[rstest]
#[tokio::test]
async fn test_cancel_all_orders_requires_credentials() {
    let addr = start_test_server(Arc::new(TestServerState::default())).await;
    let base_url = format!("http://{addr}");

    let client = BinanceRawSpotHttpClient::new(
        BinanceEnvironment::Mainnet,
        None,
        None,
        Some(base_url),
        None,
        Some(60),
        None,
    )
    .unwrap();

    let result = client.cancel_open_orders("BTCUSDT").await;

    assert!(result.is_err());
}

#[rstest]
#[tokio::test]
async fn test_cancel_all_orders_with_credentials_succeeds() {
    let addr = start_test_server(Arc::new(TestServerState::default())).await;
    let base_url = format!("http://{addr}");

    let client = BinanceRawSpotHttpClient::new(
        BinanceEnvironment::Mainnet,
        Some("test_api_key".to_string()),
        Some("test_api_secret".to_string()),
        Some(base_url),
        None,
        Some(60),
        None,
    )
    .unwrap();

    let orders = client.cancel_open_orders("BTCUSDT").await.unwrap();

    assert_eq!(orders.len(), 2);
    assert_eq!(orders[0].order_id, 12345);
    assert_eq!(orders[1].order_id, 12346);
}

async fn create_domain_client_with_instruments(
    base_url: String,
    api_key: Option<String>,
    api_secret: Option<String>,
) -> BinanceSpotHttpClient {
    let client = BinanceSpotHttpClient::new(
        BinanceEnvironment::Mainnet,
        api_key,
        api_secret,
        Some(base_url),
        None,
        Some(60),
        None,
    )
    .unwrap();

    // Cache instruments for domain methods
    client.request_instruments().await.unwrap();
    client
}

#[rstest]
#[tokio::test]
async fn test_domain_request_trades() {
    let addr = start_test_server(Arc::new(TestServerState::default())).await;
    let base_url = format!("http://{addr}");

    let client = create_domain_client_with_instruments(base_url, None, None).await;
    let instrument_id = InstrumentId::from("BTCUSDT.BINANCE");

    let trades = client
        .request_trades(instrument_id, Some(10))
        .await
        .unwrap();

    assert_eq!(trades.len(), 2);
    assert_eq!(trades[0].instrument_id, instrument_id);
}

#[rstest]
#[tokio::test]
async fn test_domain_request_order_status_reports_open() {
    let addr = start_test_server(Arc::new(TestServerState::default())).await;
    let base_url = format!("http://{addr}");

    let client = create_domain_client_with_instruments(
        base_url,
        Some("test_api_key".to_string()),
        Some("test_api_secret".to_string()),
    )
    .await;
    let account_id = AccountId::from("BINANCE-001");
    let instrument_id = InstrumentId::from("BTCUSDT.BINANCE");

    let reports = client
        .request_order_status_reports(account_id, Some(instrument_id), None, None, true, None)
        .await
        .unwrap();

    assert_eq!(reports.len(), 2);
    assert_eq!(reports[0].venue_order_id, VenueOrderId::from("12345"));
    assert_eq!(reports[1].venue_order_id, VenueOrderId::from("12346"));
}

#[rstest]
#[tokio::test]
async fn test_domain_request_fill_reports() {
    let addr = start_test_server(Arc::new(TestServerState::default())).await;
    let base_url = format!("http://{addr}");

    let client = create_domain_client_with_instruments(
        base_url,
        Some("test_api_key".to_string()),
        Some("test_api_secret".to_string()),
    )
    .await;
    let account_id = AccountId::from("BINANCE-001");
    let instrument_id = InstrumentId::from("BTCUSDT.BINANCE");

    let reports = client
        .request_fill_reports(account_id, instrument_id, None, None, None, None)
        .await
        .unwrap();

    assert_eq!(reports.len(), 2);
    assert_eq!(reports[0].venue_order_id, VenueOrderId::from("12345"));
}

#[rstest]
#[tokio::test]
async fn test_domain_submit_order() {
    let addr = start_test_server(Arc::new(TestServerState::default())).await;
    let base_url = format!("http://{addr}");

    let client = create_domain_client_with_instruments(
        base_url,
        Some("test_api_key".to_string()),
        Some("test_api_secret".to_string()),
    )
    .await;
    let account_id = AccountId::from("BINANCE-001");
    let instrument_id = InstrumentId::from("BTCUSDT.BINANCE");
    let client_order_id = ClientOrderId::from("my-order-123");

    let report = client
        .submit_order(
            account_id,
            instrument_id,
            client_order_id,
            OrderSide::Buy,
            OrderType::Limit,
            Quantity::from("0.1"),
            TimeInForce::Gtc,
            Some(Price::from("50000.00")),
            None,
            false,
        )
        .await
        .unwrap();

    assert_eq!(report.venue_order_id, VenueOrderId::from("99999"));
    assert_eq!(report.client_order_id, Some(client_order_id));
}

#[rstest]
#[tokio::test]
async fn test_domain_cancel_order() {
    let addr = start_test_server(Arc::new(TestServerState::default())).await;
    let base_url = format!("http://{addr}");

    let client = create_domain_client_with_instruments(
        base_url,
        Some("test_api_key".to_string()),
        Some("test_api_secret".to_string()),
    )
    .await;
    let instrument_id = InstrumentId::from("BTCUSDT.BINANCE");

    let venue_order_id = client
        .cancel_order(instrument_id, Some(VenueOrderId::from("12345")), None)
        .await
        .unwrap();

    assert_eq!(venue_order_id, VenueOrderId::from("12345"));
}

#[rstest]
#[tokio::test]
async fn test_domain_cancel_all_orders() {
    let addr = start_test_server(Arc::new(TestServerState::default())).await;
    let base_url = format!("http://{addr}");

    let client = create_domain_client_with_instruments(
        base_url,
        Some("test_api_key".to_string()),
        Some("test_api_secret".to_string()),
    )
    .await;
    let instrument_id = InstrumentId::from("BTCUSDT.BINANCE");

    let venue_order_ids = client.cancel_all_orders(instrument_id).await.unwrap();

    assert_eq!(venue_order_ids.len(), 2);
    assert_eq!(venue_order_ids[0], VenueOrderId::from("12345"));
    assert_eq!(venue_order_ids[1], VenueOrderId::from("12346"));
}

#[rstest]
#[tokio::test]
async fn test_domain_request_bars() {
    let addr = start_test_server(Arc::new(TestServerState::default())).await;
    let base_url = format!("http://{addr}");

    let client = create_domain_client_with_instruments(base_url, None, None).await;
    let bar_type = BarType::from("BTCUSDT.BINANCE-1-MINUTE-LAST-EXTERNAL");

    assert_eq!(bar_type.aggregation_source(), AggregationSource::External);

    let bars = client
        .request_bars(bar_type, None, None, Some(2))
        .await
        .unwrap();

    assert_eq!(bars.len(), 2);
    assert_eq!(bars[0].bar_type, bar_type);
    assert_eq!(bars[1].bar_type, bar_type);
}

#[rstest]
#[tokio::test]
async fn test_domain_request_order_status() {
    let addr = start_test_server(Arc::new(TestServerState::default())).await;
    let base_url = format!("http://{addr}");

    let client = create_domain_client_with_instruments(
        base_url,
        Some("test_api_key".to_string()),
        Some("test_api_secret".to_string()),
    )
    .await;
    let account_id = AccountId::from("BINANCE-001");
    let instrument_id = InstrumentId::from("BTCUSDT.BINANCE");
    let venue_order_id = VenueOrderId::from("12345");

    let report = client
        .request_order_status(account_id, instrument_id, Some(venue_order_id), None)
        .await
        .unwrap();

    assert_eq!(report.venue_order_id, venue_order_id);
    assert_eq!(report.instrument_id, instrument_id);
}

#[rstest]
#[tokio::test]
async fn test_domain_request_order_status_reports_history() {
    let addr = start_test_server(Arc::new(TestServerState::default())).await;
    let base_url = format!("http://{addr}");

    let client = create_domain_client_with_instruments(
        base_url,
        Some("test_api_key".to_string()),
        Some("test_api_secret".to_string()),
    )
    .await;
    let account_id = AccountId::from("BINANCE-001");
    let instrument_id = InstrumentId::from("BTCUSDT.BINANCE");

    // Request all orders (not just open ones)
    let reports = client
        .request_order_status_reports(account_id, Some(instrument_id), None, None, false, None)
        .await
        .unwrap();

    assert_eq!(reports.len(), 3);
    assert_eq!(reports[0].venue_order_id, VenueOrderId::from("12345"));
    assert_eq!(reports[1].venue_order_id, VenueOrderId::from("12346"));
    assert_eq!(reports[2].venue_order_id, VenueOrderId::from("12347"));
}
