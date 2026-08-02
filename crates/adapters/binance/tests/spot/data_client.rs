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

//! Integration tests for the Binance Spot data client.

use std::{
    collections::HashMap,
    net::SocketAddr,
    num::NonZeroUsize,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use axum::{
    Router,
    body::Body,
    extract::{
        RawQuery, State,
        ws::{Message, WebSocket},
    },
    http::{StatusCode, header},
    response::{IntoResponse, Response},
    routing::get,
};
use nautilus_binance::{
    common::{
        bar::BinanceBar,
        consts::{BINANCE_CLIENT_ID, BINANCE_VENUE},
    },
    config::{BinanceDataClientConfig, BinanceSpotMarketDataMode},
    data_types::BinanceSpotTicker,
    spot::{
        BinanceSpotDataClient,
        sbe::{
            spot::{SBE_SCHEMA_ID, SBE_SCHEMA_VERSION},
            stream::{STREAM_SCHEMA_ID, template_id},
        },
    },
};
use nautilus_common::{
    clients::DataClient,
    live::runner::set_data_event_sender,
    messages::{
        DataEvent,
        data::{
            DataResponse, RequestBars, RequestBookSnapshot, RequestCustomData, RequestTrades,
            subscribe::{
                SubscribeBars, SubscribeBookDeltas, SubscribeCustomData, SubscribeQuotes,
                SubscribeTrades,
            },
            unsubscribe::{UnsubscribeQuotes, UnsubscribeTrades},
        },
    },
    testing::wait_until_async,
};
use nautilus_core::{Params, UUID4, UnixNanos};
use nautilus_model::{
    data::{BarType, BookOrder, Data, DataType, OrderBookDelta, OrderBookDeltas, QuoteTick},
    enums::{BookAction, BookType, OrderSide, RecordFlag},
    identifiers::InstrumentId,
};
use nautilus_network::{RECONNECTED, http::HttpClient};
use rstest::rstest;
use rust_decimal_macros::dec;
use serde_json::json;

const PING_TEMPLATE_ID: u16 = 101;
const EXCHANGE_INFO_TEMPLATE_ID: u16 = 103;
const DEPTH_TEMPLATE_ID: u16 = 200;
const AGG_TRADES_TEMPLATE_ID: u16 = 202;
const KLINES_TEMPLATE_ID: u16 = 203;
const SYMBOL_BLOCK_LENGTH: u16 = 19;
const PRICE_FILTER_TEMPLATE_ID: u16 = 1;
const LOT_SIZE_FILTER_TEMPLATE_ID: u16 = 4;

#[derive(Clone)]
struct DataTestServerConfig {
    depth_diff_first_update_id: i64,
    depth_diff_last_update_id: i64,
    depth_diff_repetitions: usize,
    depth_diff_delay: Duration,
    depth_snapshot_last_update_ids: Vec<i64>,
    depth_requests: Arc<AtomicUsize>,
    json_ws_streams: bool,
    reconnect_signals_remaining: Arc<AtomicUsize>,
    subscriptions: Arc<Mutex<Vec<Vec<String>>>>,
    unsubscriptions: Arc<Mutex<Vec<Vec<String>>>>,
    agg_trade_queries: Arc<Mutex<Vec<HashMap<String, String>>>>,
    kline_queries: Arc<Mutex<Vec<HashMap<String, String>>>>,
}

impl Default for DataTestServerConfig {
    fn default() -> Self {
        Self {
            depth_diff_first_update_id: 101,
            depth_diff_last_update_id: 101,
            depth_diff_repetitions: 1,
            depth_diff_delay: Duration::from_millis(50),
            depth_snapshot_last_update_ids: vec![100],
            depth_requests: Arc::new(AtomicUsize::new(0)),
            json_ws_streams: false,
            reconnect_signals_remaining: Arc::new(AtomicUsize::new(0)),
            subscriptions: Arc::new(Mutex::new(Vec::new())),
            unsubscriptions: Arc::new(Mutex::new(Vec::new())),
            agg_trade_queries: Arc::new(Mutex::new(Vec::new())),
            kline_queries: Arc::new(Mutex::new(Vec::new())),
        }
    }
}

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

fn build_depth_response(last_update_id: i64, bids: &[(i64, i64)], asks: &[(i64, i64)]) -> Vec<u8> {
    let header = create_sbe_header(10, DEPTH_TEMPLATE_ID);
    let mut buf = Vec::new();
    buf.extend_from_slice(&header);
    buf.extend_from_slice(&last_update_id.to_le_bytes());
    buf.push((-2i8) as u8);
    buf.push((-5i8) as u8);

    buf.extend_from_slice(&create_group_header(16, bids.len() as u32));
    for (price, qty) in bids {
        buf.extend_from_slice(&price.to_le_bytes());
        buf.extend_from_slice(&qty.to_le_bytes());
    }

    buf.extend_from_slice(&create_group_header(16, asks.len() as u32));
    for (price, qty) in asks {
        buf.extend_from_slice(&price.to_le_bytes());
        buf.extend_from_slice(&qty.to_le_bytes());
    }

    buf
}

fn build_agg_trades_response(time_ms: i64) -> Vec<u8> {
    let mut buf = create_sbe_header(2, AGG_TRADES_TEMPLATE_ID).to_vec();
    buf.push((-2_i8) as u8);
    buf.push((-5_i8) as u8);
    buf.extend_from_slice(&create_group_header(50, 1));
    buf.extend_from_slice(&101_i64.to_le_bytes());
    buf.extend_from_slice(&4_200_123_i64.to_le_bytes());
    buf.extend_from_slice(&12_345_i64.to_le_bytes());
    buf.extend_from_slice(&201_i64.to_le_bytes());
    buf.extend_from_slice(&207_i64.to_le_bytes());
    buf.extend_from_slice(&(time_ms * 1_000).to_le_bytes());
    buf.push(0);
    buf.push(1);
    buf
}

fn build_klines_response(close_time_us: i64, span_us: i64) -> Vec<u8> {
    let mut buf = create_sbe_header(2, KLINES_TEMPLATE_ID).to_vec();
    buf.push((-2_i8) as u8);
    buf.push((-5_i8) as u8);
    buf.extend_from_slice(&create_group_header(120, 1));
    buf.extend_from_slice(&(close_time_us - span_us).to_le_bytes());
    buf.extend_from_slice(&4_200_000_i64.to_le_bytes());
    buf.extend_from_slice(&4_200_300_i64.to_le_bytes());
    buf.extend_from_slice(&4_199_900_i64.to_le_bytes());
    buf.extend_from_slice(&4_200_200_i64.to_le_bytes());
    buf.extend_from_slice(&750_000_i128.to_le_bytes());
    buf.extend_from_slice(&close_time_us.to_le_bytes());
    buf.extend_from_slice(&31_500_750_i128.to_le_bytes());
    buf.extend_from_slice(&7_i64.to_le_bytes());
    buf.extend_from_slice(&325_000_i128.to_le_bytes());
    buf.extend_from_slice(&13_650_125_i128.to_le_bytes());
    buf
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

fn sbe_response(body: Vec<u8>) -> impl IntoResponse {
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "application/sbe")],
        Body::from(body),
    )
}

