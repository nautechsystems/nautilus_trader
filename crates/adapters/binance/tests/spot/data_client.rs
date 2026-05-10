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

use std::{collections::HashMap, net::SocketAddr, time::Duration};

use axum::{
    Router,
    body::Body,
    extract::ws::{Message, WebSocket},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
    routing::get,
};
use nautilus_binance::{
    config::BinanceDataClientConfig,
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
            subscribe::{SubscribeBookDeltas, SubscribeQuotes, SubscribeTrades},
            unsubscribe::{UnsubscribeQuotes, UnsubscribeTrades},
        },
    },
    testing::wait_until_async,
};
use nautilus_core::UnixNanos;
use nautilus_model::{
    enums::BookType,
    identifiers::{ClientId, InstrumentId, Venue},
};
use nautilus_network::http::HttpClient;
use rstest::rstest;
use serde_json::json;

const PING_TEMPLATE_ID: u16 = 101;
const EXCHANGE_INFO_TEMPLATE_ID: u16 = 103;
const SYMBOL_BLOCK_LENGTH: u16 = 19;
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
                let _ = socket.send(Message::Text(resp.to_string().into())).await;

                if let Some(params) = parsed.get("params").and_then(|p| p.as_array()) {
                    for param in params {
                        if let Some(stream) = param.as_str() {
                            if stream.contains("@trade") {
                                let symbol =
                                    stream.split('@').next().unwrap_or("BTCUSDT").to_uppercase();
                                let data = build_sbe_trades_stream_event(&symbol);
                                tokio::time::sleep(Duration::from_millis(50)).await;
                                let _ = socket.send(Message::Binary(data.into())).await;
                            } else if stream.contains("@bestBidAsk") {
                                let symbol =
                                    stream.split('@').next().unwrap_or("BTCUSDT").to_uppercase();
                                let data = build_sbe_best_bid_ask_stream_event(&symbol);
                                tokio::time::sleep(Duration::from_millis(50)).await;
                                let _ = socket.send(Message::Binary(data.into())).await;
                            } else if stream.contains("@depth") {
                                let symbol =
                                    stream.split('@').next().unwrap_or("BTCUSDT").to_uppercase();
                                let data = build_sbe_depth_snapshot_stream_event(&symbol);
                                tokio::time::sleep(Duration::from_millis(50)).await;
                                let _ = socket.send(Message::Binary(data.into())).await;
                            }
                        }
                    }
                }
            } else if method == Some("UNSUBSCRIBE") {
                let resp = json!({"result": null, "id": id});
                let _ = socket.send(Message::Text(resp.to_string().into())).await;
            }
        }
    }
}

fn create_data_test_router() -> Router {
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
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    set_data_event_sender(tx);

    let config = BinanceDataClientConfig {
        base_url_http: Some(base_url_http),
        base_url_ws: Some(base_url_ws),
        ..Default::default()
    };

    let client = BinanceSpotDataClient::new(ClientId::from("BINANCE"), config).unwrap();

    (client, rx)
}

#[rstest]
#[tokio::test]
async fn test_client_creation() {
    let addr = start_data_test_server().await;
    let base_url_http = format!("http://{addr}");
    let base_url_ws = format!("ws://{addr}/ws");

    let (client, _rx) = create_test_data_client(base_url_http, base_url_ws);

    assert_eq!(client.client_id(), ClientId::from("BINANCE"));
    assert_eq!(client.venue(), Some(Venue::from("BINANCE")));
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
        Some(ClientId::from("BINANCE")),
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
        Some(ClientId::from("BINANCE")),
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

    let instrument_id = InstrumentId::from("BTCUSDT.BINANCE");
    let cmd = SubscribeBookDeltas::new(
        instrument_id,
        BookType::L2_MBP,
        Some(ClientId::from("BINANCE")),
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
        Some(ClientId::from("BINANCE")),
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
        Some(ClientId::from("BINANCE")),
        None,
        nautilus_core::UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );
    let result = client.unsubscribe_trades(&unsub_cmd);
    assert!(result.is_ok());
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
        Some(ClientId::from("BINANCE")),
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
        Some(ClientId::from("BINANCE")),
        None,
        nautilus_core::UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );
    let result = client.unsubscribe_quotes(&unsub_cmd);
    assert!(result.is_ok());
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
        Some(ClientId::from("BINANCE")),
        None,
        nautilus_core::UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );
    let quotes_cmd = SubscribeQuotes::new(
        instrument_id,
        Some(ClientId::from("BINANCE")),
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
