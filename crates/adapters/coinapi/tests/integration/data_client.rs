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

#![cfg(not(all(feature = "simulation", madsim)))]

use std::{
    collections::{BTreeMap, HashMap},
    future::Future,
    num::NonZeroUsize,
    time::Duration,
};

use futures_util::{SinkExt, StreamExt};
use jiff::{SignedDuration, Timestamp};
use nautilus_coinapi::{
    config::{CoinApiDataClientConfig, CoinApiInstrumentSelection},
    data::CoinApiDataClient,
    http::query::SymbolsQuery,
};
use nautilus_common::{
    clients::DataClient,
    live::runner::replace_data_event_sender,
    messages::{
        DataEvent, DataResponse,
        data::{
            RequestBars, RequestBookSnapshot, RequestInstrument, RequestInstruments, RequestQuotes,
            RequestTrades, SubscribeBars, SubscribeBookDeltas, SubscribeBookDepth, SubscribeQuotes,
            SubscribeTrades, UnsubscribeBookDeltas, UnsubscribeBookDepth, UnsubscribeQuotes,
            UnsubscribeTrades,
        },
    },
};
use nautilus_core::{Params, UUID4, UnixNanos, string::secret::SecretString};
use nautilus_live::book::conformance::BookStreamChecker;
use nautilus_model::{
    data::{Bar, BarType, BookOrder, Data, OrderBookDeltas, OrderBookDepth, QuoteTick, TradeTick},
    enums::{AggressorSide, BookAction, BookType, OrderSide, RecordFlag},
    identifiers::{ClientId, InstrumentId, TradeId, Venue},
    instruments::Instrument,
    types::{Price, Quantity},
};
use nautilus_network::websocket::TransportBackend;
use rstest::rstest;
use rust_decimal::Decimal;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};
use tokio_tungstenite::{WebSocketStream, accept_async, tungstenite::Message};

type Socket = WebSocketStream<tokio::net::TcpStream>;

const BTC_USD: &str = "BTC-USD.COINBASE";
const SYMBOLS: &str = include_str!("../../test_data/symbols.json");
const QUOTE: &str = include_str!("../../test_data/ws_quote.json");
const TRADE: &str = include_str!("../../test_data/ws_trade.json");
const OHLCV: &str = include_str!("../../test_data/ws_ohlcv.json");
const HEARTBEAT: &str = include_str!("../../test_data/ws_heartbeat.json");
const SYMBOLS_TARGET: &str = "/v1/symbols/COINBASE/active?filter_symbol_id=COINBASE_SPOT_BTC_USD";
const HELLO_TRADE: &str = r#"{"type":"hello","heartbeat":true,"subscribe_data_type":["trade"],"subscribe_filter_symbol_id":["COINBASE_SPOT_BTC_USD$"]}"#;
const HELLO_QUOTE_TRADE: &str = r#"{"type":"hello","heartbeat":true,"subscribe_data_type":["quote","trade"],"subscribe_filter_symbol_id":["COINBASE_SPOT_BTC_USD$"]}"#;
const HELLO_ALL: &str = r#"{"type":"hello","heartbeat":true,"subscribe_data_type":["ohlcv","quote","trade"],"subscribe_filter_symbol_id":["COINBASE_SPOT_BTC_USD$"],"subscribe_filter_period_id":["1SEC"]}"#;
const HELLO_OHLCV_QUOTE: &str = r#"{"type":"hello","heartbeat":true,"subscribe_data_type":["ohlcv","quote"],"subscribe_filter_symbol_id":["COINBASE_SPOT_BTC_USD$"],"subscribe_filter_period_id":["1SEC"]}"#;
const HELLO_OHLCV_TRADE: &str = r#"{"type":"hello","heartbeat":true,"subscribe_data_type":["ohlcv","trade"],"subscribe_filter_symbol_id":["COINBASE_SPOT_BTC_USD$"],"subscribe_filter_period_id":["1SEC"]}"#;
const HELLO_BOOK_TRADE: &str = r#"{"type":"hello","heartbeat":true,"subscribe_data_type":["book","trade"],"subscribe_filter_symbol_id":["COINBASE_SPOT_BTC_USD$"]}"#;
const HELLO_BOOK_L3: &str = r#"{"type":"hello","heartbeat":true,"subscribe_data_type":["book_l3"],"subscribe_filter_symbol_id":["COINBASE_SPOT_BTC_USD$"]}"#;
const HELLO_BOOK20: &str = r#"{"type":"hello","heartbeat":true,"subscribe_data_type":["book20"],"subscribe_filter_symbol_id":["COINBASE_SPOT_BTC_USD$"]}"#;

fn client_id() -> ClientId {
    ClientId::new("COINAPI")
}

fn instrument_id() -> InstrumentId {
    InstrumentId::from(BTC_USD)
}

fn bar_type() -> BarType {
    BarType::from("BTC-USD.COINBASE-1-SECOND-LAST-EXTERNAL")
}

fn config(http: &str, ws: &str, instrument_ids: &[&str]) -> CoinApiDataClientConfig {
    CoinApiDataClientConfig::builder()
        .api_key(SecretString::from("test-api-key"))
        .base_url_http(format!("http://{http}/v1/"))
        .base_url_ws(format!("ws://{ws}/v1/"))
        .instruments(vec![CoinApiInstrumentSelection {
            exchange_id: "COINBASE".into(),
            query: SymbolsQuery {
                filter_symbol_id: Some("COINBASE_SPOT_BTC_USD".into()),
                ..SymbolsQuery::default()
            },
            instrument_ids: instrument_ids
                .iter()
                .map(|id| InstrumentId::from(*id))
                .collect(),
        }])
        .max_retries(0)
        .transport_backend(TransportBackend::Tungstenite)
        .build()
}

async fn listener() -> (TcpListener, String) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap().to_string();
    (listener, address)
}

async fn bounded<F: Future>(future: F) -> F::Output {
    tokio::time::timeout(Duration::from_secs(20), future)
        .await
        .unwrap()
}

// Serves each body once, in order, and returns the request targets
async fn http_peer(bodies: Vec<String>) -> (tokio::task::JoinHandle<Vec<String>>, String) {
    let (listener, address) = listener().await;

    let task = tokio::spawn(async move {
        let mut targets = Vec::new();

        for body in bodies {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            while !request.ends_with(b"\r\n\r\n") {
                request.push(stream.read_u8().await.unwrap());
            }

            let request = String::from_utf8(request).unwrap();
            let target = request.split(' ').nth(1).unwrap().to_owned();
            targets.push(target);

            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            stream.write_all(response.as_bytes()).await.unwrap();
            stream.flush().await.unwrap();
        }

        targets
    });

    (task, address)
}

async fn accept(listener: &TcpListener) -> Socket {
    let (stream, _) = listener.accept().await.unwrap();
    accept_async(stream).await.unwrap()
}

async fn next_text(socket: &mut Socket) -> String {
    loop {
        match socket.next().await.unwrap().unwrap() {
            Message::Text(text) => return text.to_string(),
            Message::Ping(_) | Message::Pong(_) => {}
            other => panic!("expected text frame, was {other:?}"),
        }
    }
}

async fn send(socket: &mut Socket, text: &str) {
    socket.send(Message::text(text)).await.unwrap();
}

async fn next_event(rx: &mut tokio::sync::mpsc::UnboundedReceiver<DataEvent>) -> DataEvent {
    tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("data event within 5 seconds")
        .unwrap()
}

async fn next_data(rx: &mut tokio::sync::mpsc::UnboundedReceiver<DataEvent>) -> Data {
    match next_event(rx).await {
        DataEvent::Data(data) => data,
        other => panic!("expected data, was {other:?}"),
    }
}

fn data_channel() -> tokio::sync::mpsc::UnboundedReceiver<DataEvent> {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    replace_data_event_sender(tx);
    rx
}

fn expected_quote() -> Data {
    Data::Quote(QuoteTick::new(
        instrument_id(),
        Price::from("760.00"),
        Price::from("770.00"),
        Quantity::from("124.00000000"),
        Quantity::from("3252.00000000"),
        UnixNanos::from(1_380_408_050_000_000_000),
        UnixNanos::from(1_489_876_941_376_334_200),
    ))
}

fn expected_trade(price: &str) -> Data {
    Data::Trade(TradeTick::new(
        instrument_id(),
        Price::from(price),
        Quantity::from("0.00000004"),
        AggressorSide::Sell,
        TradeId::new("c2519dfb-79a6-40e9-a1c8-a95d94209399"),
        UnixNanos::from(1_790_299_746_065_618_000),
        UnixNanos::from(1_790_299_746_143_827_200),
    ))
}