fn build_sbe_trades_stream_event(symbol: &str) -> Vec<u8> {
    let trade_block_len = 25u16;
    let num_trades = 1usize;
    let body_size = 18 + 6 + (num_trades * trade_block_len as usize) + 1 + symbol.len();
    let mut buf = vec![0u8; 8 + body_size];

    // Header (stream schema)
    buf[0..2].copy_from_slice(&18u16.to_le_bytes()); // block_length
    buf[2..4].copy_from_slice(&template_id::TRADES_STREAM_EVENT.to_le_bytes());
    buf[4..6].copy_from_slice(&STREAM_SCHEMA_ID.to_le_bytes());
    buf[6..8].copy_from_slice(&0u16.to_le_bytes()); // version

    // Body
    let body = &mut buf[8..];
    body[0..8].copy_from_slice(&1_000_000i64.to_le_bytes()); // event_time_us
    body[8..16].copy_from_slice(&1_000_001i64.to_le_bytes()); // transact_time_us
    body[16] = (-2i8) as u8; // price_exponent
    body[17] = (-8i8) as u8; // qty_exponent

    // Group header (trades)
    body[18..20].copy_from_slice(&trade_block_len.to_le_bytes());
    body[20..24].copy_from_slice(&(num_trades as u32).to_le_bytes());

    // Trade entry
    let offset = 24;
    body[offset..offset + 8].copy_from_slice(&1i64.to_le_bytes()); // id
    body[offset + 8..offset + 16].copy_from_slice(&4_200_000i64.to_le_bytes()); // price
    body[offset + 16..offset + 24].copy_from_slice(&100_000_000i64.to_le_bytes()); // qty
    body[offset + 24] = 1; // is_buyer_maker

    // Symbol varString8
    let sym_offset = offset + trade_block_len as usize;
    body[sym_offset] = symbol.len() as u8;
    body[sym_offset + 1..sym_offset + 1 + symbol.len()].copy_from_slice(symbol.as_bytes());

    buf
}

fn build_sbe_depth_snapshot_stream_event(symbol: &str) -> Vec<u8> {
    let level_block_len = 16u16; // price i64 + qty i64
    let num_bids = 2u16;
    let num_asks = 2u16;

    // Body: 18 fixed + 2 group headers (4 bytes each, u16+u16) + levels + symbol var
    let body_size = 18
        + 4
        + (num_bids as usize * level_block_len as usize)
        + 4
        + (num_asks as usize * level_block_len as usize)
        + 1
        + symbol.len();
    let mut buf = vec![0u8; 8 + body_size];

    // Header (stream schema)
    buf[0..2].copy_from_slice(&18u16.to_le_bytes()); // block_length
    buf[2..4].copy_from_slice(&template_id::DEPTH_SNAPSHOT_STREAM_EVENT.to_le_bytes());
    buf[4..6].copy_from_slice(&STREAM_SCHEMA_ID.to_le_bytes());
    buf[6..8].copy_from_slice(&0u16.to_le_bytes()); // version

    // Body
    let body = &mut buf[8..];
    body[0..8].copy_from_slice(&1_000_000i64.to_le_bytes()); // event_time_us
    body[8..16].copy_from_slice(&99999i64.to_le_bytes()); // book_update_id
    body[16] = (-2i8) as u8; // price_exponent
    body[17] = (-8i8) as u8; // qty_exponent

    // Bids group header (u16 block_length + u16 num_in_group)
    let mut off = 18;
    body[off..off + 2].copy_from_slice(&level_block_len.to_le_bytes());
    body[off + 2..off + 4].copy_from_slice(&num_bids.to_le_bytes());
    off += 4;

    // Bid 1: price 42000.00, qty 1.00000000
    body[off..off + 8].copy_from_slice(&4_200_000i64.to_le_bytes());
    body[off + 8..off + 16].copy_from_slice(&100_000_000i64.to_le_bytes());
    off += level_block_len as usize;

    // Bid 2: price 41999.00, qty 2.00000000
    body[off..off + 8].copy_from_slice(&4_199_900i64.to_le_bytes());
    body[off + 8..off + 16].copy_from_slice(&200_000_000i64.to_le_bytes());
    off += level_block_len as usize;

    // Asks group header
    body[off..off + 2].copy_from_slice(&level_block_len.to_le_bytes());
    body[off + 2..off + 4].copy_from_slice(&num_asks.to_le_bytes());
    off += 4;

    // Ask 1: price 42001.00, qty 0.50000000
    body[off..off + 8].copy_from_slice(&4_200_100i64.to_le_bytes());
    body[off + 8..off + 16].copy_from_slice(&50_000_000i64.to_le_bytes());
    off += level_block_len as usize;

    // Ask 2: price 42002.00, qty 1.50000000
    body[off..off + 8].copy_from_slice(&4_200_200i64.to_le_bytes());
    body[off + 8..off + 16].copy_from_slice(&150_000_000i64.to_le_bytes());
    off += level_block_len as usize;

    // Symbol varString8
    body[off] = symbol.len() as u8;
    body[off + 1..off + 1 + symbol.len()].copy_from_slice(symbol.as_bytes());

    buf
}

fn build_sbe_depth_diff_stream_event(
    symbol: &str,
    first_update_id: i64,
    last_update_id: i64,
) -> Vec<u8> {
    let level_block_len = 16u16;
    let num_bids = 1u16;
    let num_asks = 0u16;
    let body_size = 26 + 4 + (num_bids as usize * level_block_len as usize) + 4 + 1 + symbol.len();
    let mut buf = vec![0u8; 8 + body_size];

    buf[0..2].copy_from_slice(&26u16.to_le_bytes());
    buf[2..4].copy_from_slice(&template_id::DEPTH_DIFF_STREAM_EVENT.to_le_bytes());
    buf[4..6].copy_from_slice(&STREAM_SCHEMA_ID.to_le_bytes());
    buf[6..8].copy_from_slice(&0u16.to_le_bytes());

    let body = &mut buf[8..];
    body[0..8].copy_from_slice(&1_000_100i64.to_le_bytes());
    body[8..16].copy_from_slice(&first_update_id.to_le_bytes());
    body[16..24].copy_from_slice(&last_update_id.to_le_bytes());
    body[24] = (-2i8) as u8;
    body[25] = (-5i8) as u8;

    let mut off = 26;
    body[off..off + 2].copy_from_slice(&level_block_len.to_le_bytes());
    body[off + 2..off + 4].copy_from_slice(&num_bids.to_le_bytes());
    off += 4;

    body[off..off + 8].copy_from_slice(&4_199_900i64.to_le_bytes());
    body[off + 8..off + 16].copy_from_slice(&125_000i64.to_le_bytes());
    off += level_block_len as usize;

    body[off..off + 2].copy_from_slice(&level_block_len.to_le_bytes());
    body[off + 2..off + 4].copy_from_slice(&num_asks.to_le_bytes());
    off += 4;

    body[off] = symbol.len() as u8;
    body[off + 1..off + 1 + symbol.len()].copy_from_slice(symbol.as_bytes());

    buf
}

fn build_json_depth_diff_stream_event(
    symbol: &str,
    first_update_id: i64,
    last_update_id: i64,
) -> String {
    json!({
        "e": "depthUpdate",
        "E": 1_001,
        "s": symbol,
        "U": first_update_id,
        "u": last_update_id,
        "b": [["41999.00", "1.25000"]],
        "a": [],
    })
    .to_string()
}

fn build_json_partial_depth_stream_event(symbol: &str) -> String {
    json!({
        "stream": format!("{}@depth20", symbol.to_lowercase()),
        "data": {
            "lastUpdateId": 99_999,
            "bids": [["42000.00", "1.00000"]],
            "asks": [["42001.00", "0.50000"]],
        },
    })
    .to_string()
}

fn build_json_book_ticker_stream_event(symbol: &str) -> String {
    json!({
        "stream": format!("{}@bookTicker", symbol.to_lowercase()),
        "data": {
            "u": 12345,
            "s": symbol,
            "b": "42000.00",
            "B": "1.25000",
            "a": "42001.00",
            "A": "2.50000"
        }
    })
    .to_string()
}

fn build_json_kline_stream_event(symbol: &str) -> String {
    json!({
        "stream": format!("{}@kline_1s", symbol.to_lowercase()),
        "data": {
            "e": "kline",
            "E": 1700000000999_i64,
            "s": symbol,
            "k": {
                "t": 1700000000000_i64,
                "T": 1700000000999_i64,
                "s": symbol,
                "i": "1s",
                "f": 201,
                "L": 207,
                "o": "42000.00",
                "c": "42002.00",
                "h": "42003.00",
                "l": "41999.00",
                "v": "7.50000",
                "n": 7,
                "x": true,
                "q": "315007.50",
                "V": "3.25000",
                "Q": "136501.25"
            }
        }
    })
    .to_string()
}

fn build_json_ticker_stream_event(symbol: &str) -> String {
    json!({
        "stream": format!("{}@ticker", symbol.to_lowercase()),
        "data": {
            "e": "24hrTicker",
            "E": 1700000000999_i64,
            "s": symbol,
            "p": "100.01",
            "P": "0.25",
            "w": "41950.50",
            "x": "41900.00",
            "c": "42000.01",
            "Q": "0.01000",
            "b": "42000.00",
            "B": "1.25000",
            "a": "42000.02",
            "A": "2.50000",
            "o": "41900.00",
            "h": "42500.00",
            "l": "41500.00",
            "v": "1234.50000",
            "q": "51777172.50",
            "O": 1699913600999_i64,
            "C": 1700000000998_i64,
            "F": 301,
            "L": 399,
            "n": 99
        }
    })
    .to_string()
}

fn build_sbe_best_bid_ask_stream_event(symbol: &str) -> Vec<u8> {
    let body_size = 50 + 1 + symbol.len();
    let mut buf = vec![0u8; 8 + body_size];

    // Header (stream schema)
    buf[0..2].copy_from_slice(&50u16.to_le_bytes()); // block_length
    buf[2..4].copy_from_slice(&template_id::BEST_BID_ASK_STREAM_EVENT.to_le_bytes());
    buf[4..6].copy_from_slice(&STREAM_SCHEMA_ID.to_le_bytes());
    buf[6..8].copy_from_slice(&0u16.to_le_bytes()); // version

    // Body
    let body = &mut buf[8..];
    body[0..8].copy_from_slice(&1_000_000i64.to_le_bytes()); // event_time_us
    body[8..16].copy_from_slice(&12345i64.to_le_bytes()); // book_update_id
    body[16] = (-2i8) as u8; // price_exponent
    body[17] = (-8i8) as u8; // qty_exponent
    body[18..26].copy_from_slice(&4_200_000i64.to_le_bytes()); // bid_price
    body[26..34].copy_from_slice(&100_000_000i64.to_le_bytes()); // bid_qty
    body[34..42].copy_from_slice(&4_200_100i64.to_le_bytes()); // ask_price
    body[42..50].copy_from_slice(&200_000_000i64.to_le_bytes()); // ask_qty

    // Symbol varString8
    body[50] = symbol.len() as u8;
    body[51..51 + symbol.len()].copy_from_slice(symbol.as_bytes());

    buf
}

async fn handle_ws(
    State(config): State<DataTestServerConfig>,
    ws: axum::extract::WebSocketUpgrade,
) -> Response {
    ws.on_upgrade(move |socket| handle_ws_connection(socket, config))
}