fn ohlcv(start_second: u32, close: &str) -> String {
    let end_second = start_second + 1;
    OHLCV
        .replace("01:30:02.0000000Z", "PERIOD_END")
        .replace(
            "01:30:01.0000000Z",
            &format!("01:30:{start_second:02}.0000000Z"),
        )
        .replace("PERIOD_END", &format!("01:30:{end_second:02}.0000000Z"))
        .replace(
            r#""price_close":84523.69"#,
            &format!(r#""price_close":{close}"#),
        )
}

// Compares every field except ts_init, which comes from the realtime clock
fn assert_bar(data: &Data, start_second: u32, close: &str) {
    let Data::Bar(bar) = data else {
        panic!("expected bar, was {data:?}");
    };

    let ts_event =
        UnixNanos::from(1_790_299_800_000_000_000 + u64::from(start_second + 1) * 1_000_000_000);
    let close = Price::from(close);

    assert_eq!(
        *bar,
        Bar::new(
            bar_type(),
            Price::from_decimal_dp("84523.69".parse().unwrap(), close.precision).unwrap(),
            Price::from_decimal_dp("84523.69".parse().unwrap(), close.precision).unwrap(),
            Price::from_decimal_dp("84523.68".parse().unwrap(), close.precision).unwrap(),
            close,
            Quantity::from("0.00468951"),
            ts_event,
            bar.ts_init,
        )
    );
    assert!(bar.ts_init >= ts_event);
}

fn subscribe_trades(client: &mut CoinApiDataClient) {
    client
        .subscribe_trades(SubscribeTrades::new(
            instrument_id(),
            Some(client_id()),
            None,
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ))
        .unwrap();
}

fn subscribe_bars(client: &mut CoinApiDataClient) {
    client
        .subscribe_bars(SubscribeBars::new(
            bar_type(),
            Some(client_id()),
            None,
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ))
        .unwrap();
}

async fn connected(
    http: &str,
    ws: &str,
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<DataEvent>,
) -> CoinApiDataClient {
    let mut client = CoinApiDataClient::new(client_id(), config(http, ws, &[BTC_USD])).unwrap();
    client.connect().await.unwrap();

    let DataEvent::Instrument(instrument) = next_event(rx).await else {
        panic!("expected instrument");
    };

    assert_eq!(instrument.id(), instrument_id());
    assert!(client.is_connected());
    client
}

#[tokio::test]
async fn connect_loads_selection_and_streams_domain_events() {
    bounded(async {
        let mut rx = data_channel();
        let (http, http_address) = http_peer(vec![SYMBOLS.into()]).await;
        let (ws_listener, ws_address) = listener().await;

        let mut client =
            CoinApiDataClient::new(client_id(), config(&http_address, &ws_address, &[BTC_USD]))
                .unwrap();
        client.connect().await.unwrap();

        // The response also lists COINBASE_SPOT_BTC_USDT, which is not selected
        let DataEvent::Instrument(instrument) = next_event(&mut rx).await else {
            panic!("expected instrument");
        };

        assert_eq!(http.await.unwrap(), vec![SYMBOLS_TARGET]);
        assert_eq!(instrument.id(), instrument_id());
        assert_eq!(instrument.price_increment(), Price::from("0.01"));
        assert_eq!(instrument.size_increment(), Quantity::from("0.00000001"));

        subscribe_trades(&mut client);
        let mut socket = accept(&ws_listener).await;
        assert_eq!(next_text(&mut socket).await, HELLO_TRADE);

        client
            .subscribe_quotes(SubscribeQuotes::new(
                instrument_id(),
                Some(client_id()),
                None,
                UUID4::new(),
                UnixNanos::default(),
                None,
                None,
            ))
            .unwrap();
        assert_eq!(next_text(&mut socket).await, HELLO_QUOTE_TRADE);

        subscribe_bars(&mut client);
        assert_eq!(next_text(&mut socket).await, HELLO_ALL);

        send(&mut socket, "{not json").await;
        send(&mut socket, &TRADE.replace("84520.02", "84520.021")).await;
        send(&mut socket, TRADE).await;
        send(&mut socket, &QUOTE.replace("BITSTAMP", "COINBASE")).await;
        send(&mut socket, &ohlcv(1, "84523.68")).await;
        send(
            &mut socket,
            &ohlcv(1, "84523.69").replace("0.00468951", "0.004689510000000001"),
        )
        .await;
        send(&mut socket, &ohlcv(2, "84523.68")).await;

        assert_eq!(next_data(&mut rx).await, expected_trade("84520.02"));
        assert_eq!(next_data(&mut rx).await, expected_quote());
        assert_bar(&next_data(&mut rx).await, 1, "84523.69");

        client
            .unsubscribe_trades(&UnsubscribeTrades::new(
                instrument_id(),
                Some(client_id()),
                None,
                UUID4::new(),
                UnixNanos::default(),
                None,
                None,
            ))
            .unwrap();
        assert_eq!(next_text(&mut socket).await, HELLO_OHLCV_QUOTE);

        send(&mut socket, TRADE).await;
        send(&mut socket, &QUOTE.replace("BITSTAMP", "COINBASE")).await;
        assert_eq!(next_data(&mut rx).await, expected_quote());

        client.disconnect().await.unwrap();
        assert!(client.is_disconnected());
    })
    .await;
}

// CoinAPI lists BTC-USD at two decimals, so only the override admits a three-decimal price
#[tokio::test]
async fn price_precision_override_loads_finer_instrument_and_admits_its_prices() {
    bounded(async {
        let mut rx = data_channel();
        let (_http, http_address) = http_peer(vec![SYMBOLS.into()]).await;
        let (ws_listener, ws_address) = listener().await;

        let config = CoinApiDataClientConfig {
            price_precisions: HashMap::from([(instrument_id(), 3)]),
            ..config(&http_address, &ws_address, &[BTC_USD])
        };

        let mut client = CoinApiDataClient::new(client_id(), config).unwrap();
        client.connect().await.unwrap();

        let DataEvent::Instrument(instrument) = next_event(&mut rx).await else {
            panic!("expected instrument");
        };

        subscribe_trades(&mut client);
        let mut socket = accept(&ws_listener).await;
        assert_eq!(next_text(&mut socket).await, HELLO_TRADE);
        send(&mut socket, &TRADE.replace("84520.02", "84520.021")).await;

        assert_eq!(instrument.price_increment(), Price::from("0.001"));
        assert_eq!(next_data(&mut rx).await, expected_trade("84520.021"));

        subscribe_bars(&mut client);
        assert_eq!(next_text(&mut socket).await, HELLO_OHLCV_TRADE);
        send(&mut socket, &ohlcv(1, "84523.685")).await;
        send(&mut socket, &ohlcv(2, "84523.686")).await;

        let data = next_data(&mut rx).await;
        assert_bar(&data, 1, "84523.685");

        let Data::Bar(bar) = data else {
            panic!("expected bar");
        };

        assert_eq!(
            [
                bar.open.precision,
                bar.high.precision,
                bar.low.precision,
                bar.close.precision,
            ],
            [3; 4]
        );

        client.disconnect().await.unwrap();
    })
    .await;
}

#[tokio::test]
async fn reconnect_replays_scope_and_discards_retained_period() {
    bounded(async {
        let mut rx = data_channel();
        let (_http, http_address) = http_peer(vec![SYMBOLS.into()]).await;
        let (ws_listener, ws_address) = listener().await;
        let mut client = connected(&http_address, &ws_address, &mut rx).await;

        subscribe_trades(&mut client);
        let mut socket = accept(&ws_listener).await;
        assert_eq!(next_text(&mut socket).await, HELLO_TRADE);
        subscribe_bars(&mut client);
        assert_eq!(next_text(&mut socket).await, HELLO_OHLCV_TRADE);

        send(&mut socket, &ohlcv(1, "84523.69")).await;
        socket.close(None).await.unwrap();
        drop(socket);

        let mut socket = accept(&ws_listener).await;
        assert_eq!(next_text(&mut socket).await, HELLO_OHLCV_TRADE);

        send(&mut socket, &ohlcv(2, "84523.68")).await;
        send(&mut socket, &ohlcv(3, "84523.69")).await;
        send(&mut socket, TRADE).await;

        assert_bar(&next_data(&mut rx).await, 2, "84523.68");
        assert_eq!(next_data(&mut rx).await, expected_trade("84520.02"));

        client.disconnect().await.unwrap();
    })
    .await;
}

fn book(
    data_type: &str,
    sequence: u64,
    is_snapshot: Option<bool>,
    bids: &[(&str, &str)],
    asks: &[(&str, &str)],
) -> String {
    let levels = |levels: &[(&str, &str)]| {
        levels
            .iter()
            .map(|(price, size)| format!(r#"{{"price":{price},"size":{size}}}"#))
            .collect::<Vec<_>>()
            .join(",")
    };

    let snapshot = is_snapshot.map_or(String::new(), |value| format!(r#""is_snapshot":{value},"#));

    format!(
        r#"{{"type":"{data_type}","symbol_id":"COINBASE_SPOT_BTC_USD","sequence":{sequence},"time_exchange":"2026-09-27T10:52:27.4861510Z","time_coinapi":"2026-09-27T10:52:27.5708553Z",{snapshot}"asks":[{}],"bids":[{}]}}"#,
        levels(asks),
        levels(bids),
    )
}

// Entries are (side, id, price, size, update type)
fn book_l3(
    sequence: u64,
    is_snapshot: bool,
    entries: &[(OrderSide, &str, &str, &str, &str)],
) -> String {
    let side = |side: OrderSide| {
        entries
            .iter()
            .filter(|entry| entry.0 == side)
            .map(|(_, id, price, size, update_type)| {
                let update_type = if update_type.is_empty() {
                    String::new()
                } else {
                    format!(r#","update_type":"{update_type}""#)
                };

                format!(r#"{{"id":"{id}","price":{price},"size":{size}{update_type}}}"#)
            })
            .collect::<Vec<_>>()
            .join(",")
    };

    format!(
        r#"{{"type":"book_l3","symbol_id":"COINBASE_SPOT_BTC_USD","sequence":{sequence},"time_exchange":"2026-09-27T10:52:27.4861510Z","time_coinapi":"2026-09-27T10:52:27.5708553Z","is_snapshot":{is_snapshot},"asks":[{}],"bids":[{}]}}"#,
        side(OrderSide::Sell),
        side(OrderSide::Buy),
    )
}

async fn next_deltas(rx: &mut tokio::sync::mpsc::UnboundedReceiver<DataEvent>) -> OrderBookDeltas {
    match next_data(rx).await {
        Data::BookDeltas(deltas) => *deltas,
        other => panic!("expected book deltas, was {other:?}"),
    }
}

// Applies the next batch to the contract checker, returning whether it is a snapshot and its
// sequence
async fn check_next(
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<DataEvent>,
    checker: &mut BookStreamChecker,
) -> (bool, u64) {
    let deltas = next_deltas(rx).await;
    checker.apply(&deltas).unwrap();
    let is_snapshot = deltas.deltas[0].action == BookAction::Clear;
    (is_snapshot, deltas.deltas[0].sequence)
}

// The client opens a book's snapshot gate just after its write returns, before a fast local peer
// could answer
async fn settle() {
    tokio::time::sleep(Duration::from_millis(100)).await;
}

fn levels(pairs: &[(&str, &str)]) -> BTreeMap<Decimal, Decimal> {
    pairs
        .iter()
        .map(|(price, size)| {
            (
                Decimal::from_str_exact(price).unwrap(),
                Decimal::from_str_exact(size).unwrap(),
            )
        })
        .collect()
}

fn subscribe_book_deltas(
    client: &mut CoinApiDataClient,
    book_type: BookType,
) -> anyhow::Result<()> {
    client.subscribe_book_deltas(SubscribeBookDeltas::new(
        instrument_id(),
        book_type,
        Some(client_id()),
        None,
        UUID4::new(),
        UnixNanos::default(),
        None,
        false,
        None,
        None,
    ))
}

fn subscribe_book_depth(
    client: &mut CoinApiDataClient,
    book_type: BookType,
    depth: Option<usize>,
) -> anyhow::Result<()> {
    client.subscribe_book_depth(SubscribeBookDepth::new(
        instrument_id(),
        book_type,
        Some(client_id()),
        None,
        UUID4::new(),
        UnixNanos::default(),
        depth.and_then(NonZeroUsize::new),
        false,
        None,
        None,
    ))
}

#[rstest]
#[case(None, None, BookType::L2_MBP, "book")]
#[case(None, None, BookType::L3_MBO, "book_l3")]
#[case(Some(10), Some(10), BookType::L2_MBP, "book20")]
#[case(Some(10), None, BookType::L2_MBP, "book")]
#[tokio::test]
async fn terminal_error_allows_book_subscription(
    #[case] original_depth: Option<usize>,
    #[case] requested_depth: Option<usize>,
    #[case] requested_type: BookType,
    #[case] requested_channel: &str,
) {
    bounded(async {
        let mut rx = data_channel();
        let (_http, http_address) = http_peer(vec![SYMBOLS.into()]).await;
        let (ws_listener, ws_address) = listener().await;
        let mut client = connected(&http_address, &ws_address, &mut rx).await;
        match original_depth {
            Some(depth) => subscribe_book_depth(&mut client, BookType::L2_MBP, Some(depth)),
            None => subscribe_book_deltas(&mut client, BookType::L2_MBP),
        }
        .unwrap();

        let mut socket = accept(&ws_listener).await;

        let original_channel = if original_depth.is_some() {
            "book20"
        } else {
            "book"
        };

        assert_eq!(
            next_text(&mut socket).await,
            HELLO_BOOK20.replace("book20", original_channel)
        );
        send(
            &mut socket,
            include_str!("../../test_data/ws_error_credential.json"),
        )
        .await;

        loop {
            if matches!(socket.next().await.unwrap().unwrap(), Message::Close(_)) {
                break;
            }
        }

        drop(socket);

        subscribe_trades(&mut client);
        let mut socket = accept(&ws_listener).await;
        assert_eq!(next_text(&mut socket).await, HELLO_TRADE);
        send(&mut socket, TRADE).await;
        assert_eq!(next_data(&mut rx).await, expected_trade("84520.02"));

        match requested_depth {
            Some(depth) => subscribe_book_depth(&mut client, requested_type, Some(depth)),
            None => subscribe_book_deltas(&mut client, requested_type),
        }
        .unwrap();

        assert_eq!(
            next_text(&mut socket).await,
            HELLO_BOOK_TRADE.replace("book", requested_channel)
        );

        match requested_depth {
            Some(depth) => subscribe_book_depth(&mut client, requested_type, Some(depth)),
            None => subscribe_book_deltas(&mut client, requested_type),
        }
        .unwrap();

        client.disconnect().await.unwrap();
    })
    .await;
}

#[tokio::test]
async fn terminal_error_preserves_queued_book_depth() {
    bounded(async {
        let mut rx = data_channel();
        let (_http, http_address) = http_peer(vec![SYMBOLS.into()]).await;
        let (ws_listener, ws_address) = listener().await;
        let mut client = connected(&http_address, &ws_address, &mut rx).await;
        subscribe_trades(&mut client);
        let mut socket = accept(&ws_listener).await;
        assert_eq!(next_text(&mut socket).await, HELLO_TRADE);
        send(&mut socket, TRADE).await;
        assert_eq!(next_data(&mut rx).await, expected_trade("84520.02"));
        send(
            &mut socket,
            include_str!("../../test_data/ws_error_credential.json"),
        )
        .await;
        // Queue within the transport's graceful-disconnect interval, before retirement completes
        tokio::time::sleep(Duration::from_millis(50)).await;
        subscribe_book_depth(&mut client, BookType::L2_MBP, Some(10)).unwrap();

        loop {
            if matches!(socket.next().await.unwrap().unwrap(), Message::Close(_)) {
                break;
            }
        }

        drop(socket);
        let mut replacement = accept(&ws_listener).await;
        assert_eq!(next_text(&mut replacement).await, HELLO_BOOK20);
        send(
            &mut replacement,
            &book(
                "book20",
                7,
                None,
                &[("84000.00", "2")],
                &[("84000.01", "1")],
            ),
        )
        .await;

        let Data::BookDepth(depth) = next_data(&mut rx).await else {
            panic!("expected replacement depth");
        };

        assert_eq!(
            *depth,
            OrderBookDepth::new(
                instrument_id(),
                vec![BookOrder::new(
                    OrderSide::Buy,
                    Price::from("84000.00"),
                    Quantity::from("2.00000000"),
                    0
                )],
                vec![BookOrder::new(
                    OrderSide::Sell,
                    Price::from("84000.01"),
                    Quantity::from("1.00000000"),
                    0
                )],
                vec![0],
                vec![0],
                RecordFlag::F_SNAPSHOT as u8,
                7,
                nanos("2026-09-27T10:52:27.4861510Z"),
                nanos("2026-09-27T10:52:27.5708553Z"),
            )
        );
        client
            .unsubscribe_book_depth(&UnsubscribeBookDepth::new(
                instrument_id(),
                Some(client_id()),
                None,
                UUID4::new(),
                UnixNanos::default(),
                None,
                None,
            ))
            .unwrap();

        loop {
            if matches!(
                replacement.next().await.unwrap().unwrap(),
                Message::Close(_)
            ) {
                break;
            }
        }

        client.disconnect().await.unwrap();
    })
    .await;
}

// A gap restarts the pair by leaving it out of the scope; its trades keep streaming. A reconnect
// restarts every pair, and each book waits for the snapshot its replayed pair brings. Emitted
// sequences keep rising although CoinAPI restarts them at one.
#[tokio::test]
async fn book_deltas_recover_from_gap_and_reconnect() {
    bounded(async {
        let mut rx = data_channel();
        let (_http, http_address) = http_peer(vec![SYMBOLS.into()]).await;
        let (ws_listener, ws_address) = listener().await;
        let mut client = connected(&http_address, &ws_address, &mut rx).await;
        let mut checker = BookStreamChecker::new(BookType::L2_MBP, true);
        checker.open(instrument_id());

        subscribe_trades(&mut client);
        let mut socket = accept(&ws_listener).await;
        assert_eq!(next_text(&mut socket).await, HELLO_TRADE);
        subscribe_book_deltas(&mut client, BookType::L2_MBP).unwrap();
        assert_eq!(next_text(&mut socket).await, HELLO_BOOK_TRADE);
        settle().await;

        let bid = [("84000.00", "1"), ("83999.99", "2")];
        let ask = [("84000.01", "3")];
        send(&mut socket, &book("book", 1, Some(true), &bid, &ask)).await;
        send(
            &mut socket,
            &book(
                "book",
                2,
                Some(false),
                &[("84000.00", "0")],
                &[("84000.02", "4")],
            ),
        )
        .await;
        let snapshot = check_next(&mut rx, &mut checker).await;
        let update = check_next(&mut rx, &mut checker).await;

        send(
            &mut socket,
            &book("book", 4, Some(false), &[("83999.99", "5")], &[]),
        )
        .await;
        assert_eq!(next_text(&mut socket).await, HELLO_TRADE);
        assert_eq!(next_text(&mut socket).await, HELLO_BOOK_TRADE);
        settle().await;

        // The old stream's frames still in flight end before the restarted pair's snapshot
        send(
            &mut socket,
            &book("book", 5, Some(false), &[("83999.99", "6")], &[]),
        )
        .await;
        send(&mut socket, TRADE).await;
        send(
            &mut socket,
            &book("book", 1, Some(true), &[("83999.98", "7")], &ask),
        )
        .await;
        send(
            &mut socket,
            &book("book", 2, Some(false), &[], &[("84000.01", "8")]),
        )
        .await;
        let trade = next_data(&mut rx).await;
        let recovered = check_next(&mut rx, &mut checker).await;
        let linked = check_next(&mut rx, &mut checker).await;

        socket.close(None).await.unwrap();
        drop(socket);
        let mut socket = accept(&ws_listener).await;
        assert_eq!(next_text(&mut socket).await, HELLO_BOOK_TRADE);
        settle().await;

        send(
            &mut socket,
            &book("book", 3, Some(false), &[("83999.98", "9")], &[]),
        )
        .await;
        send(
            &mut socket,
            &book("book", 1, Some(true), &bid, &[("84000.03", "1")]),
        )
        .await;
        send(
            &mut socket,
            &book("book", 2, Some(false), &[("84000.00", "2")], &[]),
        )
        .await;
        let replayed = check_next(&mut rx, &mut checker).await;
        let resumed = check_next(&mut rx, &mut checker).await;

        assert_eq!(snapshot, (true, 1));
        assert_eq!(update, (false, 2));
        assert_eq!(trade, expected_trade("84520.02"));
        assert_eq!(recovered, (true, 3));
        assert_eq!(linked, (false, 4));
        assert_eq!(replayed, (true, 5));
        assert_eq!(resumed, (false, 6));
        assert_eq!(
            checker.verify(
                instrument_id(),
                10,
                &levels(&[("84000.00", "2"), ("83999.99", "2")]),
                &levels(&[("84000.03", "1")]),
            ),
            Ok(())
        );
        assert_eq!(checker.coverage(), (3, 1));

        client.disconnect().await.unwrap();
    })
    .await;
}

// With the book as the whole scope, recovery lists only its trades for the removal scope and keeps
// the connection, which a one-connection tier could not replace
#[tokio::test]
async fn book_l3_deltas_recover_on_same_connection_when_pair_is_whole_scope() {
    bounded(async {
        let mut rx = data_channel();
        let (_http, http_address) = http_peer(vec![SYMBOLS.into()]).await;
        let (ws_listener, ws_address) = listener().await;
        let mut client = connected(&http_address, &ws_address, &mut rx).await;
        let mut checker = BookStreamChecker::new(BookType::L3_MBO, true);
        checker.open(instrument_id());

        subscribe_book_deltas(&mut client, BookType::L3_MBO).unwrap();
        let mut socket = accept(&ws_listener).await;
        assert_eq!(next_text(&mut socket).await, HELLO_BOOK_L3);
        settle().await;

        send(
            &mut socket,
            &book_l3(
                1,
                true,
                &[
                    (OrderSide::Buy, "b1", "84000.00", "0.5", ""),
                    (OrderSide::Buy, "b2", "84000.00", "0.25", ""),
                    (OrderSide::Sell, "a1", "84000.01", "1", ""),
                ],
            ),
        )
        .await;
        send(
            &mut socket,
            &book_l3(
                2,
                false,
                &[
                    (OrderSide::Buy, "b1", "84000.00", "0.2", "MATCH"),
                    (OrderSide::Buy, "b2", "84000.00", "0.25", "MATCH"),
                    (OrderSide::Buy, "b2", "84000.00", "0.25", "DELETE"),
                ],
            ),
        )
        .await;
        let snapshot = next_deltas(&mut rx).await;
        checker.apply(&snapshot).unwrap();
        let update = next_deltas(&mut rx).await;
        checker.apply(&update).unwrap();

        send(
            &mut socket,
            &book_l3(
                4,
                false,
                &[(OrderSide::Sell, "a1", "84000.01", "1", "DELETE")],
            ),
        )
        .await;
        assert_eq!(next_text(&mut socket).await, HELLO_TRADE);
        assert_eq!(next_text(&mut socket).await, HELLO_BOOK_L3);
        settle().await;

        // The removal scope's trades are not subscribed, so the client drops them
        send(&mut socket, TRADE).await;

        send(
            &mut socket,
            &book_l3(1, true, &[(OrderSide::Sell, "a2", "84000.02", "3", "")]),
        )
        .await;
        let recovered = check_next(&mut rx, &mut checker).await;

        let orders = |deltas: &OrderBookDeltas| {
            deltas
                .deltas
                .iter()
                .map(|delta| {
                    (
                        delta.action,
                        delta.order.order_id,
                        delta.order.size.to_string(),
                    )
                })
                .collect::<Vec<_>>()
        };

        assert_eq!(
            orders(&snapshot),
            vec![
                (BookAction::Clear, 0, "0".to_string()),
                (BookAction::Add, 1, "0.50000000".to_string()),
                (BookAction::Add, 2, "0.25000000".to_string()),
                (BookAction::Add, 3, "1.00000000".to_string()),
            ]
        );
        assert_eq!(
            orders(&update),
            vec![
                (BookAction::Update, 1, "0.30000000".to_string()),
                (BookAction::Delete, 2, "0.25000000".to_string()),
            ]
        );
        assert_eq!(recovered, (true, 3));
        assert_eq!(
            checker.verify(
                instrument_id(),
                10,
                &levels(&[]),
                &levels(&[("84000.02", "3")])
            ),
            Ok(())
        );

        client.disconnect().await.unwrap();
    })
    .await;
}

#[tokio::test]
async fn book_depth_streams_cheapest_channel_and_keeps_one_book_channel() {
    bounded(async {
        let mut rx = data_channel();
        let (_http, http_address) = http_peer(vec![SYMBOLS.into()]).await;
        let (ws_listener, ws_address) = listener().await;
        let mut client = connected(&http_address, &ws_address, &mut rx).await;

        let too_deep = subscribe_book_depth(&mut client, BookType::L2_MBP, Some(51)).unwrap_err();
        let l3 = subscribe_book_depth(&mut client, BookType::L3_MBO, None).unwrap_err();
        subscribe_book_depth(&mut client, BookType::L2_MBP, None).unwrap();
        let mut socket = accept(&ws_listener).await;
        assert_eq!(next_text(&mut socket).await, HELLO_BOOK20);
        let conflicting = subscribe_book_deltas(&mut client, BookType::L2_MBP).unwrap_err();

        // Only the depth subscription is active, so a deltas unsubscribe must keep it streaming
        client
            .unsubscribe_book_deltas(&UnsubscribeBookDeltas::new(
                instrument_id(),
                Some(client_id()),
                None,
                UUID4::new(),
                UnixNanos::default(),
                None,
                None,
            ))
            .unwrap();

        let prices: Vec<(String, String)> = (0..12)
            .map(|i| (format!("{}.00", 84000 - i), format!("{}", i + 1)))
            .collect();
        let bids: Vec<(&str, &str)> = prices
            .iter()
            .map(|(p, s)| (p.as_str(), s.as_str()))
            .collect();
        send(
            &mut socket,
            &book("book20", 7, None, &bids, &[("84000.01", "1")]),
        )
        .await;

        let Data::BookDepth(depth) = next_data(&mut rx).await else {
            panic!("expected book depth");
        };

        let expected_bids: Vec<BookOrder> = prices
            .iter()
            .take(10)
            .map(|(price, size)| {
                BookOrder::new(
                    OrderSide::Buy,
                    Price::from(price.as_str()),
                    Quantity::from(format!("{size}.00000000").as_str()),
                    0,
                )
            })
            .collect();

        assert_eq!(
            too_deep.to_string(),
            "CoinAPI depth snapshots list at most 50 levels, was 51"
        );
        assert_eq!(
            l3.to_string(),
            "CoinAPI depth snapshots are L2_MBP, was L3_MBO"
        );
        assert_eq!(
            conflicting.to_string(),
            "Conflicting CoinAPI book subscription for BTC-USD.COINBASE: active book20 with \
             depth Some(10), requested book with depth None"
        );
        assert_eq!(
            *depth,
            OrderBookDepth::new(
                instrument_id(),
                expected_bids,
                vec![BookOrder::new(
                    OrderSide::Sell,
                    Price::from("84000.01"),
                    Quantity::from("1.00000000"),
                    0,
                )],
                vec![0; 10],
                vec![0],
                RecordFlag::F_SNAPSHOT as u8,
                7,
                nanos("2026-09-27T10:52:27.4861510Z"),
                nanos("2026-09-27T10:52:27.5708553Z"),
            )
        );

        client.disconnect().await.unwrap();
    })
    .await;
}

// CoinAPI retires the whole connection when a scope lists a second book type
#[tokio::test]
async fn second_book_channel_is_rejected_for_another_instrument() {
    bounded(async {
        let mut rx = data_channel();
        let (_http, http_address) = http_peer(vec![SYMBOLS.into()]).await;
        let (ws_listener, ws_address) = listener().await;
        let mut client = CoinApiDataClient::new(
            client_id(),
            config(&http_address, &ws_address, &[BTC_USD, "BTC-USDT.COINBASE"]),
        )
        .unwrap();
        client.connect().await.unwrap();
        next_event(&mut rx).await;
        next_event(&mut rx).await;

        subscribe_book_depth(&mut client, BookType::L2_MBP, Some(5)).unwrap();
        let mut socket = accept(&ws_listener).await;
        let hello = next_text(&mut socket).await;

        let error = client
            .subscribe_book_deltas(SubscribeBookDeltas::new(
                InstrumentId::from("BTC-USDT.COINBASE"),
                BookType::L2_MBP,
                Some(client_id()),
                None,
                UUID4::new(),
                UnixNanos::default(),
                None,
                false,
                None,
                None,
            ))
            .unwrap_err();

        assert_eq!(
            hello,
            r#"{"type":"hello","heartbeat":true,"subscribe_data_type":["book5"],"subscribe_filter_symbol_id":["COINBASE_SPOT_BTC_USD$"]}"#
        );
        assert_eq!(
            error.to_string(),
            "CoinAPI streams one book channel per connection: book5 is active, requested book"
        );

        client.disconnect().await.unwrap();
    })
    .await;
}

#[tokio::test]
async fn book_unsubscribe_removes_pair_from_scope() {
    bounded(async {
        let mut rx = data_channel();
        let (_http, http_address) = http_peer(vec![SYMBOLS.into()]).await;
        let (ws_listener, ws_address) = listener().await;
        let mut client = connected(&http_address, &ws_address, &mut rx).await;

        subscribe_trades(&mut client);
        let mut socket = accept(&ws_listener).await;
        assert_eq!(next_text(&mut socket).await, HELLO_TRADE);
        subscribe_book_deltas(&mut client, BookType::L2_MBP).unwrap();
        assert_eq!(next_text(&mut socket).await, HELLO_BOOK_TRADE);
        settle().await;

        send(
            &mut socket,
            &book("book", 1, Some(true), &[("84000.00", "1")], &[]),
        )
        .await;
        let snapshot = next_deltas(&mut rx).await;

        client
            .unsubscribe_book_deltas(&UnsubscribeBookDeltas::new(
                instrument_id(),
                Some(client_id()),
                None,
                UUID4::new(),
                UnixNanos::default(),
                None,
                None,
            ))
            .unwrap();
        let hello = next_text(&mut socket).await;

        send(
            &mut socket,
            &book("book", 2, Some(false), &[("84000.00", "2")], &[]),
        )
        .await;
        send(&mut socket, TRADE).await;

        assert_eq!(snapshot.deltas[0].action, BookAction::Clear);
        assert_eq!(hello, HELLO_TRADE);
        assert_eq!(next_data(&mut rx).await, expected_trade("84520.02"));

        client.disconnect().await.unwrap();
    })
    .await;
}

// Disconnecting forgets book subscriptions, so the engine's next subscribe reaches the venue
#[tokio::test]
async fn book_subscription_after_reconnecting_client_sends_scope() {
    bounded(async {
        let mut rx = data_channel();
        let (_http, http_address) = http_peer(vec![SYMBOLS.into(), SYMBOLS.into()]).await;
        let (ws_listener, ws_address) = listener().await;
        let mut client = connected(&http_address, &ws_address, &mut rx).await;

        subscribe_book_deltas(&mut client, BookType::L2_MBP).unwrap();
        let mut socket = accept(&ws_listener).await;
        let first = next_text(&mut socket).await;

        client.disconnect().await.unwrap();
        client.connect().await.unwrap();
        next_event(&mut rx).await;
        subscribe_book_deltas(&mut client, BookType::L2_MBP).unwrap();
        let mut socket = accept(&ws_listener).await;
        let second = next_text(&mut socket).await;

        let hello = r#"{"type":"hello","heartbeat":true,"subscribe_data_type":["book"],"subscribe_filter_symbol_id":["COINBASE_SPOT_BTC_USD$"]}"#;
        assert_eq!(first, hello);
        assert_eq!(second, hello);

        client.disconnect().await.unwrap();
    })
    .await;
}

// Restarting a book beside another symbol pauses the symbol's bars, so its retained period is
// discarded rather than closed without the paused updates
#[tokio::test]
async fn book_restart_discards_paused_symbol_retained_period() {
    bounded(async {
        let mut rx = data_channel();
        let (_http, http_address) = http_peer(vec![SYMBOLS.into()]).await;
        let (ws_listener, ws_address) = listener().await;
        let mut client = CoinApiDataClient::new(
            client_id(),
            config(&http_address, &ws_address, &[BTC_USD, "BTC-USDT.COINBASE"]),
        )
        .unwrap();
        client.connect().await.unwrap();
        next_event(&mut rx).await;
        next_event(&mut rx).await;

        subscribe_bars(&mut client);
        let mut socket = accept(&ws_listener).await;
        next_text(&mut socket).await;
        client
            .subscribe_trades(SubscribeTrades::new(
                InstrumentId::from("BTC-USDT.COINBASE"),
                Some(client_id()),
                None,
                UUID4::new(),
                UnixNanos::default(),
                None,
                None,
            ))
            .unwrap();
        next_text(&mut socket).await;
        subscribe_book_deltas(&mut client, BookType::L2_MBP).unwrap();
        next_text(&mut socket).await;
        settle().await;

        send(&mut socket, &ohlcv(1, "84523.69")).await;
        send(&mut socket, &book("book", 1, Some(true), &[("84000.00", "1")], &[])).await;
        next_deltas(&mut rx).await;
        send(&mut socket, &book("book", 3, Some(false), &[("84000.00", "2")], &[])).await;
        let removal = next_text(&mut socket).await;

        // An update sent before CoinAPI applied the removal restores the period during the pause,
        // after the client discarded it at the removal
        tokio::time::sleep(Duration::from_millis(300)).await;
        send(&mut socket, &ohlcv(1, "84523.68")).await;
        next_text(&mut socket).await;
        settle().await;

        send(&mut socket, &ohlcv(2, "84523.68")).await;
        send(&mut socket, &ohlcv(3, "84523.69")).await;

        assert_eq!(
            removal,
            r#"{"type":"hello","heartbeat":true,"subscribe_data_type":["trade"],"subscribe_filter_symbol_id":["COINBASE_SPOT_BTC_USDT$"]}"#
        );
        assert_bar(&next_data(&mut rx).await, 2, "84523.68");

        client.disconnect().await.unwrap();
    })
    .await;
}

#[tokio::test]
async fn missing_initial_snapshot_restarts_pair_after_deadline() {
    bounded(async {
        let mut rx = data_channel();
        let (_http, http_address) = http_peer(vec![SYMBOLS.into()]).await;
        let (ws_listener, ws_address) = listener().await;

        let config = CoinApiDataClientConfig {
            book_snapshot_timeout_secs: 1,
            ..config(&http_address, &ws_address, &[BTC_USD])
        };

        let mut client = CoinApiDataClient::new(client_id(), config).unwrap();
        client.connect().await.unwrap();

        let DataEvent::Instrument(_) = next_event(&mut rx).await else {
            panic!("expected instrument");
        };

        subscribe_trades(&mut client);
        let mut socket = accept(&ws_listener).await;
        assert_eq!(next_text(&mut socket).await, HELLO_TRADE);
        subscribe_book_deltas(&mut client, BookType::L2_MBP).unwrap();
        assert_eq!(next_text(&mut socket).await, HELLO_BOOK_TRADE);

        // Heartbeats keep the idle timeout from reconnecting while paced hellos wait
        send(&mut socket, HEARTBEAT).await;
        let started = tokio::time::Instant::now();
        assert_eq!(next_text(&mut socket).await, HELLO_TRADE);
        send(&mut socket, HEARTBEAT).await;
        assert_eq!(next_text(&mut socket).await, HELLO_BOOK_TRADE);
        let waited = started.elapsed();
        settle().await;

        send(
            &mut socket,
            &book("book", 1, Some(true), &[("84000.00", "1")], &[]),
        )
        .await;
        let snapshot = next_deltas(&mut rx).await;

        assert!(
            waited >= Duration::from_millis(900),
            "restarted after {waited:?}"
        );
        assert_eq!(snapshot.deltas[0].action, BookAction::Clear);
        assert_eq!(snapshot.deltas.len(), 2);

        client.disconnect().await.unwrap();
    })
    .await;
}

#[tokio::test]
async fn request_instrument_fetches_fresh_definition() {
    bounded(async {
        let mut rx = data_channel();
        let refreshed = SYMBOLS.replace("0.01000000000000000000", "0.1");
        let (http, http_address) = http_peer(vec![SYMBOLS.into(), SYMBOLS.into(), refreshed]).await;
        let mut client = connected(&http_address, "127.0.0.1:9", &mut rx).await;

        let mut responses = Vec::new();

        for _ in 0..2 {
            let request_id = UUID4::new();
            client
                .request_instrument(RequestInstrument::new(
                    instrument_id(),
                    None,
                    None,
                    Some(client_id()),
                    request_id,
                    UnixNanos::default(),
                    None,
                ))
                .unwrap();

            let DataEvent::Response(DataResponse::Instrument(response)) = next_event(&mut rx).await
            else {
                panic!("expected instrument response");
            };

            assert_eq!(response.correlation_id, request_id);
            assert_eq!(response.client_id, client_id());
            assert_eq!(response.instrument_id, instrument_id());
            responses.push(response.data.price_increment());
        }

        assert_eq!(responses, vec![Price::from("0.01"), Price::from("0.1")]);
        assert_eq!(http.await.unwrap(), vec![SYMBOLS_TARGET; 3]);

        client.disconnect().await.unwrap();
    })
    .await;
}

#[tokio::test]
async fn missing_replayed_snapshot_restarts_pair_after_reconnect() {
    bounded(async {
        let mut rx = data_channel();
        let (_http, http_address) = http_peer(vec![SYMBOLS.into()]).await;
        let (ws_listener, ws_address) = listener().await;

        let config = CoinApiDataClientConfig {
            book_snapshot_timeout_secs: 1,
            ..config(&http_address, &ws_address, &[BTC_USD])
        };

        let mut client = CoinApiDataClient::new(client_id(), config).unwrap();
        client.connect().await.unwrap();
        assert!(matches!(
            next_event(&mut rx).await,
            DataEvent::Instrument(_)
        ));
        let mut checker = BookStreamChecker::new(BookType::L2_MBP, true);
        checker.open(instrument_id());
        subscribe_trades(&mut client);
        let mut socket = accept(&ws_listener).await;
        assert_eq!(next_text(&mut socket).await, HELLO_TRADE);
        subscribe_book_deltas(&mut client, BookType::L2_MBP).unwrap();
        assert_eq!(next_text(&mut socket).await, HELLO_BOOK_TRADE);
        settle().await;
        send(
            &mut socket,
            &book("book", 1, Some(true), &[("84000.00", "1")], &[]),
        )
        .await;
        assert_eq!(check_next(&mut rx, &mut checker).await, (true, 1));

        socket.close(None).await.unwrap();
        drop(socket);
        let mut socket = accept(&ws_listener).await;
        assert_eq!(next_text(&mut socket).await, HELLO_BOOK_TRADE);
        send(&mut socket, HEARTBEAT).await;
        assert_eq!(next_text(&mut socket).await, HELLO_TRADE);
        assert!(matches!(
            rx.try_recv(),
            Err(tokio::sync::mpsc::error::TryRecvError::Empty)
        ));
        send(&mut socket, HEARTBEAT).await;
        assert_eq!(next_text(&mut socket).await, HELLO_BOOK_TRADE);
        settle().await;
        send(
            &mut socket,
            &book(
                "book",
                1,
                Some(true),
                &[("83999.99", "2")],
                &[("84000.01", "3")],
            ),
        )
        .await;
        let recovered = check_next(&mut rx, &mut checker).await;
        send(&mut socket, TRADE).await;
        let trade = next_data(&mut rx).await;

        assert_eq!(recovered, (true, 2));
        assert_eq!(
            checker.verify(
                instrument_id(),
                10,
                &levels(&[("83999.99", "2")]),
                &levels(&[("84000.01", "3")])
            ),
            Ok(())
        );
        assert_eq!(trade, expected_trade("84520.02"));
        client.disconnect().await.unwrap();
    })
    .await;
}

#[rstest]
#[case::stop(DataClient::stop)]
#[case::reset(DataClient::reset)]
#[case::dispose(DataClient::dispose)]
#[tokio::test]
async fn shutdown_closes_socket_and_reconnects_with_fresh_book_scope(
    #[case] shutdown: fn(&mut CoinApiDataClient) -> anyhow::Result<()>,
) {
    bounded(async {
        let mut rx = data_channel();
        let (_http, http_address) = http_peer(vec![SYMBOLS.into(), SYMBOLS.into()]).await;
        let (ws_listener, ws_address) = listener().await;
        let mut client = connected(&http_address, &ws_address, &mut rx).await;
        subscribe_book_deltas(&mut client, BookType::L2_MBP).unwrap();
        let mut socket = accept(&ws_listener).await;
        let hello = next_text(&mut socket).await;

        shutdown(&mut client).unwrap();
        assert!(client.is_disconnected());

        match socket.next().await {
            Some(Ok(Message::Close(_))) => socket.flush().await.unwrap(),
            Some(Err(tokio_tungstenite::tungstenite::Error::Protocol(
                tokio_tungstenite::tungstenite::error::ProtocolError::ResetWithoutClosingHandshake,
            ))) | None => {},
            other => panic!("expected closed socket, received {other:?}"),
        }

        drop(socket);
        client.connect().await.unwrap();
        assert!(matches!(next_event(&mut rx).await, DataEvent::Instrument(_)));
        subscribe_book_deltas(&mut client, BookType::L2_MBP).unwrap();
        let mut socket = accept(&ws_listener).await;
        let replay = next_text(&mut socket).await;
        settle().await;
        send(&mut socket, &book("book", 1, Some(true), &[("84000.00", "1")], &[])).await;
        let mut checker = BookStreamChecker::new(BookType::L2_MBP, true);
        checker.open(instrument_id());

        assert_eq!(hello, r#"{"type":"hello","heartbeat":true,"subscribe_data_type":["book"],"subscribe_filter_symbol_id":["COINBASE_SPOT_BTC_USD$"]}"#);
        assert_eq!(replay, hello);
        assert_eq!(check_next(&mut rx, &mut checker).await, (true, 1));
        assert_eq!(checker.verify(instrument_id(), 10, &levels(&[("84000.00", "1")]), &levels(&[])), Ok(()));
        client.disconnect().await.unwrap();
    }).await;
}

#[tokio::test]
async fn request_instruments_serves_only_configured_venue() {
    bounded(async {
        let mut rx = data_channel();
        let (http, http_address) =
            http_peer(vec![SYMBOLS.into(), SYMBOLS.into(), SYMBOLS.into()]).await;
        let mut client = connected(&http_address, "127.0.0.1:9", &mut rx).await;

        let mut responses = Vec::new();

        for venue in [Some("COINBASE"), Some("BINANCE"), None] {
            let request_id = UUID4::new();
            client
                .request_instruments(RequestInstruments::new(
                    None,
                    None,
                    Some(client_id()),
                    venue.map(Venue::new),
                    request_id,
                    UnixNanos::default(),
                    None,
                ))
                .unwrap();

            let DataEvent::Response(DataResponse::Instruments(response)) =
                next_event(&mut rx).await
            else {
                panic!("expected instruments response");
            };

            assert_eq!(response.correlation_id, request_id);
            assert_eq!(response.venue, Venue::new(venue.unwrap_or("COINAPI")));
            responses.push(response.data.iter().map(Instrument::id).collect::<Vec<_>>());
        }

        assert_eq!(
            responses,
            vec![vec![instrument_id()], vec![], vec![instrument_id()]]
        );
        assert_eq!(http.await.unwrap(), vec![SYMBOLS_TARGET; 3]);

        let error = client
            .request_instrument(RequestInstrument::new(
                InstrumentId::from("ETH-USD.COINBASE"),
                None,
                None,
                Some(client_id()),
                UUID4::new(),
                UnixNanos::default(),
                None,
            ))
            .unwrap_err();
        assert_eq!(
            error.to_string(),
            "ETH-USD.COINBASE is not a configured CoinAPI instrument"
        );

        client.disconnect().await.unwrap();
    })
    .await;
}

#[tokio::test]
async fn connect_fails_when_selected_instrument_is_missing() {
    bounded(async {
        let _rx = data_channel();
        let (_http, http_address) = http_peer(vec![SYMBOLS.into()]).await;
        let mut client = CoinApiDataClient::new(
            client_id(),
            config(
                &http_address,
                "127.0.0.1:9",
                &[BTC_USD, "BTC-USDC.COINBASE"],
            ),
        )
        .unwrap();

        let error = client.connect().await.unwrap_err();

        assert_eq!(
            format!("{error:#}"),
            "Failed to load CoinAPI instruments: CoinAPI COINBASE returned no instrument \
             BTC-USDC.COINBASE"
        );
        assert!(client.is_disconnected());
    })
    .await;
}

#[tokio::test]
async fn subscribe_before_connect_fails() {
    let _rx = data_channel();
    let mut client = CoinApiDataClient::new(
        client_id(),
        config("127.0.0.1:9", "127.0.0.1:9", &[BTC_USD]),
    )
    .unwrap();

    let error = client
        .subscribe_trades(SubscribeTrades::new(
            instrument_id(),
            Some(client_id()),
            None,
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ))
        .unwrap_err();

    assert_eq!(error.to_string(), "COINAPI is not connected");
}

#[tokio::test]
async fn request_instrument_refreshes_stream_precision() {
    bounded(async {
        let mut rx = data_channel();
        let coarse = SYMBOLS.replace("0.01000000000000000000", "0.1");
        let (_http, http_address) = http_peer(vec![coarse, SYMBOLS.into()]).await;
        let (ws_listener, ws_address) = listener().await;
        let mut client = connected(&http_address, &ws_address, &mut rx).await;

        client
            .request_instrument(RequestInstrument::new(
                instrument_id(),
                None,
                None,
                Some(client_id()),
                UUID4::new(),
                UnixNanos::default(),
                None,
            ))
            .unwrap();

        let DataEvent::Response(DataResponse::Instrument(response)) = next_event(&mut rx).await
        else {
            panic!("expected instrument response");
        };

        assert_eq!(response.data.price_increment(), Price::from("0.01"));

        // The connect-time increment of 0.1 would reject this trade's price
        subscribe_trades(&mut client);
        let mut socket = accept(&ws_listener).await;
        assert_eq!(next_text(&mut socket).await, HELLO_TRADE);
        send(&mut socket, TRADE).await;

        assert_eq!(next_data(&mut rx).await, expected_trade("84520.02"));

        client.disconnect().await.unwrap();
    })
    .await;
}

#[rstest]
#[case(false)]
#[case(true)]
#[tokio::test]
async fn instrument_refresh_preserves_stream_symbol(#[case] plural: bool) {
    bounded(async {
        let mut rx = data_channel();
        let coarse = SYMBOLS.replace("0.01000000000000000000", "0.1");
        let refreshed = SYMBOLS.replace(
            "\"COINBASE_SPOT_BTC_USD\"",
            "\"COINBASE_SPOT_BTC_USD_22F0062\"",
        );
        let (http, http_address) = http_peer(vec![coarse, refreshed.clone(), refreshed]).await;
        let (ws_listener, ws_address) = listener().await;
        let mut client = connected(&http_address, &ws_address, &mut rx).await;
        subscribe_trades(&mut client);
        let mut socket = accept(&ws_listener).await;
        assert_eq!(next_text(&mut socket).await, HELLO_TRADE);
        send(&mut socket, &TRADE.replace("84520.02", "84520.00")).await;
        assert_eq!(next_data(&mut rx).await, expected_trade("84520.00"));

        let request_id = UUID4::new();

        if plural {
            client
                .request_instruments(RequestInstruments::new(
                    None,
                    None,
                    Some(client_id()),
                    Some(Venue::new("COINBASE")),
                    request_id,
                    UnixNanos::default(),
                    None,
                ))
                .unwrap();
        } else {
            client
                .request_instrument(RequestInstrument::new(
                    instrument_id(),
                    None,
                    None,
                    Some(client_id()),
                    request_id,
                    UnixNanos::default(),
                    None,
                ))
                .unwrap();
        }

        let response = next_event(&mut rx).await;

        let instrument = match response {
            DataEvent::Response(DataResponse::Instrument(response)) if !plural => {
                assert_eq!(response.correlation_id, request_id);
                response.data
            }
            DataEvent::Response(DataResponse::Instruments(response)) if plural => {
                assert_eq!(response.correlation_id, request_id);
                assert_eq!(response.data.len(), 1);
                response.data.into_iter().next().unwrap()
            }
            other => panic!("expected instrument response, was {other:?}"),
        };

        assert_eq!(instrument.id(), instrument_id());
        assert_eq!(instrument.price_increment(), Price::from("0.01"));
        send(&mut socket, TRADE).await;
        assert_eq!(next_data(&mut rx).await, expected_trade("84520.02"));

        client
            .subscribe_quotes(SubscribeQuotes::new(
                instrument_id(),
                Some(client_id()),
                None,
                UUID4::new(),
                UnixNanos::default(),
                None,
                None,
            ))
            .unwrap();
        assert_eq!(next_text(&mut socket).await, HELLO_QUOTE_TRADE);
        send(&mut socket, &QUOTE.replace("BITSTAMP", "COINBASE")).await;
        assert_eq!(next_data(&mut rx).await, expected_quote());
        client
            .unsubscribe_trades(&UnsubscribeTrades::new(
                instrument_id(),
                Some(client_id()),
                None,
                UUID4::new(),
                UnixNanos::default(),
                None,
                None,
            ))
            .unwrap();
        assert_eq!(
            next_text(&mut socket).await,
            HELLO_QUOTE_TRADE.replace("\"quote\",\"trade\"", "\"quote\"")
        );
        client
            .unsubscribe_quotes(&UnsubscribeQuotes::new(
                instrument_id(),
                Some(client_id()),
                None,
                UUID4::new(),
                UnixNanos::default(),
                None,
                None,
            ))
            .unwrap();

        loop {
            if matches!(socket.next().await.unwrap().unwrap(), Message::Close(_)) {
                break;
            }
        }

        drop(socket);
        client.disconnect().await.unwrap();
        client.connect().await.unwrap();

        let DataEvent::Instrument(instrument) = next_event(&mut rx).await else {
            panic!("expected instrument");
        };

        assert_eq!(instrument.id(), instrument_id());
        subscribe_trades(&mut client);
        let mut socket = accept(&ws_listener).await;
        assert_eq!(
            next_text(&mut socket).await,
            HELLO_TRADE.replace("COINBASE_SPOT_BTC_USD", "COINBASE_SPOT_BTC_USD_22F0062")
        );
        send(
            &mut socket,
            &TRADE.replace("COINBASE_SPOT_BTC_USD", "COINBASE_SPOT_BTC_USD_22F0062"),
        )
        .await;
        assert_eq!(next_data(&mut rx).await, expected_trade("84520.02"));
        assert_eq!(http.await.unwrap(), vec![SYMBOLS_TARGET; 3]);
        client.disconnect().await.unwrap();
    })
    .await;
}

#[tokio::test]
async fn connect_after_disconnect_restores_streaming() {
    bounded(async {
        let mut rx = data_channel();
        let (http, http_address) = http_peer(vec![SYMBOLS.into(), SYMBOLS.into()]).await;
        let (ws_listener, ws_address) = listener().await;
        let mut client = connected(&http_address, &ws_address, &mut rx).await;

        client.disconnect().await.unwrap();
        assert!(client.is_disconnected());
        client.connect().await.unwrap();

        let DataEvent::Instrument(instrument) = next_event(&mut rx).await else {
            panic!("expected instrument");
        };

        assert_eq!(instrument.id(), instrument_id());
        assert_eq!(http.await.unwrap(), vec![SYMBOLS_TARGET; 2]);

        subscribe_trades(&mut client);
        let mut socket = accept(&ws_listener).await;
        assert_eq!(next_text(&mut socket).await, HELLO_TRADE);
        send(&mut socket, TRADE).await;

        assert_eq!(next_data(&mut rx).await, expected_trade("84520.02"));

        client.disconnect().await.unwrap();
    })
    .await;
}

#[tokio::test]
async fn subscribe_bars_rejects_internal_aggregation() {
    let _rx = data_channel();
    let mut client = CoinApiDataClient::new(
        client_id(),
        config("127.0.0.1:9", "127.0.0.1:9", &[BTC_USD]),
    )
    .unwrap();

    let error = client
        .subscribe_bars(SubscribeBars::new(
            BarType::from("BTC-USD.COINBASE-1-SECOND-LAST-INTERNAL"),
            Some(client_id()),
            None,
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ))
        .unwrap_err();

    assert_eq!(
        error.to_string(),
        "CoinAPI streams only external bars, was BTC-USD.COINBASE-1-SECOND-LAST-INTERNAL"
    );
}

#[tokio::test]
async fn subscription_changes_apply_in_call_order() {
    bounded(async {
        let mut rx = data_channel();
        let (_http, http_address) = http_peer(vec![SYMBOLS.into()]).await;
        let (ws_listener, ws_address) = listener().await;
        let mut client = connected(&http_address, &ws_address, &mut rx).await;

        subscribe_trades(&mut client);
        client
            .subscribe_quotes(SubscribeQuotes::new(
                instrument_id(),
                Some(client_id()),
                None,
                UUID4::new(),
                UnixNanos::default(),
                None,
                None,
            ))
            .unwrap();
        client
            .unsubscribe_trades(&UnsubscribeTrades::new(
                instrument_id(),
                Some(client_id()),
                None,
                UUID4::new(),
                UnixNanos::default(),
                None,
                None,
            ))
            .unwrap();

        // The runner may drain the queue between calls, so each hello it sends is one of the
        // call-order scopes, in order, ending with the unsubscribe applied
        let hello_quote = HELLO_QUOTE_TRADE.replace(r#"["quote","trade"]"#, r#"["quote"]"#);
        let scopes = [HELLO_TRADE, HELLO_QUOTE_TRADE, hello_quote.as_str()];
        let mut socket = accept(&ws_listener).await;
        let mut hellos = Vec::new();
        while hellos.last() != Some(&hello_quote) {
            hellos.push(next_text(&mut socket).await);
        }

        let mut remaining = scopes.iter();
        assert!(
            hellos
                .iter()
                .all(|hello| remaining.any(|scope| scope == hello)),
            "hellos out of call order: {hellos:?}"
        );

        send(&mut socket, TRADE).await;
        send(&mut socket, &QUOTE.replace("BITSTAMP", "COINBASE")).await;
        assert_eq!(next_data(&mut rx).await, expected_quote());

        client.disconnect().await.unwrap();
    })
    .await;
}

#[tokio::test]
async fn subscribe_retries_after_rejected_connection() {
    bounded(async {
        let mut rx = data_channel();
        let (_http, http_address) = http_peer(vec![SYMBOLS.into()]).await;
        let (ws_listener, ws_address) = listener().await;
        let mut client = connected(&http_address, &ws_address, &mut rx).await;

        subscribe_trades(&mut client);

        // A one-connection tier rejects the upgrade until the previous connection is released
        let (mut rejected, _) = ws_listener.accept().await.unwrap();
        let mut request = Vec::new();
        while !request.ends_with(b"\r\n\r\n") {
            request.push(rejected.read_u8().await.unwrap());
        }

        rejected
            .write_all(
                b"HTTP/1.1 429 Too Many Requests\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            )
            .await
            .unwrap();
        drop(rejected);

        let mut socket = accept(&ws_listener).await;
        assert_eq!(next_text(&mut socket).await, HELLO_TRADE);
        send(&mut socket, TRADE).await;

        assert_eq!(next_data(&mut rx).await, expected_trade("84520.02"));

        client.disconnect().await.unwrap();
    })
    .await;
}

#[rstest]
#[case(
    "https://ws.coinapi.io/v1/",
    "URL must be WS(S) without credentials, query, or fragment"
)]
#[case(
    "ws://127.0.0.1:9/v1/?apikey=x",
    "URL must be WS(S) without credentials, query, or fragment"
)]
#[case("not a url", "invalid URL")]
fn new_rejects_invalid_websocket_override(#[case] url: &str, #[case] reason: &str) {
    let _rx = data_channel();

    let config = CoinApiDataClientConfig {
        base_url_ws: Some(url.into()),
        ..config("127.0.0.1:9", "127.0.0.1:9", &[BTC_USD])
    };

    let error = CoinApiDataClient::new(client_id(), config).unwrap_err();

    assert_eq!(
        error.to_string(),
        format!("Invalid CoinAPI WebSocket request: {reason}")
    );
}

const HISTORY_BOOK: &str = include_str!("../../test_data/book.json");
const HISTORY_QUOTES: &str = include_str!("../../test_data/quotes.json");
const HISTORY_TRADES: &str = include_str!("../../test_data/trades.json");
const HISTORY_START: &str = "2026-09-23T09:00:00Z";
const HISTORY_END: &str = "2026-09-23T10:00:00Z";
const HISTORY_WINDOW: &str =
    "time_start=2026-09-23T09%3A00%3A00.0000000Z&time_end=2026-09-23T10%3A00%3A00.0000000Z";

fn at(timestamp: &str) -> Timestamp {
    timestamp.parse().unwrap()
}

fn nanos(timestamp: &str) -> UnixNanos {
    UnixNanos::from(at(timestamp).as_nanosecond() as u64)
}

fn params() -> Params {
    let mut params = Params::new();
    params.insert("tag".into(), serde_json::json!("history"));
    params
}

fn minute_bar_type() -> BarType {
    BarType::from("BTC-USD.COINBASE-1-MINUTE-LAST-EXTERNAL")
}

fn period(start: Timestamp, volume: &str) -> String {
    let end = start + SignedDuration::from_mins(1);
    format!(
        r#"{{"time_period_start":"{start}","time_period_end":"{end}","time_open":"{start}","time_close":"{start}","price_open":85988.49,"price_high":86007.23,"price_low":85986.91,"price_close":85998.69,"volume_traded":{volume},"trades_count":261}}"#
    )
}

#[rstest]
#[case(NonZeroUsize::new(10), "?limit_levels=10")]
#[case(None, "")]
#[tokio::test]
async fn request_book_snapshot_returns_correlated_book(
    #[case] depth: Option<NonZeroUsize>,
    #[case] query: &str,
) {
    bounded(async {
        let mut rx = data_channel();
        let (http, http_address) = http_peer(vec![SYMBOLS.into(), HISTORY_BOOK.into()]).await;
        let mut client = connected(&http_address, "127.0.0.1:9", &mut rx).await;
        let request_id = UUID4::new();

        client
            .request_book_snapshot(RequestBookSnapshot::new(
                instrument_id(),
                depth,
                Some(client_id()),
                request_id,
                UnixNanos::default(),
                Some(params()),
            ))
            .unwrap();

        let DataEvent::Response(DataResponse::Book(response)) = next_event(&mut rx).await else {
            panic!("expected book response");
        };

        let book = &response.data;
        assert_eq!(response.correlation_id, request_id);
        assert_eq!(response.client_id, client_id());
        assert_eq!(response.instrument_id, instrument_id());
        assert_eq!(response.start, None);
        assert_eq!(response.end, None);
        assert_eq!(response.params, Some(params()));
        assert_eq!(book.instrument_id, instrument_id());
        assert_eq!(book.book_type, BookType::L2_MBP);
        assert_eq!(book.ts_last, nanos("2026-09-24T09:10:36.0854790Z"));
        assert_eq!(book.bids(None).count(), 1);
        assert_eq!(book.asks(None).count(), 1);
        assert_eq!(book.best_bid_price(), Some(Price::from("83556.59")));
        assert_eq!(book.best_bid_size(), Some(Quantity::from("0.02515277")));
        assert_eq!(book.best_ask_price(), Some(Price::from("83556.60")));
        assert_eq!(book.best_ask_size(), Some(Quantity::from("0.01675362")));
        assert_eq!(
            http.await.unwrap(),
            vec![
                SYMBOLS_TARGET.to_owned(),
                format!("/v1/orderbooks/COINBASE_SPOT_BTC_USD/current{query}"),
            ]
        );

        client.disconnect().await.unwrap();
    })
    .await;
}

#[tokio::test]
async fn request_quotes_returns_correlated_ticks() {
    bounded(async {
        let mut rx = data_channel();
        let (http, http_address) = http_peer(vec![SYMBOLS.into(), HISTORY_QUOTES.into()]).await;
        let mut client = connected(&http_address, "127.0.0.1:9", &mut rx).await;
        let request_id = UUID4::new();

        client
            .request_quotes(RequestQuotes::new(
                instrument_id(),
                Some(at(HISTORY_START)),
                Some(at(HISTORY_END)),
                None,
                Some(client_id()),
                request_id,
                UnixNanos::default(),
                Some(params()),
            ))
            .unwrap();

        let DataEvent::Response(mut response) = next_event(&mut rx).await else {
            panic!("expected response");
        };

        // The data engine bounds every response by ts_init before delivering it
        response.trim_to_bounds();

        let DataResponse::Quotes(response) = response else {
            panic!("expected quotes response");
        };

        assert_eq!(response.correlation_id, request_id);
        assert_eq!(response.client_id, client_id());
        assert_eq!(response.instrument_id, instrument_id());
        assert_eq!(
            response.data,
            vec![QuoteTick::new(
                instrument_id(),
                Price::from("85988.48"),
                Price::from("85988.49"),
                Quantity::from("0.05799828"),
                Quantity::from("0.00075591"),
                nanos("2026-09-23T09:09:00.0822750Z"),
                nanos("2026-09-23T09:09:00.0822750Z"),
            )]
        );
        assert_eq!(response.start, Some(nanos(HISTORY_START)));
        assert_eq!(response.end, Some(nanos(HISTORY_END)));
        assert_eq!(response.params, Some(params()));

        // Without a limit, the default 100-credit budget sizes the page
        assert_eq!(
            http.await.unwrap(),
            vec![
                SYMBOLS_TARGET.to_owned(),
                format!("/v1/quotes/COINBASE_SPOT_BTC_USD/history?{HISTORY_WINDOW}&limit=9900"),
            ]
        );

        client.disconnect().await.unwrap();
    })
    .await;
}

#[tokio::test]
async fn request_trades_returns_correlated_ticks() {
    bounded(async {
        let mut rx = data_channel();
        let (http, http_address) = http_peer(vec![SYMBOLS.into(), HISTORY_TRADES.into()]).await;
        let mut client = connected(&http_address, "127.0.0.1:9", &mut rx).await;
        let request_id = UUID4::new();

        client
            .request_trades(RequestTrades::new(
                instrument_id(),
                Some(at(HISTORY_START)),
                Some(at(HISTORY_END)),
                NonZeroUsize::new(5),
                Some(client_id()),
                request_id,
                UnixNanos::default(),
                Some(params()),
            ))
            .unwrap();

        let DataEvent::Response(mut response) = next_event(&mut rx).await else {
            panic!("expected response");
        };

        // The data engine bounds every response by ts_init before delivering it
        response.trim_to_bounds();

        let DataResponse::Trades(response) = response else {
            panic!("expected trades response");
        };

        assert_eq!(response.correlation_id, request_id);
        assert_eq!(response.client_id, client_id());
        assert_eq!(response.instrument_id, instrument_id());
        assert_eq!(
            response.data,
            vec![TradeTick::new(
                instrument_id(),
                Price::from("85988.49"),
                Quantity::from("0.01000000"),
                AggressorSide::Buy,
                TradeId::new("f84a79ad-ea86-4527-b1f1-e9d8bd7ed004"),
                nanos("2026-09-23T09:09:00.0822750Z"),
                nanos("2026-09-23T09:09:00.0822750Z"),
            )]
        );
        assert_eq!(response.start, Some(nanos(HISTORY_START)));
        assert_eq!(response.end, Some(nanos(HISTORY_END)));
        assert_eq!(response.params, Some(params()));
        assert_eq!(
            http.await.unwrap(),
            vec![
                SYMBOLS_TARGET.to_owned(),
                format!("/v1/trades/COINBASE_SPOT_BTC_USD/history?{HISTORY_WINDOW}&limit=5"),
            ]
        );

        client.disconnect().await.unwrap();
    })
    .await;
}

#[tokio::test]
async fn request_trades_rounds_window_up_to_provider_precision() {
    bounded(async {
        let mut rx = data_channel();
        let (http, http_address) = http_peer(vec![SYMBOLS.into(), HISTORY_TRADES.into()]).await;
        let mut client = connected(&http_address, "127.0.0.1:9", &mut rx).await;
        let start = "2026-09-23T09:09:00.082274950Z";
        let end = "2026-09-23T09:09:00.082275050Z";

        client
            .request_trades(RequestTrades::new(
                instrument_id(),
                Some(at(start)),
                Some(at(end)),
                NonZeroUsize::new(5),
                Some(client_id()),
                UUID4::new(),
                UnixNanos::default(),
                None,
            ))
            .unwrap();

        let DataEvent::Response(mut response) = next_event(&mut rx).await else {
            panic!("expected response");
        };

        response.trim_to_bounds();

        let DataResponse::Trades(response) = response else {
            panic!("expected trades response");
        };

        assert_eq!(
            response
                .data
                .iter()
                .map(|trade| trade.ts_event)
                .collect::<Vec<_>>(),
            vec![nanos("2026-09-23T09:09:00.0822750Z")]
        );
        assert_eq!(response.start, Some(nanos(start)));
        assert_eq!(response.end, Some(nanos(end)));
        assert_eq!(
            http.await.unwrap(),
            vec![
                SYMBOLS_TARGET.to_owned(),
                "/v1/trades/COINBASE_SPOT_BTC_USD/history?time_start=2026-09-23T09%3A09%3A00.0822750Z&time_end=2026-09-23T09%3A09%3A00.0822751Z&limit=5".to_owned(),
            ]
        );

        client.disconnect().await.unwrap();
    })
    .await;
}

#[tokio::test]
async fn request_bars_returns_only_closed_periods_at_instrument_precision() {
    bounded(async {
        let mut rx = data_channel();
        let now = Timestamp::now();
        let closed = at("2026-09-23T09:09:00Z");
        let noisy = at("2026-09-23T09:10:00Z");
        let finer = at("2026-09-23T09:11:00Z");
        let open_now = now - SignedDuration::from_secs(10);
        let open_at_end = at("2026-09-23T09:59:00Z");
        let bodies = [
            format!(
                "[{},{},{},{}]",
                period(closed, "0.97075568"),
                period(noisy, "0.9707556800000001"),
                period(finer, "0.970755681"),
                period(open_now, "0.5")
            ),
            format!("[{}]", period(open_at_end, "0.5")),
            format!("[{},{}]", period(closed, "0.97075568"), period(open_now, "0.5")),
        ];
        let mut served = vec![SYMBOLS.to_owned()];
        served.extend(bodies);
        let (http, http_address) = http_peer(served).await;
        let mut client = connected(&http_address, "127.0.0.1:9", &mut rx).await;

        // The windows run to now, end partway through their only period, and end after now
        let ends = [
            None,
            Some(at("2026-09-23T09:59:30Z")),
            Some(now + SignedDuration::from_hours(1)),
        ];
        let mut responses = Vec::new();

        for end in ends {
            let request_id = UUID4::new();
            client
                .request_bars(RequestBars::new(
                    minute_bar_type(),
                    Some(at(HISTORY_START)),
                    end,
                    None,
                    Some(client_id()),
                    request_id,
                    UnixNanos::default(),
                    Some(params()),
                ))
                .unwrap();

            let DataEvent::Response(mut response) = next_event(&mut rx).await else {
                panic!("expected response");
            };

            // The data engine bounds every response by ts_init before delivering it
            response.trim_to_bounds();

            let DataResponse::Bars(response) = response else {
                panic!("expected bars response");
            };

            assert_eq!(response.correlation_id, request_id);
            assert_eq!(response.client_id, client_id());
            assert_eq!(response.bar_type, minute_bar_type());
            assert_eq!(response.start, Some(nanos(HISTORY_START)));
            assert_eq!(response.end, end.map(|end| nanos(&end.to_string())));
            assert_eq!(response.params, Some(params()));
            responses.push(response.data);
        }

        let closed_bar = Bar::new(
            minute_bar_type(),
            Price::from("85988.49"),
            Price::from("86007.23"),
            Price::from("85986.91"),
            Price::from("85998.69"),
            Quantity::from("0.97075568"),
            nanos("2026-09-23T09:10:00Z"),
            nanos("2026-09-23T09:10:00Z"),
        );
        let mut normalized_bar = closed_bar;
        normalized_bar.ts_event = nanos("2026-09-23T09:11:00Z");
        normalized_bar.ts_init = nanos("2026-09-23T09:11:00Z");
        assert_eq!(
            responses,
            vec![vec![closed_bar, normalized_bar], vec![], vec![closed_bar]]
        );

        // The first page billed one credit, which sizes the second page from 98 credits
        let targets = http.await.unwrap();
        assert_eq!(targets.len(), 4);
        assert!(targets[1].starts_with(
            "/v1/ohlcv/COINBASE_SPOT_BTC_USD/history?period_id=1MIN&time_start=2026-09-23T09%3A00%3A00.0000000Z&time_end="
        ));
        assert_eq!(
            targets[2],
            "/v1/ohlcv/COINBASE_SPOT_BTC_USD/history?period_id=1MIN&time_start=2026-09-23T09%3A00%3A00.0000000Z&time_end=2026-09-23T09%3A59%3A30.0000000Z&limit=9800"
        );

        client.disconnect().await.unwrap();
    })
    .await;
}

#[tokio::test]
async fn history_requests_reject_before_sending() {
    bounded(async {
        let mut rx = data_channel();
        let (http, http_address) = http_peer(vec![SYMBOLS.into()]).await;
        let mut client = connected(&http_address, "127.0.0.1:9", &mut rx).await;

        let trades = |instrument_id: InstrumentId, start: Option<Timestamp>| {
            RequestTrades::new(
                instrument_id,
                start,
                None,
                None,
                Some(client_id()),
                UUID4::new(),
                UnixNanos::default(),
                None,
            )
        };

        let bars = |bar_type: &str| {
            RequestBars::new(
                BarType::from(bar_type),
                Some(at(HISTORY_START)),
                None,
                None,
                Some(client_id()),
                UUID4::new(),
                UnixNanos::default(),
                None,
            )
        };

        let errors = [
            client.request_book_snapshot(RequestBookSnapshot::new(
                InstrumentId::from("ETH-USD.COINBASE"),
                None,
                Some(client_id()),
                UUID4::new(),
                UnixNanos::default(),
                None,
            )),
            client.request_trades(trades(
                InstrumentId::from("ETH-USD.COINBASE"),
                Some(at(HISTORY_START)),
            )),
            client.request_trades(trades(instrument_id(), None)),
            client.request_bars(bars("BTC-USD.COINBASE-1-MINUTE-LAST-INTERNAL")),
            client.request_bars(bars("BTC-USD.COINBASE-12-SECOND-LAST-EXTERNAL")),
        ]
        .map(|result| result.unwrap_err().to_string());

        assert_eq!(
            errors,
            [
                "ETH-USD.COINBASE is not a loaded CoinAPI instrument",
                "ETH-USD.COINBASE is not a loaded CoinAPI instrument",
                "CoinAPI historical requests require a start time",
                "CoinAPI provides only external bars, was BTC-USD.COINBASE-1-MINUTE-LAST-INTERNAL",
                "CoinAPI has no streaming OHLCV period for 12-SECOND-LAST",
            ]
        );
        assert_eq!(http.await.unwrap(), vec![SYMBOLS_TARGET]);
        assert!(rx.try_recv().is_err());

        client.disconnect().await.unwrap();
    })
    .await;
}