async fn handle_ws_connection(mut socket: WebSocket, config: DataTestServerConfig) {
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
                    let streams = params
                        .iter()
                        .filter_map(|param| param.as_str().map(str::to_string))
                        .collect::<Vec<_>>();

                    if !streams.is_empty() {
                        config.subscriptions.lock().unwrap().push(streams.clone());
                    }

                    for stream in streams {
                        if stream.contains("@trade") {
                            let symbol =
                                stream.split('@').next().unwrap_or("BTCUSDT").to_uppercase();
                            let data = build_sbe_trades_stream_event(&symbol);
                            tokio::time::sleep(Duration::from_millis(50)).await;
                            let _result = socket.send(Message::Binary(data.into())).await;
                        } else if stream.contains("@bestBidAsk") {
                            let symbol =
                                stream.split('@').next().unwrap_or("BTCUSDT").to_uppercase();
                            let data = build_sbe_best_bid_ask_stream_event(&symbol);
                            tokio::time::sleep(Duration::from_millis(50)).await;
                            let _result = socket.send(Message::Binary(data.into())).await;
                        } else if stream.contains("@bookTicker") {
                            let symbol =
                                stream.split('@').next().unwrap_or("BTCUSDT").to_uppercase();
                            let data = build_json_book_ticker_stream_event(&symbol);
                            tokio::time::sleep(Duration::from_millis(50)).await;
                            let _result = socket.send(Message::Text(data.into())).await;
                        } else if stream.contains("@kline_") {
                            let symbol =
                                stream.split('@').next().unwrap_or("BTCUSDT").to_uppercase();
                            let data = build_json_kline_stream_event(&symbol);
                            tokio::time::sleep(Duration::from_millis(50)).await;
                            let _result = socket.send(Message::Text(data.into())).await;
                        } else if stream.contains("@ticker") {
                            let symbol =
                                stream.split('@').next().unwrap_or("BTCUSDT").to_uppercase();
                            let data = build_json_ticker_stream_event(&symbol);
                            tokio::time::sleep(Duration::from_millis(50)).await;
                            let _result = socket.send(Message::Text(data.into())).await;
                        } else if stream.contains("@depth") {
                            let symbol =
                                stream.split('@').next().unwrap_or("BTCUSDT").to_uppercase();
                            if stream.ends_with("@depth") {
                                for index in 0..config.depth_diff_repetitions {
                                    let update_offset = index as i64;
                                    tokio::time::sleep(config.depth_diff_delay).await;
                                    if config.json_ws_streams {
                                        let data = build_json_depth_diff_stream_event(
                                            &symbol,
                                            config.depth_diff_first_update_id + update_offset,
                                            config.depth_diff_last_update_id + update_offset,
                                        );
                                        let _result = socket.send(Message::Text(data.into())).await;
                                    } else {
                                        let data = build_sbe_depth_diff_stream_event(
                                            &symbol,
                                            config.depth_diff_first_update_id + update_offset,
                                            config.depth_diff_last_update_id + update_offset,
                                        );
                                        let _result =
                                            socket.send(Message::Binary(data.into())).await;
                                    }
                                }

                                if config
                                    .reconnect_signals_remaining
                                    .try_update(Ordering::Relaxed, Ordering::Relaxed, |remaining| {
                                        remaining.checked_sub(1)
                                    })
                                    .is_ok()
                                {
                                    tokio::time::sleep(Duration::from_millis(500)).await;
                                    let _result =
                                        socket.send(Message::Text(RECONNECTED.into())).await;
                                }
                            } else {
                                tokio::time::sleep(Duration::from_millis(50)).await;

                                if config.json_ws_streams {
                                    let data = build_json_partial_depth_stream_event(&symbol);
                                    let _result = socket.send(Message::Text(data.into())).await;
                                } else {
                                    let data = build_sbe_depth_snapshot_stream_event(&symbol);
                                    let _result = socket.send(Message::Binary(data.into())).await;
                                }
                            }
                        }
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
                    config.unsubscriptions.lock().unwrap().push(streams);
                }
            }
        }
    }
}

async fn handle_depth(State(config): State<DataTestServerConfig>) -> Response {
    let request = config.depth_requests.fetch_add(1, Ordering::Relaxed);
    let last_update_id = config
        .depth_snapshot_last_update_ids
        .get(request)
        .copied()
        .or_else(|| config.depth_snapshot_last_update_ids.last().copied())
        .unwrap_or(100);

    let bids = vec![(4_200_000, 100_000)];
    let asks = vec![(4_200_100, 200_000)];
    sbe_response(build_depth_response(last_update_id, &bids, &asks)).into_response()
}

async fn handle_agg_trades(
    State(config): State<DataTestServerConfig>,
    RawQuery(raw_query): RawQuery,
) -> Response {
    let query = serde_urlencoded::from_str::<HashMap<String, String>>(
        raw_query.as_deref().unwrap_or_default(),
    )
    .unwrap();
    let time_ms = query
        .get("startTime")
        .and_then(|value| value.parse().ok())
        .unwrap_or(1_700_000_000_123);
    config.agg_trade_queries.lock().unwrap().push(query);
    sbe_response(build_agg_trades_response(time_ms)).into_response()
}

async fn handle_klines(
    State(config): State<DataTestServerConfig>,
    RawQuery(raw_query): RawQuery,
) -> Response {
    let query = serde_urlencoded::from_str::<HashMap<String, String>>(
        raw_query.as_deref().unwrap_or_default(),
    )
    .unwrap();
    let close_time_us = query
        .get("endTime")
        .and_then(|value| value.parse::<i64>().ok())
        .map_or_else(
            || chrono::Utc::now().timestamp_micros() - 1_000_000,
            |value| value * 1_000,
        );
    let span_us = match query.get("interval").map(String::as_str) {
        Some("1s") => 999_000,
        _ => 59_999_000,
    };
    config.kline_queries.lock().unwrap().push(query);
    sbe_response(build_klines_response(close_time_us, span_us)).into_response()
}

fn create_data_test_router(config: DataTestServerConfig) -> Router {
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
        .route("/api/v3/depth", get(handle_depth))
        .route("/api/v3/aggTrades", get(handle_agg_trades))
        .route("/api/v3/klines", get(handle_klines))
        .route("/ws", get(handle_ws))
        .route("/stream", get(handle_ws))
        .with_state(config)
}

async fn start_data_test_server() -> SocketAddr {
    start_data_test_server_with_config(DataTestServerConfig::default()).await
}

async fn start_data_test_server_with_config(config: DataTestServerConfig) -> SocketAddr {
    let router = create_data_test_router(config);
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

    addr
}

fn create_test_data_client(
    base_url_http: String,
    base_url_ws: String,
) -> (
    BinanceSpotDataClient,
    tokio::sync::mpsc::UnboundedReceiver<DataEvent>,
) {
    create_test_data_client_with_mode(base_url_http, base_url_ws, BinanceSpotMarketDataMode::Sbe)
}

fn create_test_data_client_with_mode(
    base_url_http: String,
    base_url_ws: String,
    spot_market_data_mode: BinanceSpotMarketDataMode,
) -> (
    BinanceSpotDataClient,
    tokio::sync::mpsc::UnboundedReceiver<DataEvent>,
) {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    set_data_event_sender(tx);

    let config = BinanceDataClientConfig {
        base_url_http: Some(base_url_http),
        base_url_ws: Some(base_url_ws),
        api_key: Some("test-api-key".to_string()),
        api_secret: Some(
            "MC4CAQAwBQYDK2VwBCIEIJ1hsZ3v/VpguoRK9JLsLMREScVpezJpGXA7rAMcrn9g".to_string(),
        ),
        spot_market_data_mode,
        ..Default::default()
    };

    let client = BinanceSpotDataClient::new(*BINANCE_CLIENT_ID, config).unwrap();

    (client, rx)
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

fn recorded_streams_include(records: &Arc<Mutex<Vec<Vec<String>>>>, stream: &str) -> bool {
    records
        .lock()
        .unwrap()
        .iter()
        .any(|streams| streams.iter().any(|recorded| recorded == stream))
}

fn spot_ticker_data_type(instrument_id: InstrumentId) -> DataType {
    let mut metadata = Params::new();
    metadata.insert(
        "instrument_id".to_string(),
        serde_json::Value::String(instrument_id.to_string()),
    );
    DataType::new(
        "BinanceSpotTicker",
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

    let instrument_id = InstrumentId::from("BTCUSDT.BINANCE");
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

    let instrument_id = InstrumentId::from("BTCUSDT.BINANCE");
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
async fn test_subscribe_one_second_bars_emits_core_and_binance_bars() {
    let state = DataTestServerConfig {
        json_ws_streams: true,
        ..Default::default()
    };
    let addr = start_data_test_server_with_config(state.clone()).await;
    let (mut client, mut rx) = create_test_data_client_with_mode(
        format!("http://{addr}"),
        format!("ws://{addr}/ws"),
        BinanceSpotMarketDataMode::Json,
    );
    client.connect().await.unwrap();

    while rx.try_recv().is_ok() {}

    let bar_type = BarType::from("BTCUSDT.BINANCE-1-SECOND-LAST-EXTERNAL");
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

    wait_until_async(
        || {
            let found = recorded_streams_include(&state.subscriptions, "btcusdt@kline_1s");
            async move { found }
        },
        Duration::from_secs(5),
    )
    .await;

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
    assert_eq!(core.bar_type, bar_type);
    assert_eq!(core.open.as_decimal(), dec!(42000.00));
    assert_eq!(core.high.as_decimal(), dec!(42003.00));
    assert_eq!(core.low.as_decimal(), dec!(41999.00));
    assert_eq!(core.close.as_decimal(), dec!(42002.00));
    assert_eq!(core.volume.as_decimal(), dec!(7.50000));
    assert_eq!(payload.bar_type, bar_type);
    assert_eq!(payload.open, core.open);
    assert_eq!(payload.high, core.high);
    assert_eq!(payload.low, core.low);
    assert_eq!(payload.close, core.close);
    assert_eq!(payload.volume, core.volume);
    assert_eq!(payload.quote_volume, dec!(315007.50));
    assert_eq!(payload.count, 7);
    assert_eq!(payload.taker_buy_base_volume, dec!(3.25000));
    assert_eq!(payload.taker_buy_quote_volume, dec!(136501.25));
    assert_eq!(payload.ts_event, core.ts_event);
    assert_eq!(payload.ts_init, core.ts_init);
}

#[rstest]
#[tokio::test]
async fn test_subscribe_bars_rejects_spot_sbe_transport() {
    let addr = start_data_test_server().await;
    let (mut client, _rx) =
        create_test_data_client(format!("http://{addr}"), format!("ws://{addr}/ws"));
    let bar_type = BarType::from("BTCUSDT.BINANCE-1-SECOND-LAST-EXTERNAL");

    let error = client
        .subscribe_bars(SubscribeBars::new(
            bar_type,
            Some(*BINANCE_CLIENT_ID),
            None,
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ))
        .unwrap_err();

    assert_eq!(
        error.to_string(),
        "Binance Spot kline subscriptions require JSON market-data mode"
    );
}

#[rstest]
#[tokio::test]
async fn test_subscribe_spot_ticker_emits_custom_data() {
    let state = DataTestServerConfig {
        json_ws_streams: true,
        ..Default::default()
    };
    let addr = start_data_test_server_with_config(state.clone()).await;
    let (mut client, mut rx) = create_test_data_client_with_mode(
        format!("http://{addr}"),
        format!("ws://{addr}/ws"),
        BinanceSpotMarketDataMode::Json,
    );
    client.connect().await.unwrap();

    while rx.try_recv().is_ok() {}

    let instrument_id = InstrumentId::from("BTCUSDT.BINANCE");
    let data_type = spot_ticker_data_type(instrument_id);
    client
        .subscribe(SubscribeCustomData::new(
            Some(*BINANCE_CLIENT_ID),
            None,
            data_type.clone(),
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ))
        .unwrap();

    let data = recv_data(&mut rx, Duration::from_secs(5))
        .await
        .expect("expected Spot ticker custom data");
    let Data::Custom(custom) = data else {
        panic!("expected Spot ticker custom data");
    };
    let ticker = custom
        .data
        .as_any()
        .downcast_ref::<BinanceSpotTicker>()
        .expect("expected BinanceSpotTicker payload");

    assert!(recorded_streams_include(
        &state.subscriptions,
        "btcusdt@ticker"
    ));
    assert_eq!(custom.data_type, data_type);
    assert_eq!(ticker.instrument_id, instrument_id);
    assert_eq!(ticker.last_price, dec!(42000.01));
    assert_eq!(ticker.bid_price, dec!(42000.00));
    assert_eq!(ticker.ask_price, dec!(42000.02));
    assert_eq!(ticker.volume, dec!(1234.50000));
    assert_eq!(ticker.quote_volume, dec!(51777172.50));
    assert_eq!(ticker.num_trades, 99);
}

#[rstest]
#[tokio::test]
async fn test_subscribe_spot_ticker_rejects_sbe_transport() {
    let addr = start_data_test_server().await;
    let (mut client, _rx) =
        create_test_data_client(format!("http://{addr}"), format!("ws://{addr}/ws"));
    let data_type = spot_ticker_data_type(InstrumentId::from("BTCUSDT.BINANCE"));

    let error = client
        .subscribe(SubscribeCustomData::new(
            Some(*BINANCE_CLIENT_ID),
            None,
            data_type,
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ))
        .unwrap_err();

    assert_eq!(
        error.to_string(),
        "Binance Spot 24-hour ticker custom data requires JSON market-data mode"
    );
}

#[rstest]
#[tokio::test]
async fn test_request_bounded_aggregate_trades_routes_spot_bounds() {
    let state = DataTestServerConfig::default();
    let addr = start_data_test_server_with_config(state.clone()).await;
    let (mut client, mut rx) =
        create_test_data_client(format!("http://{addr}"), format!("ws://{addr}/ws"));
    client.connect().await.unwrap();

    while rx.try_recv().is_ok() {}
    let instrument_id = InstrumentId::from("BTCUSDT.BINANCE");
    let start = chrono::DateTime::from_timestamp_millis(1_700_000_000_123).unwrap();
    let end = chrono::DateTime::from_timestamp_millis(1_700_000_000_999).unwrap();

    client
        .request_trades(RequestTrades::new(
            instrument_id,
            Some(start),
            Some(end),
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
    let query = state.agg_trade_queries.lock().unwrap()[0].clone();
    assert_eq!(query.get("symbol").map(String::as_str), Some("BTCUSDT"));
    assert_eq!(
        query.get("startTime").map(String::as_str),
        Some("1700000000123")
    );
    assert_eq!(
        query.get("endTime").map(String::as_str),
        Some("1700000000999")
    );
    assert_eq!(query.get("limit").map(String::as_str), Some("456"));
    assert_eq!(response.instrument_id, instrument_id);
    assert_eq!(response.data.len(), 1);
    assert_eq!(response.data[0].price.as_decimal(), dec!(42001.23));
    assert_eq!(response.data[0].size.as_decimal(), dec!(0.12345));
    assert_eq!(response.data[0].trade_id.to_string(), "101");
    assert_eq!(
        response.data[0].ts_event,
        UnixNanos::from_millis(1_700_000_000_123)
    );
    assert_eq!(response.data[0].ts_init, response.data[0].ts_event);
}

#[rstest]
#[tokio::test]
async fn test_request_historical_one_second_binance_bars_preserves_fields() {
    let state = DataTestServerConfig::default();
    let addr = start_data_test_server_with_config(state.clone()).await;
    let (mut client, mut rx) =
        create_test_data_client(format!("http://{addr}"), format!("ws://{addr}/ws"));
    client.connect().await.unwrap();

    while rx.try_recv().is_ok() {}
    let bar_type = BarType::from("BTCUSDT.BINANCE-1-SECOND-LAST-EXTERNAL");
    let data_type = binance_bar_data_type(bar_type);
    let start = chrono::DateTime::from_timestamp_millis(1_700_000_000_000).unwrap();
    let end = chrono::DateTime::from_timestamp_millis(1_700_000_000_999).unwrap();

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
    let query = state.kline_queries.lock().unwrap()[0].clone();
    assert_eq!(query.get("symbol").map(String::as_str), Some("BTCUSDT"));
    assert_eq!(query.get("interval").map(String::as_str), Some("1s"));
    assert_eq!(
        query.get("startTime").map(String::as_str),
        Some("1700000000000")
    );
    assert_eq!(
        query.get("endTime").map(String::as_str),
        Some("1700000000999")
    );
    assert_eq!(query.get("limit").map(String::as_str), Some("321"));
    assert_eq!(response.data_type, data_type);
    assert_eq!(bars.len(), 1);
    assert_eq!(bars[0].bar_type, bar_type);
    assert_eq!(bars[0].open.as_decimal(), dec!(42000.00));
    assert_eq!(bars[0].high.as_decimal(), dec!(42003.00));
    assert_eq!(bars[0].low.as_decimal(), dec!(41999.00));
    assert_eq!(bars[0].close.as_decimal(), dec!(42002.00));
    assert_eq!(bars[0].volume.as_decimal(), dec!(7.50000));
    assert_eq!(bars[0].quote_volume, dec!(315007.50));
    assert_eq!(bars[0].count, 7);
    assert_eq!(bars[0].taker_buy_base_volume, dec!(3.25000));
    assert_eq!(bars[0].taker_buy_quote_volume, dec!(136501.25));
    assert_eq!(bars[0].ts_init, bars[0].ts_event);
    let expected_bar = bars[0].clone();

    client
        .request_bars(RequestBars::new(
            bar_type,
            Some(start),
            Some(end),
            Some(NonZeroUsize::new(123).unwrap()),
            Some(*BINANCE_CLIENT_ID),
            UUID4::new(),
            UnixNanos::default(),
            None,
        ))
        .unwrap();
    let event = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("timeout waiting for core bar history")
        .expect("data channel closed");
    let DataEvent::Response(DataResponse::Bars(response)) = event else {
        panic!("expected core bars response");
    };
    let queries = state.kline_queries.lock().unwrap();
    assert_eq!(queries.len(), 2);
    assert_eq!(queries[1].get("interval").map(String::as_str), Some("1s"));
    assert_eq!(queries[1].get("limit").map(String::as_str), Some("123"));
    assert_eq!(response.bar_type, bar_type);
    assert_eq!(response.data.len(), 1);
    assert_eq!(response.data[0], expected_bar.bar());
}

#[rstest]
#[case(
    "BTCUSDT.BINANCE-1-MINUTE-BID-EXTERNAL",
    "historical BinanceBar requests require LAST price type"
)]
#[case(
    "BTCUSDT.BINANCE-1-TICK-LAST-EXTERNAL",
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
    "BTCUSDT.BINANCE-1-MINUTE-LAST-INTERNAL",
    "Binance historical bars require EXTERNAL aggregation"
)]
#[case(
    "BTCUSDT.BINANCE-1-TICK-LAST-EXTERNAL",
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
async fn test_subscribe_l1_mbp_uses_sbe_best_bid_ask() {
    let state = DataTestServerConfig::default();
    let addr = start_data_test_server_with_config(state.clone()).await;
    let (mut client, mut rx) =
        create_test_data_client(format!("http://{addr}"), format!("ws://{addr}/ws"));
    client.connect().await.unwrap();

    while rx.try_recv().is_ok() {}
    let instrument_id = InstrumentId::from("BTCUSDT.BINANCE");

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
        .expect("expected SBE top-of-book quote");
    let Data::Quote(quote) = data else {
        panic!("expected top-of-book quote");
    };
    assert!(recorded_streams_include(
        &state.subscriptions,
        "btcusdt@bestBidAsk"
    ));
    assert_eq!(quote.instrument_id, instrument_id);
    assert_eq!(quote.bid_price.as_decimal(), dec!(42000.00));
    assert_eq!(quote.bid_size.as_decimal(), dec!(1.00000));
    assert_eq!(quote.ask_price.as_decimal(), dec!(42001.00));
    assert_eq!(quote.ask_size.as_decimal(), dec!(2.00000));

    let data = recv_data(&mut rx, Duration::from_secs(5))
        .await
        .expect("expected SBE L1 deltas");
    let Data::Deltas(deltas) = data else {
        panic!("expected SBE L1 deltas");
    };
    assert_eq!(deltas.into_inner(), expected_l1_deltas(quote, 12345));
}

#[rstest]
#[tokio::test]
async fn test_subscribe_l1_mbp_uses_json_top_of_book_and_rejects_invalid_depth() {
    let state = DataTestServerConfig {
        json_ws_streams: true,
        ..Default::default()
    };
    let addr = start_data_test_server_with_config(state.clone()).await;
    let (mut client, mut rx) = create_test_data_client_with_mode(
        format!("http://{addr}"),
        format!("ws://{addr}/ws"),
        BinanceSpotMarketDataMode::Json,
    );
    client.connect().await.unwrap();

    while rx.try_recv().is_ok() {}
    let instrument_id = InstrumentId::from("BTCUSDT.BINANCE");

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
    assert_eq!(quote.bid_price.as_decimal(), dec!(42000.00));
    assert_eq!(quote.bid_size.as_decimal(), dec!(1.25000));
    assert_eq!(quote.ask_price.as_decimal(), dec!(42001.00));
    assert_eq!(quote.ask_size.as_decimal(), dec!(2.50000));

    let data = recv_data(&mut rx, Duration::from_secs(5))
        .await
        .expect("expected JSON L1 deltas");
    let Data::Deltas(deltas) = data else {
        panic!("expected JSON L1 deltas");
    };
    assert_eq!(deltas.into_inner(), expected_l1_deltas(quote, 12345));

    let invalid = client.subscribe_book_deltas(SubscribeBookDeltas::new(
        InstrumentId::from("ETHUSDT.BINANCE"),
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
        "Binance Spot L1_MBP supports depth 1 only"
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
        "cannot subscribe L1_MBP and L2_MBP for the same Binance Spot instrument"
    );
}

#[rstest]
#[tokio::test]
async fn test_subscribe_book_deltas_with_partial_depth_stream() {
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

    let instrument_id = InstrumentId::from("BTCUSDT.BINANCE");
    let cmd = SubscribeBookDeltas::new(
        instrument_id,
        BookType::L2_MBP,
        Some(*BINANCE_CLIENT_ID),
        None,
        nautilus_core::UUID4::new(),
        UnixNanos::default(),
        Some(NonZeroUsize::new(20).unwrap()),
        false,
        None,
        None,
    );

    client.subscribe_book_deltas(cmd).unwrap();

    let data = recv_data(&mut rx, Duration::from_secs(5))
        .await
        .expect("expected partial depth snapshot data");
    let Data::Deltas(deltas) = data else {
        panic!("expected order book deltas");
    };

    assert_eq!(deltas.sequence, 99_999);
    assert_eq!(deltas.deltas[0].action, BookAction::Clear);
    assert_eq!(deltas.deltas[1].action, BookAction::Add);
    assert_eq!(deltas.deltas[1].order.side, OrderSide::Buy);
    assert_eq!(deltas.deltas[1].order.price.as_decimal(), dec!(42000.00));
    assert_eq!(deltas.deltas[1].order.size.as_decimal(), dec!(1.00000));
    assert_eq!(
        deltas.deltas.last().unwrap().flags,
        RecordFlag::F_LAST as u8
    );
}

#[rstest]
#[tokio::test]
async fn test_request_book_snapshot_returns_exact_spot_book() {
    let addr = start_data_test_server().await;
    let (mut client, mut rx) =
        create_test_data_client(format!("http://{addr}"), format!("ws://{addr}/ws"));
    client.connect().await.unwrap();

    while rx.try_recv().is_ok() {}
    let instrument_id = InstrumentId::from("BTCUSDT.BINANCE");

    client
        .request_book_snapshot(RequestBookSnapshot::new(
            instrument_id,
            Some(NonZeroUsize::new(2).unwrap()),
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
    assert_eq!(bids.len(), 1);
    assert_eq!(asks.len(), 1);
    assert_eq!(bids[0].price.value.as_decimal(), dec!(42000.00));
    assert_eq!(bids[0].size_decimal(), dec!(1.00000));
    assert_eq!(asks[0].price.value.as_decimal(), dec!(42001.00));
    assert_eq!(asks[0].size_decimal(), dec!(2.00000));

    let error = client
        .request_book_snapshot(RequestBookSnapshot::new(
            instrument_id,
            Some(NonZeroUsize::new(5001).unwrap()),
            Some(*BINANCE_CLIENT_ID),
            UUID4::new(),
            UnixNanos::default(),
            None,
        ))
        .unwrap_err();
    assert_eq!(
        error.to_string(),
        "Binance Spot order-book depth must be between 1 and 5000"
    );
}

#[rstest]
#[tokio::test]
async fn test_subscribe_book_deltas_full_depth_replays_buffered_diff_after_snapshot() {
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

    let instrument_id = InstrumentId::from("BTCUSDT.BINANCE");
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
    let Data::Deltas(snapshot) = snapshot else {
        panic!("expected order book deltas");
    };

    let replayed = recv_data(&mut rx, Duration::from_secs(5))
        .await
        .expect("expected replayed depth diff data");
    let Data::Deltas(replayed) = replayed else {
        panic!("expected order book deltas");
    };

    assert_eq!(snapshot.sequence, 100);
    assert_eq!(snapshot.ts_event, replayed.ts_event);
    assert_eq!(snapshot.deltas[0].action, BookAction::Clear);
    assert_eq!(snapshot.deltas[1].action, BookAction::Add);
    assert_eq!(snapshot.deltas[1].order.side, OrderSide::Buy);
    assert_eq!(snapshot.deltas[1].order.price.as_decimal(), dec!(42000.00));
    assert_eq!(snapshot.deltas[1].order.size.as_decimal(), dec!(1.00000));
    assert_eq!(
        snapshot.deltas.last().unwrap().flags,
        RecordFlag::F_LAST as u8
    );

    assert_eq!(replayed.sequence, 101);
    assert_eq!(replayed.deltas[0].action, BookAction::Update);
    assert_eq!(replayed.deltas[0].order.side, OrderSide::Buy);
    assert_eq!(replayed.deltas[0].order.price.as_decimal(), dec!(41999.00));
    assert_eq!(replayed.deltas[0].order.size.as_decimal(), dec!(1.25000));
    assert_eq!(replayed.deltas[0].flags, RecordFlag::F_LAST as u8);
}

#[rstest]
#[tokio::test]
async fn test_subscribe_book_deltas_json_full_depth_replays_buffered_diff_after_snapshot() {
    let config = DataTestServerConfig {
        json_ws_streams: true,
        ..Default::default()
    };
    let addr = start_data_test_server_with_config(config).await;
    let base_url_http = format!("http://{addr}");
    let base_url_ws = format!("ws://{addr}/ws");

    let (mut client, mut rx) = create_test_data_client_with_mode(
        base_url_http,
        base_url_ws,
        BinanceSpotMarketDataMode::Json,
    );

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

    let instrument_id = InstrumentId::from("BTCUSDT.BINANCE");
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
    let Data::Deltas(snapshot) = snapshot else {
        panic!("expected order book deltas");
    };

    let replayed = recv_data(&mut rx, Duration::from_secs(5))
        .await
        .expect("expected replayed JSON depth diff data");
    let Data::Deltas(replayed) = replayed else {
        panic!("expected order book deltas");
    };

    assert_eq!(snapshot.sequence, 100);
    assert_eq!(snapshot.ts_event, replayed.ts_event);
    assert_eq!(snapshot.deltas[0].action, BookAction::Clear);
    assert_eq!(snapshot.deltas[1].order.price.as_decimal(), dec!(42000.00));
    assert_eq!(snapshot.deltas[1].order.size.as_decimal(), dec!(1.00000));
    assert_eq!(replayed.sequence, 101);
    assert_eq!(replayed.deltas[0].action, BookAction::Update);
    assert_eq!(replayed.deltas[0].order.side, OrderSide::Buy);
    assert_eq!(replayed.deltas[0].order.price.as_decimal(), dec!(41999.00));
    assert_eq!(replayed.deltas[0].order.size.as_decimal(), dec!(1.25000));
    assert_eq!(replayed.deltas[0].flags, RecordFlag::F_LAST as u8);
}

#[rstest]
#[tokio::test]
async fn test_subscribe_book_deltas_json_full_depth_rebuilds_after_reconnect() {
    let config = DataTestServerConfig {
        depth_snapshot_last_update_ids: vec![100, 100],
        json_ws_streams: true,
        reconnect_signals_remaining: Arc::new(AtomicUsize::new(1)),
        ..Default::default()
    };
    let depth_requests = config.depth_requests.clone();
    let addr = start_data_test_server_with_config(config).await;
    let base_url_http = format!("http://{addr}");
    let base_url_ws = format!("ws://{addr}/ws");

    let (mut client, mut rx) = create_test_data_client_with_mode(
        base_url_http,
        base_url_ws,
        BinanceSpotMarketDataMode::Json,
    );

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

    let instrument_id = InstrumentId::from("BTCUSDT.BINANCE");
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

    let first_snapshot = recv_data(&mut rx, Duration::from_secs(5))
        .await
        .expect("expected initial REST depth snapshot data");
    let Data::Deltas(first_snapshot) = first_snapshot else {
        panic!("expected order book deltas");
    };

    let first_replayed = recv_data(&mut rx, Duration::from_secs(5))
        .await
        .expect("expected initial replayed JSON depth diff data");
    let Data::Deltas(first_replayed) = first_replayed else {
        panic!("expected order book deltas");
    };

    let second_snapshot = recv_data(&mut rx, Duration::from_secs(5))
        .await
        .expect("expected reconnect REST depth snapshot data");
    let Data::Deltas(second_snapshot) = second_snapshot else {
        panic!("expected order book deltas");
    };

    let second_replayed = recv_data(&mut rx, Duration::from_secs(5))
        .await
        .expect("expected reconnect replayed JSON depth diff data");
    let Data::Deltas(second_replayed) = second_replayed else {
        panic!("expected order book deltas");
    };

    assert_eq!(first_snapshot.sequence, 100);
    assert_eq!(first_replayed.sequence, 101);
    assert_eq!(second_snapshot.sequence, 100);
    assert_eq!(second_replayed.sequence, 101);
    assert_eq!(depth_requests.load(Ordering::Relaxed), 2);
}

#[rstest]
#[tokio::test]
async fn test_subscribe_book_deltas_full_depth_waits_for_first_diff_before_snapshot() {
    let config = DataTestServerConfig {
        depth_diff_delay: Duration::from_millis(500),
        ..Default::default()
    };
    let depth_requests = config.depth_requests.clone();
    let addr = start_data_test_server_with_config(config).await;
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

    let instrument_id = InstrumentId::from("BTCUSDT.BINANCE");
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
    let Data::Deltas(snapshot) = snapshot else {
        panic!("expected order book deltas");
    };

    let replayed = recv_data(&mut rx, Duration::from_secs(5))
        .await
        .expect("expected replayed depth diff data after first diff");
    let Data::Deltas(replayed) = replayed else {
        panic!("expected order book deltas");
    };

    assert_eq!(snapshot.sequence, 100);
    assert_eq!(snapshot.deltas[0].action, BookAction::Clear);
    assert_eq!(replayed.sequence, 101);
    assert_eq!(replayed.deltas[0].action, BookAction::Update);
}

#[rstest]
#[tokio::test]
async fn test_subscribe_book_deltas_full_depth_keeps_buffered_diffs_across_overlap_retry() {
    let config = DataTestServerConfig {
        depth_snapshot_last_update_ids: vec![99, 100],
        ..Default::default()
    };
    let depth_requests = config.depth_requests.clone();
    let addr = start_data_test_server_with_config(config).await;
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

    let instrument_id = InstrumentId::from("BTCUSDT.BINANCE");
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
    let Data::Deltas(snapshot) = snapshot else {
        panic!("expected order book deltas");
    };

    let replayed = recv_data(&mut rx, Duration::from_secs(5))
        .await
        .expect("expected retained replayed depth diff data");
    let Data::Deltas(replayed) = replayed else {
        panic!("expected order book deltas");
    };

    assert_eq!(snapshot.sequence, 100);
    assert_eq!(replayed.sequence, 101);
    assert_eq!(replayed.deltas[0].action, BookAction::Update);
    assert_eq!(depth_requests.load(Ordering::Relaxed), 2);
}

#[rstest]
#[tokio::test]
async fn test_subscribe_book_deltas_full_depth_rejects_non_overlapping_first_diff() {
    let addr = start_data_test_server_with_config(DataTestServerConfig {
        depth_diff_first_update_id: 103,
        depth_diff_last_update_id: 103,
        depth_diff_repetitions: 4,
        ..Default::default()
    })
    .await;
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

    let instrument_id = InstrumentId::from("BTCUSDT.BINANCE");
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

    assert!(recv_data(&mut rx, Duration::from_secs(1)).await.is_none());
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

    let instrument_id = InstrumentId::from("BTCUSDT.BINANCE");

    // Subscribe first
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

    // Wait for data to arrive
    wait_until_async(
        || {
            let found = rx.try_recv().is_ok_and(|e| matches!(e, DataEvent::Data(_)));
            async move { found }
        },
        Duration::from_secs(5),
    )
    .await;

    while rx.try_recv().is_ok() {}

    // Unsubscribe (should not error)
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

    let instrument_id = InstrumentId::from("BTCUSDT.BINANCE");

    // Subscribe first
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

    // Wait for data to arrive
    wait_until_async(
        || {
            let found = rx.try_recv().is_ok_and(|e| matches!(e, DataEvent::Data(_)));
            async move { found }
        },
        Duration::from_secs(5),
    )
    .await;

    while rx.try_recv().is_ok() {}

    // Unsubscribe (should not error)
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

    let instrument_id = InstrumentId::from("BTCUSDT.BINANCE");

    // Subscribe to both trades and quotes for the same instrument
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

    // Should receive data events for both subscriptions
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
