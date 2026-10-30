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

//! Integration tests for the Databento feed handler.
//!
//! Tests the `DatabentoFeedHandler` against a mock LSG (Live Streaming Gateway)
//! server that implements the Databento wire protocol. Covers data test groups
//! from `spec_data_testing.md` (TC-D02, TC-D10, TC-D12, TC-D20, TC-D30, TC-D40,
//! TC-D60, TC-D70) plus protocol and reconnection tests.

use std::time::Duration;

use databento::{
    dbn::{self},
    live::Subscription,
};
use nautilus_common::testing::wait_until_async;
use nautilus_databento::live::{DatabentoMessage, HandlerCommand};
use nautilus_model::{
    data::Data,
    enums::{AggressorSide, BookAction, OrderSide},
    identifiers::{InstrumentId, Symbol, TradeId, Venue, futures::FuturesYearDigits},
    instruments::Instrument,
    types::{Price, Quantity},
};
use rstest::rstest;

use crate::common::{
    TEST_DATASET, TestHandlerConfig, create_test_handler, create_test_handler_with_config,
    error_msg, imbalance_msg, instrument_def_msg, mbo_msg, mbo_msg_with_ts, mbp1_msg, mbp10_msg,
    mock_server::MockLsgServer, ohlcv_msg, statistics_msg, status_msg, symbol_mapping_msg,
    symbol_mapping_msg_with_stype, system_msg, trade_msg,
};

const INSTRUMENT_ID: u32 = 1;
const RAW_SYMBOL: &str = "ESM4";
const CONTINUOUS_SYMBOL: &str = "ES.c.0";
const RECV_TIMEOUT: Duration = Duration::from_secs(5);

async fn recv_msg(
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<DatabentoMessage>,
) -> DatabentoMessage {
    tokio::time::timeout(RECV_TIMEOUT, rx.recv())
        .await
        .expect("timed out waiting for message")
        .expect("channel closed")
}

fn subscription(schema: dbn::Schema) -> Subscription {
    Subscription::builder()
        .symbols(RAW_SYMBOL)
        .schema(schema)
        .stype_in(dbn::SType::RawSymbol)
        .build()
}

#[rstest]
#[tokio::test]
async fn test_connect_and_authenticate() {
    let server = MockLsgServer::new(TEST_DATASET).await;
    let (cmd_tx, mut msg_rx, mut handler) = create_test_handler(&server.addr(), TEST_DATASET);

    server.authenticate();
    server.expect_subscription();
    server.start();

    let handle = tokio::spawn(async move { handler.run().await });

    cmd_tx
        .send(HandlerCommand::Subscribe(subscription(dbn::Schema::Trades)))
        .unwrap();
    cmd_tx.send(HandlerCommand::Start).unwrap();

    tokio::time::sleep(Duration::from_millis(100)).await;
    cmd_tx.send(HandlerCommand::Close).unwrap();

    let close_msg = recv_msg(&mut msg_rx).await;
    assert!(matches!(close_msg, DatabentoMessage::Close));

    handle.await.unwrap().unwrap();
    server.stop().await;
}

#[rstest]
#[tokio::test]
async fn test_close_command_stops_handler() {
    let server = MockLsgServer::new(TEST_DATASET).await;
    let (cmd_tx, mut msg_rx, mut handler) = create_test_handler(&server.addr(), TEST_DATASET);

    server.authenticate();
    server.expect_subscription();
    server.start();

    let handle = tokio::spawn(async move { handler.run().await });

    cmd_tx
        .send(HandlerCommand::Subscribe(subscription(dbn::Schema::Trades)))
        .unwrap();
    cmd_tx.send(HandlerCommand::Start).unwrap();

    tokio::time::sleep(Duration::from_millis(100)).await;
    cmd_tx.send(HandlerCommand::Close).unwrap();

    let msg = recv_msg(&mut msg_rx).await;
    assert!(matches!(msg, DatabentoMessage::Close));

    let result = handle.await.unwrap();
    assert!(result.is_ok());

    server.stop().await;
}

#[rstest]
#[tokio::test]
async fn test_subscribe_trades() {
    let server = MockLsgServer::new(TEST_DATASET).await;
    let (cmd_tx, mut msg_rx, mut handler) = create_test_handler(&server.addr(), TEST_DATASET);

    server.authenticate();
    server.expect_subscription();
    server.start();
    server.send_record(symbol_mapping_msg(INSTRUMENT_ID, RAW_SYMBOL));
    server.send_record(trade_msg(INSTRUMENT_ID, 1_170_400_000, 50));
    server.disconnect();

    let handle = tokio::spawn(async move { handler.run().await });

    cmd_tx
        .send(HandlerCommand::SetPricePrecision(
            Symbol::from(RAW_SYMBOL),
            5,
        ))
        .unwrap();
    cmd_tx
        .send(HandlerCommand::Subscribe(subscription(dbn::Schema::Trades)))
        .unwrap();
    cmd_tx.send(HandlerCommand::Start).unwrap();

    let msg = recv_msg(&mut msg_rx).await;
    match msg {
        DatabentoMessage::Data(Data::Trade(trade)) => {
            assert_eq!(trade.instrument_id.symbol.as_str(), RAW_SYMBOL);
            assert_eq!(trade.price, Price::from("1.17040"));
            assert_eq!(trade.price.precision, 5);
            assert_eq!(trade.size, Quantity::from(50));
        }
        other => panic!("expected Data::Trade, was {other:?}"),
    }

    cmd_tx.send(HandlerCommand::Close).unwrap();
    let _ = handle.await;
    server.stop().await;
}

#[rstest]
#[tokio::test]
async fn test_futures_record_without_publisher_venue_does_not_end_session() {
    let server = MockLsgServer::new(TEST_DATASET).await;
    let (cmd_tx, mut msg_rx, handler) = create_test_handler(&server.addr(), TEST_DATASET);
    let mut handler = handler.with_futures_year_digits(FuturesYearDigits::Two);
    let mut invalid = trade_msg(INSTRUMENT_ID, 1_010_000_000, 17);
    invalid.hd.publisher_id = dbn::Publisher::XoseItchXose as u16;

    server.authenticate();
    server.expect_subscription();
    server.start();
    server.send_record(symbol_mapping_msg(INSTRUMENT_ID, RAW_SYMBOL));
    server.send_record(invalid);
    server.send_record(trade_msg(INSTRUMENT_ID, 1_170_400_000, 50));

    let handle = tokio::spawn(async move { handler.run().await });
    cmd_tx
        .send(HandlerCommand::SetPricePrecision(
            Symbol::from(RAW_SYMBOL),
            5,
        ))
        .unwrap();
    cmd_tx
        .send(HandlerCommand::Subscribe(subscription(dbn::Schema::Trades)))
        .unwrap();
    cmd_tx.send(HandlerCommand::Start).unwrap();

    let msg = recv_msg(&mut msg_rx).await;
    cmd_tx.send(HandlerCommand::Close).unwrap();
    handle.await.unwrap().unwrap();
    server.stop().await;

    match msg {
        DatabentoMessage::Data(Data::Trade(trade)) => {
            assert_eq!(trade.instrument_id.to_string(), "ESM74.GLBX");
            assert_eq!(trade.price, Price::from("1.17040"));
            assert_eq!(trade.size, Quantity::from(50));
            assert_eq!(trade.aggressor_side, AggressorSide::Sell);
            assert_eq!(trade.trade_id, TradeId::from("1"));
            assert_eq!(trade.ts_event.as_u64(), 1_000_000_000);
        }
        other => panic!("expected Data::Trade, was {other:?}"),
    }
}

#[rstest]
#[tokio::test]
async fn test_subscribe_trades_uses_non_raw_symbol_precision_fallback() {
    let server = MockLsgServer::new(TEST_DATASET).await;
    let (cmd_tx, mut msg_rx, mut handler) = create_test_handler(&server.addr(), TEST_DATASET);

    server.authenticate();
    server.expect_subscription();
    server.start();
    server.send_record(symbol_mapping_msg_with_stype(
        INSTRUMENT_ID,
        dbn::SType::Continuous,
        CONTINUOUS_SYMBOL,
        RAW_SYMBOL,
    ));
    server.send_record(trade_msg(INSTRUMENT_ID, 1_170_400_000, 50));
    server.disconnect();

    let handle = tokio::spawn(async move { handler.run().await });

    cmd_tx
        .send(HandlerCommand::SetPricePrecision(
            Symbol::from(CONTINUOUS_SYMBOL),
            5,
        ))
        .unwrap();
    cmd_tx
        .send(HandlerCommand::Subscribe(
            Subscription::builder()
                .symbols(CONTINUOUS_SYMBOL)
                .schema(dbn::Schema::Trades)
                .stype_in(dbn::SType::Continuous)
                .build(),
        ))
        .unwrap();
    cmd_tx.send(HandlerCommand::Start).unwrap();

    let msg = recv_msg(&mut msg_rx).await;
    match msg {
        DatabentoMessage::Data(Data::Trade(trade)) => {
            assert_eq!(trade.instrument_id.symbol.as_str(), RAW_SYMBOL);
            assert_eq!(trade.price, Price::from("1.17040"));
            assert_eq!(trade.price.precision, 5);
        }
        other => panic!("expected Data::Trade, was {other:?}"),
    }

    cmd_tx.send(HandlerCommand::Close).unwrap();
    let _ = handle.await;
    server.stop().await;
}

#[rstest]
#[case(dbn::SType::RawSymbol, RAW_SYMBOL)]
#[case(dbn::SType::Continuous, CONTINUOUS_SYMBOL)]
#[tokio::test]
async fn test_explicit_precision_takes_precedence_over_instrument_definition(
    #[case] stype: dbn::SType,
    #[case] symbol: &str,
) {
    let server = MockLsgServer::new(TEST_DATASET).await;
    let (cmd_tx, mut msg_rx, mut handler) = create_test_handler(&server.addr(), TEST_DATASET);

    server.authenticate();
    server.expect_subscription();
    server.start();
    server.send_record(symbol_mapping_msg_with_stype(
        INSTRUMENT_ID,
        stype,
        symbol,
        RAW_SYMBOL,
    ));
    server.send_record(trade_msg(INSTRUMENT_ID, 1_083_450_000, 50));
    server.send_record(instrument_def_msg(INSTRUMENT_ID, b'F'));
    server.send_record(trade_msg(INSTRUMENT_ID, 1_083_460_000, 51));
    server.disconnect();

    let handle = tokio::spawn(async move { handler.run().await });

    cmd_tx
        .send(HandlerCommand::SetPricePrecision(Symbol::from(symbol), 5))
        .unwrap();
    cmd_tx
        .send(HandlerCommand::Subscribe(
            Subscription::builder()
                .symbols(symbol)
                .schema(dbn::Schema::Trades)
                .stype_in(stype)
                .build(),
        ))
        .unwrap();
    cmd_tx.send(HandlerCommand::Start).unwrap();

    let DatabentoMessage::Data(Data::Trade(before)) = recv_msg(&mut msg_rx).await else {
        panic!("expected trade before definition");
    };

    let DatabentoMessage::Instrument(instrument) = recv_msg(&mut msg_rx).await else {
        panic!("expected instrument definition");
    };

    let msg = recv_msg(&mut msg_rx).await;
    match msg {
        DatabentoMessage::Data(Data::Trade(trade)) => {
            assert_eq!(before.price, Price::from("1.08345"));
            assert_eq!(before.price.precision, 5);
            assert_eq!(before.size, Quantity::from(50));
            assert_eq!(instrument.price_precision(), 2);
            assert_eq!(instrument.price_increment(), Price::from("0.01"));
            assert_eq!(trade.price, Price::from("1.08346"));
            assert_eq!(trade.price.precision, 5);
            assert_eq!(trade.size, Quantity::from(51));
        }
        other => panic!("expected Data::Trade, was {other:?}"),
    }

    cmd_tx.send(HandlerCommand::Close).unwrap();
    let _ = handle.await;
    server.stop().await;
}

#[rstest]
#[tokio::test]
async fn test_symbol_mapping_clears_stale_instrument_definition_precision() {
    let server = MockLsgServer::new(TEST_DATASET).await;
    let (cmd_tx, mut msg_rx, mut handler) = create_test_handler(&server.addr(), TEST_DATASET);

    server.authenticate();
    server.expect_subscription();
    server.start();
    server.send_record(symbol_mapping_msg(INSTRUMENT_ID, RAW_SYMBOL));
    server.send_record(instrument_def_msg(INSTRUMENT_ID, b'F'));
    server.send_record(symbol_mapping_msg_with_stype(
        INSTRUMENT_ID,
        dbn::SType::Continuous,
        CONTINUOUS_SYMBOL,
        RAW_SYMBOL,
    ));
    server.send_record(trade_msg(INSTRUMENT_ID, 1_170_400_000, 50));
    server.disconnect();

    let handle = tokio::spawn(async move { handler.run().await });

    cmd_tx
        .send(HandlerCommand::SetPricePrecision(
            Symbol::from(CONTINUOUS_SYMBOL),
            5,
        ))
        .unwrap();
    cmd_tx
        .send(HandlerCommand::Subscribe(
            Subscription::builder()
                .symbols(CONTINUOUS_SYMBOL)
                .schema(dbn::Schema::Trades)
                .stype_in(dbn::SType::Continuous)
                .build(),
        ))
        .unwrap();
    cmd_tx.send(HandlerCommand::Start).unwrap();

    let _instrument = recv_msg(&mut msg_rx).await;

    let msg = recv_msg(&mut msg_rx).await;
    match msg {
        DatabentoMessage::Data(Data::Trade(trade)) => {
            assert_eq!(trade.price, Price::from("1.17040"));
            assert_eq!(trade.price.precision, 5);
        }
        other => panic!("expected Data::Trade, was {other:?}"),
    }

    cmd_tx.send(HandlerCommand::Close).unwrap();
    let _ = handle.await;
    server.stop().await;
}

#[rstest]
#[tokio::test]
async fn test_subscribe_quotes_mbp1() {
    let server = MockLsgServer::new(TEST_DATASET).await;
    let (cmd_tx, mut msg_rx, mut handler) = create_test_handler(&server.addr(), TEST_DATASET);

    server.authenticate();
    server.expect_subscription();
    server.start();
    server.send_record(symbol_mapping_msg(INSTRUMENT_ID, RAW_SYMBOL));
    server.send_record(mbp1_msg(
        INSTRUMENT_ID,
        1_170_350_000, // bid 1.17035
        1_170_400_000, // ask 1.17040
        b'A',          // Add action
    ));
    server.disconnect();

    let handle = tokio::spawn(async move { handler.run().await });

    cmd_tx
        .send(HandlerCommand::SetPricePrecision(
            Symbol::from(RAW_SYMBOL),
            5,
        ))
        .unwrap();
    cmd_tx
        .send(HandlerCommand::Subscribe(subscription(dbn::Schema::Mbp1)))
        .unwrap();
    cmd_tx.send(HandlerCommand::Start).unwrap();

    let msg = recv_msg(&mut msg_rx).await;
    match msg {
        DatabentoMessage::Data(Data::Quote(quote)) => {
            assert_eq!(quote.instrument_id.symbol.as_str(), RAW_SYMBOL);
            assert_eq!(quote.bid_price, Price::from("1.17035"));
            assert_eq!(quote.bid_price.precision, 5);
            assert_eq!(quote.ask_price, Price::from("1.17040"));
            assert_eq!(quote.ask_price.precision, 5);
        }
        other => panic!("expected Data::Quote, was {other:?}"),
    }

    cmd_tx.send(HandlerCommand::Close).unwrap();
    let _ = handle.await;
    server.stop().await;
}

#[rstest]
#[tokio::test]
async fn test_subscribe_quotes_with_trade() {
    let server = MockLsgServer::new(TEST_DATASET).await;
    let (cmd_tx, mut msg_rx, mut handler) = create_test_handler(&server.addr(), TEST_DATASET);

    server.authenticate();
    server.expect_subscription();
    server.start();
    server.send_record(symbol_mapping_msg(INSTRUMENT_ID, RAW_SYMBOL));
    server.send_record(mbp1_msg(
        INSTRUMENT_ID,
        100_000_000_000,
        101_000_000_000,
        b'T', // Trade action
    ));
    server.disconnect();

    let handle = tokio::spawn(async move { handler.run().await });

    cmd_tx
        .send(HandlerCommand::Subscribe(subscription(dbn::Schema::Mbp1)))
        .unwrap();
    cmd_tx.send(HandlerCommand::Start).unwrap();

    let mut got_quote = false;
    let mut got_trade = false;

    for _ in 0..2 {
        let msg = recv_msg(&mut msg_rx).await;
        match msg {
            DatabentoMessage::Data(Data::Quote(_)) => got_quote = true,
            DatabentoMessage::Data(Data::Trade(_)) => got_trade = true,
            _ => {}
        }
    }

    assert!(got_quote, "expected a QuoteTick");
    assert!(got_trade, "expected a TradeTick");

    cmd_tx.send(HandlerCommand::Close).unwrap();
    let _ = handle.await;
    server.stop().await;
}

#[rstest]
#[tokio::test]
async fn test_trades_and_mbp1_send_each_trade_once() {
    let server = MockLsgServer::new(TEST_DATASET).await;
    let (cmd_tx, mut msg_rx, mut handler) = create_test_handler(&server.addr(), TEST_DATASET);

    server.authenticate();
    server.expect_subscription();
    server.expect_subscription();
    server.start();
    server.send_record(symbol_mapping_msg(INSTRUMENT_ID, RAW_SYMBOL));

    // Delivers the instrument's first trade, so `trades` becomes its trade source
    server.send_record(trade_msg(INSTRUMENT_ID, 100_000_000_000, 10));

    // The same trade on `mbp-1`: the quote is sent and the trade is dropped
    server.send_record(mbp1_msg(
        INSTRUMENT_ID,
        100_000_000_000,
        101_000_000_000,
        b'T',
    ));

    // Must arrive directly after the quote, proving the `mbp-1` trade was dropped
    server.send_record(trade_msg(INSTRUMENT_ID, 100_000_000_000, 60));
    server.disconnect();

    let handle = tokio::spawn(async move { handler.run().await });

    cmd_tx
        .send(HandlerCommand::Subscribe(subscription(dbn::Schema::Trades)))
        .unwrap();
    cmd_tx
        .send(HandlerCommand::Subscribe(subscription(dbn::Schema::Mbp1)))
        .unwrap();
    cmd_tx.send(HandlerCommand::Start).unwrap();

    let msg = recv_msg(&mut msg_rx).await;
    match msg {
        DatabentoMessage::Data(Data::Trade(trade)) => {
            assert_eq!(trade.price, Price::from("100.00"));
            assert_eq!(trade.size, Quantity::from(10));
        }
        other => panic!("expected Data::Trade, was {other:?}"),
    }

    let msg = recv_msg(&mut msg_rx).await;
    match msg {
        DatabentoMessage::Data(Data::Quote(quote)) => {
            assert_eq!(quote.bid_price, Price::from("100.00"));
            assert_eq!(quote.ask_price, Price::from("101.00"));
        }
        other => panic!("expected Data::Quote, was {other:?}"),
    }

    let msg = recv_msg(&mut msg_rx).await;
    match msg {
        DatabentoMessage::Data(Data::Trade(trade)) => {
            assert_eq!(trade.size, Quantity::from(60));
        }
        other => panic!("expected Data::Trade, was {other:?}"),
    }

    cmd_tx.send(HandlerCommand::Close).unwrap();
    let _ = handle.await;
    server.stop().await;
}

#[rstest]
#[tokio::test]
async fn test_subscribe_bars_ohlcv() {
    let server = MockLsgServer::new(TEST_DATASET).await;
    let (cmd_tx, mut msg_rx, mut handler) = create_test_handler(&server.addr(), TEST_DATASET);

    server.authenticate();
    server.expect_subscription();
    server.start();
    server.send_record(symbol_mapping_msg(INSTRUMENT_ID, RAW_SYMBOL));
    server.send_record(ohlcv_msg(INSTRUMENT_ID));
    server.disconnect();

    let handle = tokio::spawn(async move { handler.run().await });

    cmd_tx
        .send(HandlerCommand::Subscribe(subscription(
            dbn::Schema::Ohlcv1S,
        )))
        .unwrap();
    cmd_tx.send(HandlerCommand::Start).unwrap();

    let msg = recv_msg(&mut msg_rx).await;
    match msg {
        DatabentoMessage::Data(Data::Bar(bar)) => {
            assert!(bar.high.as_f64() >= bar.low.as_f64());
            assert!(bar.high.as_f64() >= bar.open.as_f64());
            assert!(bar.high.as_f64() >= bar.close.as_f64());
            assert!(bar.volume.as_f64() > 0.0);
        }
        other => panic!("expected Data::Bar, was {other:?}"),
    }

    cmd_tx.send(HandlerCommand::Close).unwrap();
    let _ = handle.await;
    server.stop().await;
}

#[rstest]
#[tokio::test]
async fn test_subscribe_instrument_status() {
    let server = MockLsgServer::new(TEST_DATASET).await;
    let (cmd_tx, mut msg_rx, mut handler) = create_test_handler(&server.addr(), TEST_DATASET);

    server.authenticate();
    server.expect_subscription();
    server.start();
    server.send_record(symbol_mapping_msg(INSTRUMENT_ID, RAW_SYMBOL));
    server.send_record(status_msg(INSTRUMENT_ID));
    server.disconnect();

    let handle = tokio::spawn(async move { handler.run().await });

    cmd_tx
        .send(HandlerCommand::Subscribe(subscription(dbn::Schema::Status)))
        .unwrap();
    cmd_tx.send(HandlerCommand::Start).unwrap();

    let msg = recv_msg(&mut msg_rx).await;
    match msg {
        DatabentoMessage::Status(status) => {
            assert_eq!(status.instrument_id.symbol.as_str(), RAW_SYMBOL);
        }
        other => panic!("expected DatabentoMessage::Status, was {other:?}"),
    }

    cmd_tx.send(HandlerCommand::Close).unwrap();
    let _ = handle.await;
    server.stop().await;
}

#[rstest]
#[tokio::test]
async fn test_subscribe_book_depth_mbp10() {
    let server = MockLsgServer::new(TEST_DATASET).await;
    let (cmd_tx, mut msg_rx, mut handler) = create_test_handler(&server.addr(), TEST_DATASET);

    server.authenticate();
    server.expect_subscription();
    server.start();
    server.send_record(symbol_mapping_msg(INSTRUMENT_ID, RAW_SYMBOL));
    server.send_record(mbp10_msg(INSTRUMENT_ID));
    server.disconnect();

    let handle = tokio::spawn(async move { handler.run().await });

    cmd_tx
        .send(HandlerCommand::Subscribe(subscription(dbn::Schema::Mbp10)))
        .unwrap();
    cmd_tx.send(HandlerCommand::Start).unwrap();

    let msg = recv_msg(&mut msg_rx).await;
    match msg {
        DatabentoMessage::Data(Data::BookDepth(depth)) => {
            assert_eq!(depth.instrument_id.symbol.as_str(), RAW_SYMBOL);
            // Bids descending, asks ascending
            for i in 0..9 {
                assert!(
                    depth.bids[i].price >= depth.bids[i + 1].price,
                    "bids should descend: {} < {}",
                    depth.bids[i].price,
                    depth.bids[i + 1].price,
                );
                assert!(
                    depth.asks[i].price <= depth.asks[i + 1].price,
                    "asks should ascend: {} > {}",
                    depth.asks[i].price,
                    depth.asks[i + 1].price,
                );
            }
        }
        other => panic!("expected Data::BookDepth, was {other:?}"),
    }

    cmd_tx.send(HandlerCommand::Close).unwrap();
    let _ = handle.await;
    server.stop().await;
}

#[rstest]
#[tokio::test]
async fn test_subscribe_book_deltas_mbo() {
    let server = MockLsgServer::new(TEST_DATASET).await;
    let (cmd_tx, mut msg_rx, mut handler) = create_test_handler(&server.addr(), TEST_DATASET);

    server.authenticate();
    server.expect_subscription();
    server.start();
    server.send_record(symbol_mapping_msg(INSTRUMENT_ID, RAW_SYMBOL));

    // F_LAST (0x80) | F_SNAPSHOT (0x20) = snapshot that completes the book image
    server.send_record(mbo_msg(
        INSTRUMENT_ID,
        b'A',
        b'B',
        128 | 32,
        100_000_000_000,
    ));

    // F_LAST only: real-time delta that triggers emission of buffered deltas
    server.send_record(mbo_msg(INSTRUMENT_ID, b'A', b'A', 128, 101_000_000_000));

    server.disconnect();

    let handle = tokio::spawn(async move { handler.run().await });

    cmd_tx
        .send(HandlerCommand::Subscribe(subscription(dbn::Schema::Mbo)))
        .unwrap();
    cmd_tx.send(HandlerCommand::Start).unwrap();

    let msg = recv_msg(&mut msg_rx).await;
    match msg {
        DatabentoMessage::Data(Data::BookDeltas(deltas)) => {
            assert!(!deltas.deltas.is_empty(), "expected at least one delta");
        }
        other => panic!("expected Data::BookDeltas, was {other:?}"),
    }

    cmd_tx.send(HandlerCommand::Close).unwrap();
    let _ = handle.await;
    server.stop().await;
}

#[rstest]
#[tokio::test]
async fn test_mbo_single_f_last_emits_one_delta() {
    let server = MockLsgServer::new(TEST_DATASET).await;
    let (cmd_tx, mut msg_rx, mut handler) = create_test_handler(&server.addr(), TEST_DATASET);

    server.authenticate();
    server.expect_subscription();
    server.start();
    server.send_record(symbol_mapping_msg(INSTRUMENT_ID, RAW_SYMBOL));
    server.send_record(mbo_msg_with_ts(
        INSTRUMENT_ID,
        b'A',
        b'B',
        128,
        101_000_000_000,
        101_000_000_000,
    ));
    server.disconnect();

    let handle = tokio::spawn(async move { handler.run().await });

    cmd_tx
        .send(HandlerCommand::Subscribe(subscription(dbn::Schema::Mbo)))
        .unwrap();
    cmd_tx.send(HandlerCommand::Start).unwrap();

    let msg = recv_msg(&mut msg_rx).await;
    match msg {
        DatabentoMessage::Data(Data::BookDeltas(deltas)) => {
            assert_eq!(deltas.instrument_id.symbol.as_str(), RAW_SYMBOL);
            assert_eq!(deltas.deltas.len(), 1);
            assert_eq!(deltas.flags, 128);
            assert_eq!(deltas.sequence, 1);
            assert_eq!(deltas.ts_event, 101_000_000_000u64);
            assert_eq!(deltas.deltas[0].flags, 128);
            assert_eq!(deltas.deltas[0].sequence, 1);
            assert_eq!(deltas.deltas[0].ts_event, 101_000_000_000u64);
        }
        other => panic!("expected Data::BookDeltas, was {other:?}"),
    }

    cmd_tx.send(HandlerCommand::Close).unwrap();
    let _ = handle.await;
    server.stop().await;
}

#[rstest]
#[tokio::test]
async fn test_mbo_rejected_trade_flushes_event_boundary() {
    let server = MockLsgServer::new(TEST_DATASET).await;
    let (cmd_tx, mut msg_rx, mut handler) = create_test_handler(&server.addr(), TEST_DATASET);

    server.authenticate();
    server.expect_subscription();
    server.start();
    server.send_record(mbo_msg(INSTRUMENT_ID + 1, b'A', b'A', 128, 99_000_000_000));
    server.send_record(symbol_mapping_msg(INSTRUMENT_ID, RAW_SYMBOL));
    for (action, flags, price, ts) in [
        (b'A', 128, 100_000_000_000, 1_000_000_000),
        (b'M', 0, 100_250_000_000, 2_000_000_000),
        (b'T', 128, 100_125_000_000, 3_000_000_000),
        (b'A', 128, 101_000_000_000, 4_000_000_000),
    ] {
        server.send_record(mbo_msg_with_ts(
            INSTRUMENT_ID,
            action,
            b'A',
            flags,
            price,
            ts,
        ));
    }

    server.disconnect();

    let handle = tokio::spawn(async move { handler.run().await });
    cmd_tx
        .send(HandlerCommand::Subscribe(subscription(dbn::Schema::Mbo)))
        .unwrap();
    cmd_tx.send(HandlerCommand::Start).unwrap();

    for (price, action, flags, ts) in [
        ("100.00", BookAction::Add, 128, 1_000_000_000u64),
        ("100.25", BookAction::Update, 0, 2_000_000_000),
        ("101.00", BookAction::Add, 128, 4_000_000_000),
    ] {
        let DatabentoMessage::Data(Data::BookDeltas(deltas)) = recv_msg(&mut msg_rx).await else {
            panic!("expected separate book events without the rejected trade");
        };

        assert_eq!(deltas.instrument_id, InstrumentId::from("ESM4.GLBX"));
        assert_eq!(deltas.deltas.len(), 1);
        assert_eq!(deltas.flags, flags);
        assert_eq!(deltas.sequence, 1);
        assert_eq!(deltas.ts_event, ts);
        assert_eq!(deltas.ts_init, deltas.deltas[0].ts_init);
        let delta = &deltas.deltas[0];
        assert_eq!(delta.instrument_id, deltas.instrument_id);
        assert_eq!(delta.action, action);
        assert_eq!(delta.order.order_id, 1);
        assert_eq!(delta.order.side, Some(OrderSide::Sell));
        assert_eq!(delta.order.price, Price::from(price));
        assert_eq!(delta.order.price.precision, 2);
        assert_eq!(delta.order.size, Quantity::from("10"));
        assert_eq!(delta.order.size.precision, 0);
        assert_eq!(delta.flags, flags);
        assert_eq!(delta.sequence, 1);
        assert_eq!(delta.ts_event, ts);
    }

    cmd_tx.send(HandlerCommand::Close).unwrap();
    handle.await.unwrap().unwrap();
    server.stop().await;
}

#[rstest]
#[tokio::test]
async fn test_close_during_backoff() {
    let server = MockLsgServer::new(TEST_DATASET).await;
    let (cmd_tx, _msg_rx, mut handler) = create_test_handler(&server.addr(), TEST_DATASET);

    server.authenticate_reject("test rejection");

    let handle = tokio::spawn(async move { handler.run().await });

    cmd_tx
        .send(HandlerCommand::Subscribe(subscription(dbn::Schema::Trades)))
        .unwrap();
    cmd_tx.send(HandlerCommand::Start).unwrap();

    tokio::time::sleep(Duration::from_millis(500)).await;
    cmd_tx.send(HandlerCommand::Close).unwrap();

    let result = handle.await.unwrap();
    assert!(
        result.is_ok(),
        "handler should exit cleanly on close during backoff"
    );

    server.stop().await;
}

#[rstest]
#[tokio::test]
async fn test_reconnection_resubscribes() {
    let server = MockLsgServer::new(TEST_DATASET).await;
    let (cmd_tx, mut msg_rx, mut handler) = create_test_handler(&server.addr(), TEST_DATASET);

    server.authenticate();
    server.expect_subscription();
    server.start();
    server.send_record(symbol_mapping_msg(INSTRUMENT_ID, RAW_SYMBOL));
    server.send_record(trade_msg(INSTRUMENT_ID, 100_000_000_000, 50));
    server.disconnect();

    // Second session after reconnect
    server.authenticate();
    server.expect_subscription();
    server.start();
    server.send_record(symbol_mapping_msg(INSTRUMENT_ID, RAW_SYMBOL));
    server.send_record(trade_msg(INSTRUMENT_ID, 101_000_000_000, 25));
    server.disconnect();

    let handle = tokio::spawn(async move { handler.run().await });

    cmd_tx
        .send(HandlerCommand::Subscribe(subscription(dbn::Schema::Trades)))
        .unwrap();
    cmd_tx.send(HandlerCommand::Start).unwrap();

    let msg1 = recv_msg(&mut msg_rx).await;
    assert!(matches!(msg1, DatabentoMessage::Data(Data::Trade(_))));

    let msg2 = recv_msg(&mut msg_rx).await;
    assert!(matches!(msg2, DatabentoMessage::Data(Data::Trade(_))));

    cmd_tx.send(HandlerCommand::Close).unwrap();
    let _ = handle.await;
    server.stop().await;
}

#[rstest]
#[tokio::test]
async fn test_system_msg_subscription_ack() {
    let server = MockLsgServer::new(TEST_DATASET).await;
    let (cmd_tx, mut msg_rx, mut handler) = create_test_handler(&server.addr(), TEST_DATASET);

    server.authenticate();
    server.expect_subscription();
    server.start();
    server.send_record(system_msg(
        "Subscription request 1 for trades data succeeded",
        1,
    ));
    server.disconnect();

    let handle = tokio::spawn(async move { handler.run().await });

    cmd_tx
        .send(HandlerCommand::Subscribe(subscription(dbn::Schema::Trades)))
        .unwrap();
    cmd_tx.send(HandlerCommand::Start).unwrap();

    let msg = recv_msg(&mut msg_rx).await;
    match msg {
        DatabentoMessage::SubscriptionAck(ack) => {
            assert!(
                ack.message.contains("succeeded"),
                "ack message should contain 'succeeded', was: {}",
                ack.message
            );
        }
        other => panic!("expected SubscriptionAck, was {other:?}"),
    }

    cmd_tx.send(HandlerCommand::Close).unwrap();
    let _ = handle.await;
    server.stop().await;
}

#[rstest]
#[tokio::test]
async fn test_subscribe_instrument_def() {
    let server = MockLsgServer::new(TEST_DATASET).await;
    let (cmd_tx, mut msg_rx, mut handler) = create_test_handler(&server.addr(), TEST_DATASET);

    server.authenticate();
    server.expect_subscription();
    server.start();
    server.send_record(symbol_mapping_msg(INSTRUMENT_ID, RAW_SYMBOL));
    server.send_record(instrument_def_msg(INSTRUMENT_ID, b'F'));
    server.disconnect();

    let handle = tokio::spawn(async move { handler.run().await });

    cmd_tx
        .send(HandlerCommand::Subscribe(subscription(
            dbn::Schema::Definition,
        )))
        .unwrap();
    cmd_tx.send(HandlerCommand::Start).unwrap();

    let msg = recv_msg(&mut msg_rx).await;
    match msg {
        DatabentoMessage::Instrument(instrument) => {
            assert_eq!(instrument.id().symbol.as_str(), RAW_SYMBOL);
        }
        other => panic!("expected DatabentoMessage::Instrument, was {other:?}"),
    }

    cmd_tx.send(HandlerCommand::Close).unwrap();
    let _ = handle.await;
    server.stop().await;
}

#[rstest]
#[tokio::test]
async fn test_subscribe_imbalance() {
    let server = MockLsgServer::new(TEST_DATASET).await;
    let (cmd_tx, mut msg_rx, mut handler) = create_test_handler(&server.addr(), TEST_DATASET);

    server.authenticate();
    server.expect_subscription();
    server.start();
    server.send_record(symbol_mapping_msg(INSTRUMENT_ID, RAW_SYMBOL));

    // InstrumentDefMsg populates the price_precision_map
    server.send_record(instrument_def_msg(INSTRUMENT_ID, b'K'));
    server.send_record(imbalance_msg(INSTRUMENT_ID));
    server.disconnect();

    let handle = tokio::spawn(async move { handler.run().await });

    cmd_tx
        .send(HandlerCommand::Subscribe(subscription(
            dbn::Schema::Imbalance,
        )))
        .unwrap();
    cmd_tx.send(HandlerCommand::Start).unwrap();

    // Skip the instrument message
    let _instrument = recv_msg(&mut msg_rx).await;

    let msg = recv_msg(&mut msg_rx).await;
    match msg {
        DatabentoMessage::Imbalance(imbalance) => {
            assert_eq!(imbalance.instrument_id.symbol.as_str(), RAW_SYMBOL);
        }
        other => panic!("expected DatabentoMessage::Imbalance, was {other:?}"),
    }

    cmd_tx.send(HandlerCommand::Close).unwrap();
    let _ = handle.await;
    server.stop().await;
}

#[rstest]
#[tokio::test]
async fn test_subscribe_imbalance_uses_symbol_precision_fallback() {
    let server = MockLsgServer::new(TEST_DATASET).await;
    let (cmd_tx, mut msg_rx, mut handler) = create_test_handler(&server.addr(), TEST_DATASET);

    server.authenticate();
    server.expect_subscription();
    server.start();
    server.send_record(symbol_mapping_msg(INSTRUMENT_ID, RAW_SYMBOL));
    server.send_record(imbalance_msg(INSTRUMENT_ID));
    server.disconnect();

    let handle = tokio::spawn(async move { handler.run().await });

    cmd_tx
        .send(HandlerCommand::SetPricePrecision(
            Symbol::from(RAW_SYMBOL),
            5,
        ))
        .unwrap();
    cmd_tx
        .send(HandlerCommand::Subscribe(subscription(
            dbn::Schema::Imbalance,
        )))
        .unwrap();
    cmd_tx.send(HandlerCommand::Start).unwrap();

    let msg = recv_msg(&mut msg_rx).await;
    match msg {
        DatabentoMessage::Imbalance(imbalance) => {
            assert_eq!(imbalance.instrument_id.symbol.as_str(), RAW_SYMBOL);
            assert_eq!(imbalance.ref_price, Price::from("100.00000"));
            assert_eq!(imbalance.ref_price.precision, 5);
            assert_eq!(imbalance.cont_book_clr_price, Price::from("100.50000"));
            assert_eq!(imbalance.cont_book_clr_price.precision, 5);
            assert_eq!(imbalance.auct_interest_clr_price, Price::from("100.25000"));
            assert_eq!(imbalance.auct_interest_clr_price.precision, 5);
        }
        other => panic!("expected DatabentoMessage::Imbalance, was {other:?}"),
    }

    cmd_tx.send(HandlerCommand::Close).unwrap();
    let _ = handle.await;
    server.stop().await;
}

#[rstest]
#[tokio::test]
async fn test_imbalance_rejects_insufficient_precision_without_reconnecting() {
    let server = MockLsgServer::new(TEST_DATASET).await;
    let (cmd_tx, mut msg_rx, mut handler) = create_test_handler(&server.addr(), TEST_DATASET);
    let mut invalid = imbalance_msg(INSTRUMENT_ID);
    invalid.ref_price = 876_500_000;

    server.authenticate();
    server.expect_subscription();
    server.start();
    server.send_record(symbol_mapping_msg(INSTRUMENT_ID, RAW_SYMBOL));
    server.send_record(invalid);
    server.send_record(imbalance_msg(INSTRUMENT_ID));
    server.disconnect();

    let handle = tokio::spawn(async move { handler.run().await });
    cmd_tx
        .send(HandlerCommand::Subscribe(subscription(
            dbn::Schema::Imbalance,
        )))
        .unwrap();
    cmd_tx.send(HandlerCommand::Start).unwrap();

    let DatabentoMessage::Imbalance(imbalance) = recv_msg(&mut msg_rx).await else {
        panic!("expected the valid imbalance from the same session");
    };

    cmd_tx.send(HandlerCommand::Close).unwrap();
    handle.await.unwrap().unwrap();
    server.stop().await;

    assert_eq!(imbalance.instrument_id.symbol.as_str(), RAW_SYMBOL);
    assert_eq!(imbalance.ref_price, Price::from("100.00"));
    assert_eq!(imbalance.ref_price.precision, 2);
    assert_eq!(imbalance.cont_book_clr_price, Price::from("100.50"));
    assert_eq!(imbalance.auct_interest_clr_price, Price::from("100.25"));
    assert_eq!(imbalance.paired_qty, Quantity::from("1000"));
    assert_eq!(imbalance.total_imbalance_qty, Quantity::from("500"));
}

#[rstest]
#[tokio::test]
async fn test_subscribe_statistics() {
    let server = MockLsgServer::new(TEST_DATASET).await;
    let (cmd_tx, mut msg_rx, mut handler) = create_test_handler(&server.addr(), TEST_DATASET);

    server.authenticate();
    server.expect_subscription();
    server.start();
    server.send_record(symbol_mapping_msg(INSTRUMENT_ID, RAW_SYMBOL));

    // InstrumentDefMsg populates the price_precision_map
    server.send_record(instrument_def_msg(INSTRUMENT_ID, b'F'));
    server.send_record(statistics_msg(INSTRUMENT_ID));
    server.disconnect();

    let handle = tokio::spawn(async move { handler.run().await });

    cmd_tx
        .send(HandlerCommand::Subscribe(subscription(
            dbn::Schema::Statistics,
        )))
        .unwrap();
    cmd_tx.send(HandlerCommand::Start).unwrap();

    // Skip the instrument message
    let _instrument = recv_msg(&mut msg_rx).await;

    let msg = recv_msg(&mut msg_rx).await;
    match msg {
        DatabentoMessage::Statistics(stats) => {
            assert_eq!(stats.instrument_id.symbol.as_str(), RAW_SYMBOL);
        }
        other => panic!("expected DatabentoMessage::Statistics, was {other:?}"),
    }

    cmd_tx.send(HandlerCommand::Close).unwrap();
    let _ = handle.await;
    server.stop().await;
}

#[rstest]
#[tokio::test]
async fn test_subscribe_statistics_uses_wire_precision() {
    let server = MockLsgServer::new(TEST_DATASET).await;
    let (cmd_tx, mut msg_rx, mut handler) = create_test_handler(&server.addr(), TEST_DATASET);

    server.authenticate();
    server.expect_subscription();
    server.start();
    server.send_record(symbol_mapping_msg(INSTRUMENT_ID, RAW_SYMBOL));
    server.send_record(statistics_msg(INSTRUMENT_ID));
    server.disconnect();

    let handle = tokio::spawn(async move { handler.run().await });

    cmd_tx
        .send(HandlerCommand::SetPricePrecision(
            Symbol::from(RAW_SYMBOL),
            5,
        ))
        .unwrap();
    cmd_tx
        .send(HandlerCommand::Subscribe(subscription(
            dbn::Schema::Statistics,
        )))
        .unwrap();
    cmd_tx.send(HandlerCommand::Start).unwrap();

    let msg = recv_msg(&mut msg_rx).await;
    match msg {
        DatabentoMessage::Statistics(stats) => {
            let price = stats.price.expect("expected statistics price");
            assert_eq!(stats.instrument_id.symbol.as_str(), RAW_SYMBOL);
            assert_eq!(price, Price::from("100.000000000"));
            assert_eq!(price.precision, 9);
        }
        other => panic!("expected DatabentoMessage::Statistics, was {other:?}"),
    }

    cmd_tx.send(HandlerCommand::Close).unwrap();
    let _ = handle.await;
    server.stop().await;
}

#[rstest]
#[tokio::test]
async fn test_error_msg_continues_processing() {
    let server = MockLsgServer::new(TEST_DATASET).await;
    let (cmd_tx, mut msg_rx, mut handler) = create_test_handler(&server.addr(), TEST_DATASET);

    server.authenticate();
    server.expect_subscription();
    server.start();
    server.send_record(symbol_mapping_msg(INSTRUMENT_ID, RAW_SYMBOL));
    server.send_record(error_msg("test error from gateway"));
    server.send_record(trade_msg(INSTRUMENT_ID, 100_000_000_000, 50));
    server.disconnect();

    let handle = tokio::spawn(async move { handler.run().await });

    cmd_tx
        .send(HandlerCommand::Subscribe(subscription(dbn::Schema::Trades)))
        .unwrap();
    cmd_tx.send(HandlerCommand::Start).unwrap();

    // ErrorMsg is logged but does not stop processing; trade should arrive
    let msg = recv_msg(&mut msg_rx).await;
    assert!(
        matches!(msg, DatabentoMessage::Data(Data::Trade(_))),
        "expected trade after error, was {msg:?}"
    );

    cmd_tx.send(HandlerCommand::Close).unwrap();
    let _ = handle.await;
    server.stop().await;
}

#[rstest]
#[tokio::test]
async fn test_mbo_buffering_waits_for_f_last() {
    let server = MockLsgServer::new(TEST_DATASET).await;
    let (cmd_tx, mut msg_rx, mut handler) = create_test_handler(&server.addr(), TEST_DATASET);

    server.authenticate();
    server.expect_subscription();
    server.start();
    server.send_record(symbol_mapping_msg(INSTRUMENT_ID, RAW_SYMBOL));

    // MBO without F_LAST: should buffer, not emit
    server.send_record(mbo_msg(INSTRUMENT_ID, b'A', b'B', 0, 100_000_000_000));
    server.send_record(mbo_msg(INSTRUMENT_ID, b'A', b'B', 0, 99_000_000_000));

    server.send_record(mbo_msg(INSTRUMENT_ID, b'A', b'A', 128, 101_000_000_000));
    server.disconnect();

    let handle = tokio::spawn(async move { handler.run().await });

    cmd_tx
        .send(HandlerCommand::Subscribe(subscription(dbn::Schema::Mbo)))
        .unwrap();
    cmd_tx.send(HandlerCommand::Start).unwrap();

    let msg = recv_msg(&mut msg_rx).await;
    match msg {
        DatabentoMessage::Data(Data::BookDeltas(deltas)) => {
            assert_eq!(
                deltas.deltas.len(),
                3,
                "expected 3 buffered deltas, was {}",
                deltas.deltas.len()
            );
        }
        other => panic!("expected Data::BookDeltas, was {other:?}"),
    }

    cmd_tx.send(HandlerCommand::Close).unwrap();
    let _ = handle.await;
    server.stop().await;
}

#[rstest]
#[tokio::test]
async fn test_mbo_trade_emitted_once_book_initialized() {
    let server = MockLsgServer::new(TEST_DATASET).await;
    let (cmd_tx, mut msg_rx, mut handler) = create_test_handler(&server.addr(), TEST_DATASET);

    server.authenticate();
    server.expect_subscription();
    server.start();
    server.send_record(symbol_mapping_msg(INSTRUMENT_ID, RAW_SYMBOL));

    // Dropped: no delta has initialized the book yet
    server.send_record(mbo_msg_with_ts(
        INSTRUMENT_ID,
        b'T',
        b'A',
        128,
        98_000_000_000,
        100_000_000_000,
    ));

    // Initializes the book and stays buffered until the trade's F_LAST closes the event
    server.send_record(mbo_msg_with_ts(
        INSTRUMENT_ID,
        b'A',
        b'B',
        0,
        100_000_000_000,
        101_000_000_000,
    ));
    server.send_record(mbo_msg_with_ts(
        INSTRUMENT_ID,
        b'T',
        b'A',
        128,
        99_000_000_000,
        102_000_000_000,
    ));

    // Must arrive directly after the trade, proving the trade is sent once
    server.send_record(mbo_msg_with_ts(
        INSTRUMENT_ID,
        b'C',
        b'B',
        128,
        100_000_000_000,
        103_000_000_000,
    ));
    server.disconnect();

    let handle = tokio::spawn(async move { handler.run().await });

    cmd_tx
        .send(HandlerCommand::Subscribe(subscription(dbn::Schema::Mbo)))
        .unwrap();
    cmd_tx.send(HandlerCommand::Start).unwrap();

    let msg = recv_msg(&mut msg_rx).await;
    match msg {
        DatabentoMessage::Data(Data::BookDeltas(deltas)) => {
            assert_eq!(deltas.deltas.len(), 1);
            assert_eq!(deltas.deltas[0].action, BookAction::Add);
            assert_eq!(deltas.deltas[0].order.price, Price::from("100.00"));
            assert_eq!(deltas.deltas[0].ts_event, 101_000_000_000u64);
        }
        other => panic!("expected Data::BookDeltas, was {other:?}"),
    }

    let msg = recv_msg(&mut msg_rx).await;
    match msg {
        DatabentoMessage::Data(Data::Trade(trade)) => {
            assert_eq!(trade.instrument_id.symbol.as_str(), RAW_SYMBOL);
            assert_eq!(trade.price, Price::from("99.00"));
            assert_eq!(trade.size, Quantity::from(10));
            assert_eq!(trade.aggressor_side, AggressorSide::Sell);
            assert_eq!(trade.trade_id, TradeId::new("1"));
            assert_eq!(trade.ts_event, 102_000_000_000u64);
        }
        other => panic!("expected Data::Trade, was {other:?}"),
    }

    let msg = recv_msg(&mut msg_rx).await;
    match msg {
        DatabentoMessage::Data(Data::BookDeltas(deltas)) => {
            assert_eq!(deltas.deltas.len(), 1);
            assert_eq!(deltas.deltas[0].action, BookAction::Delete);
            assert_eq!(deltas.deltas[0].ts_event, 103_000_000_000u64);
        }
        other => panic!("expected Data::BookDeltas, was {other:?}"),
    }

    cmd_tx.send(HandlerCommand::Close).unwrap();
    let _ = handle.await;
    server.stop().await;
}

#[rstest]
#[tokio::test]
async fn test_mbo_and_trades_send_each_trade_once() {
    let server = MockLsgServer::new(TEST_DATASET).await;
    let (cmd_tx, mut msg_rx, mut handler) = create_test_handler(&server.addr(), TEST_DATASET);

    server.authenticate();
    server.expect_subscription();
    server.expect_subscription();
    server.start();
    server.send_record(symbol_mapping_msg(INSTRUMENT_ID, RAW_SYMBOL));
    server.send_record(mbo_msg_with_ts(
        INSTRUMENT_ID,
        b'A',
        b'B',
        128,
        100_000_000_000,
        101_000_000_000,
    ));

    // Delivers the instrument's first trade, so `mbo` becomes its trade source
    server.send_record(mbo_msg_with_ts(
        INSTRUMENT_ID,
        b'T',
        b'A',
        128,
        99_000_000_000,
        102_000_000_000,
    ));

    // The same trade on `trades` is dropped
    server.send_record(trade_msg(INSTRUMENT_ID, 99_000_000_000, 10));

    // Must arrive directly after the `mbo` trade, proving the `trades` copy was dropped
    server.send_record(mbo_msg_with_ts(
        INSTRUMENT_ID,
        b'C',
        b'B',
        128,
        100_000_000_000,
        103_000_000_000,
    ));
    server.disconnect();

    let handle = tokio::spawn(async move { handler.run().await });

    cmd_tx
        .send(HandlerCommand::Subscribe(subscription(dbn::Schema::Mbo)))
        .unwrap();
    cmd_tx
        .send(HandlerCommand::Subscribe(subscription(dbn::Schema::Trades)))
        .unwrap();
    cmd_tx.send(HandlerCommand::Start).unwrap();

    let msg = recv_msg(&mut msg_rx).await;
    match msg {
        DatabentoMessage::Data(Data::BookDeltas(deltas)) => {
            assert_eq!(deltas.deltas[0].action, BookAction::Add);
        }
        other => panic!("expected Data::BookDeltas, was {other:?}"),
    }

    let msg = recv_msg(&mut msg_rx).await;
    match msg {
        DatabentoMessage::Data(Data::Trade(trade)) => {
            assert_eq!(trade.price, Price::from("99.00"));
            assert_eq!(trade.ts_event, 102_000_000_000u64);
        }
        other => panic!("expected Data::Trade, was {other:?}"),
    }

    let msg = recv_msg(&mut msg_rx).await;
    match msg {
        DatabentoMessage::Data(Data::BookDeltas(deltas)) => {
            assert_eq!(deltas.deltas[0].action, BookAction::Delete);
        }
        other => panic!("expected Data::BookDeltas, was {other:?}"),
    }

    cmd_tx.send(HandlerCommand::Close).unwrap();
    let _ = handle.await;
    server.stop().await;
}

#[rstest]
#[tokio::test]
async fn test_dropped_mbo_trade_still_flushes_its_event() {
    let server = MockLsgServer::new(TEST_DATASET).await;
    let (cmd_tx, mut msg_rx, mut handler) = create_test_handler(&server.addr(), TEST_DATASET);

    server.authenticate();
    server.expect_subscription();
    server.expect_subscription();
    server.start();
    server.send_record(symbol_mapping_msg(INSTRUMENT_ID, RAW_SYMBOL));

    // Delivers the instrument's first trade, so `trades` becomes its trade source
    server.send_record(trade_msg(INSTRUMENT_ID, 99_000_000_000, 10));

    // A non-terminal delta, then the event's trade carrying F_LAST: the trade is
    // dropped but its F_LAST still flushes the buffered delta
    server.send_record(mbo_msg_with_ts(
        INSTRUMENT_ID,
        b'C',
        b'B',
        0,
        100_000_000_000,
        101_000_000_000,
    ));
    server.send_record(mbo_msg_with_ts(
        INSTRUMENT_ID,
        b'T',
        b'A',
        128,
        99_000_000_000,
        102_000_000_000,
    ));

    // Must arrive directly after the flushed delta, proving the `mbo` trade was dropped
    server.send_record(trade_msg(INSTRUMENT_ID, 99_000_000_000, 60));
    server.disconnect();

    let handle = tokio::spawn(async move { handler.run().await });

    cmd_tx
        .send(HandlerCommand::Subscribe(subscription(dbn::Schema::Trades)))
        .unwrap();
    cmd_tx
        .send(HandlerCommand::Subscribe(subscription(dbn::Schema::Mbo)))
        .unwrap();
    cmd_tx.send(HandlerCommand::Start).unwrap();

    let msg = recv_msg(&mut msg_rx).await;
    match msg {
        DatabentoMessage::Data(Data::Trade(trade)) => {
            assert_eq!(trade.size, Quantity::from(10));
        }
        other => panic!("expected Data::Trade, was {other:?}"),
    }

    let msg = recv_msg(&mut msg_rx).await;
    match msg {
        DatabentoMessage::Data(Data::BookDeltas(deltas)) => {
            assert_eq!(deltas.deltas.len(), 1);
            assert_eq!(deltas.deltas[0].action, BookAction::Delete);
            assert_eq!(deltas.deltas[0].ts_event, 101_000_000_000u64);
        }
        other => panic!("expected Data::BookDeltas, was {other:?}"),
    }

    let msg = recv_msg(&mut msg_rx).await;
    match msg {
        DatabentoMessage::Data(Data::Trade(trade)) => {
            assert_eq!(trade.size, Quantity::from(60));
        }
        other => panic!("expected Data::Trade, was {other:?}"),
    }

    cmd_tx.send(HandlerCommand::Close).unwrap();
    let _ = handle.await;
    server.stop().await;
}

#[rstest]
#[tokio::test]
async fn test_mbo_snapshot_buffered_until_delta() {
    let server = MockLsgServer::new(TEST_DATASET).await;
    let (cmd_tx, mut msg_rx, mut handler) = create_test_handler(&server.addr(), TEST_DATASET);

    server.authenticate();
    server.expect_subscription();
    server.start();
    server.send_record(symbol_mapping_msg(INSTRUMENT_ID, RAW_SYMBOL));

    // F_SNAPSHOT | F_LAST: buffered, not emitted
    server.send_record(mbo_msg(
        INSTRUMENT_ID,
        b'A',
        b'B',
        128 | 32,
        100_000_000_000,
    ));
    server.send_record(mbo_msg(
        INSTRUMENT_ID,
        b'A',
        b'A',
        128 | 32,
        101_000_000_000,
    ));

    // Non-snapshot F_LAST triggers emission of all buffered
    server.send_record(mbo_msg(INSTRUMENT_ID, b'A', b'B', 128, 99_500_000_000));
    server.disconnect();

    let handle = tokio::spawn(async move { handler.run().await });

    cmd_tx
        .send(HandlerCommand::Subscribe(subscription(dbn::Schema::Mbo)))
        .unwrap();
    cmd_tx.send(HandlerCommand::Start).unwrap();

    let msg = recv_msg(&mut msg_rx).await;
    match msg {
        DatabentoMessage::Data(Data::BookDeltas(deltas)) => {
            assert_eq!(
                deltas.deltas.len(),
                3,
                "expected 3 deltas (2 snapshot + 1 delta), was {}",
                deltas.deltas.len()
            );
        }
        other => panic!("expected Data::BookDeltas, was {other:?}"),
    }

    cmd_tx.send(HandlerCommand::Close).unwrap();
    let _ = handle.await;
    server.stop().await;
}

#[rstest]
#[tokio::test]
async fn test_mbo_multiple_instruments() {
    let server = MockLsgServer::new(TEST_DATASET).await;
    let (cmd_tx, mut msg_rx, mut handler) = create_test_handler(&server.addr(), TEST_DATASET);

    let instrument_a = 1u32;
    let instrument_b = 2u32;

    server.authenticate();
    server.expect_subscription();
    server.start();
    server.send_record(symbol_mapping_msg(instrument_a, "ESM4"));
    server.send_record(symbol_mapping_msg(instrument_b, "NQM4"));

    // Interleave deltas for two instruments
    server.send_record(mbo_msg(instrument_a, b'A', b'B', 128 | 32, 100_000_000_000));
    server.send_record(mbo_msg(instrument_b, b'A', b'B', 128 | 32, 200_000_000_000));
    server.send_record(mbo_msg(instrument_a, b'A', b'A', 128, 101_000_000_000));
    server.send_record(mbo_msg(instrument_b, b'A', b'A', 128, 201_000_000_000));
    server.disconnect();

    let handle = tokio::spawn(async move { handler.run().await });

    cmd_tx
        .send(HandlerCommand::Subscribe(subscription(dbn::Schema::Mbo)))
        .unwrap();
    cmd_tx.send(HandlerCommand::Start).unwrap();

    let msg1 = recv_msg(&mut msg_rx).await;
    let msg2 = recv_msg(&mut msg_rx).await;

    let mut symbols: Vec<String> = Vec::new();

    for msg in [msg1, msg2] {
        match msg {
            DatabentoMessage::Data(Data::BookDeltas(deltas)) => {
                symbols.push(deltas.instrument_id.symbol.to_string());
            }
            other => panic!("expected Data::BookDeltas, was {other:?}"),
        }
    }

    symbols.sort();
    assert_eq!(symbols, vec!["ESM4", "NQM4"]);

    cmd_tx.send(HandlerCommand::Close).unwrap();
    let _ = handle.await;
    server.stop().await;
}

#[rstest]
#[tokio::test]
async fn test_reconnect_timeout_gives_up() {
    let server = MockLsgServer::new(TEST_DATASET).await;
    let (cmd_tx, _msg_rx, mut handler) = create_test_handler_with_config(
        &server.addr(),
        TEST_DATASET,
        &TestHandlerConfig {
            reconnect_timeout_mins: Some(0),
            ..Default::default()
        },
    );

    server.authenticate_reject("unauthorized");

    let handle = tokio::spawn(async move { handler.run().await });

    cmd_tx
        .send(HandlerCommand::Subscribe(subscription(dbn::Schema::Trades)))
        .unwrap();
    cmd_tx.send(HandlerCommand::Start).unwrap();

    let result = handle.await.unwrap();
    assert!(result.is_err(), "handler should return Err on timeout");

    server.stop().await;
}

#[rstest]
#[tokio::test]
async fn test_command_channel_disconnect_stops_handler() {
    let server = MockLsgServer::new(TEST_DATASET).await;
    let (cmd_tx, _msg_rx, mut handler) = create_test_handler(&server.addr(), TEST_DATASET);

    server.authenticate();
    server.expect_subscription();
    server.start();

    let handle = tokio::spawn(async move { handler.run().await });

    cmd_tx
        .send(HandlerCommand::Subscribe(subscription(dbn::Schema::Trades)))
        .unwrap();
    cmd_tx.send(HandlerCommand::Start).unwrap();

    tokio::time::sleep(Duration::from_millis(100)).await;
    drop(cmd_tx);

    let result = handle.await.unwrap();
    assert!(result.is_ok());

    server.stop().await;
}

#[rstest]
#[tokio::test]
async fn test_bars_timestamp_on_close_enabled() {
    let server = MockLsgServer::new(TEST_DATASET).await;
    let (cmd_tx, mut msg_rx, mut handler) = create_test_handler_with_config(
        &server.addr(),
        TEST_DATASET,
        &TestHandlerConfig {
            bars_timestamp_on_close: true,
            ..Default::default()
        },
    );

    server.authenticate();
    server.expect_subscription();
    server.start();
    server.send_record(symbol_mapping_msg(INSTRUMENT_ID, RAW_SYMBOL));
    server.send_record(ohlcv_msg(INSTRUMENT_ID));
    server.disconnect();

    let handle = tokio::spawn(async move { handler.run().await });

    cmd_tx
        .send(HandlerCommand::Subscribe(subscription(
            dbn::Schema::Ohlcv1S,
        )))
        .unwrap();
    cmd_tx.send(HandlerCommand::Start).unwrap();

    let msg = recv_msg(&mut msg_rx).await;
    match msg {
        DatabentoMessage::Data(Data::Bar(bar)) => {
            // With bars_timestamp_on_close, ts_event shifts by the bar interval (1s)
            assert!(bar.ts_event.as_u64() > 0);
        }
        other => panic!("expected Data::Bar, was {other:?}"),
    }

    cmd_tx.send(HandlerCommand::Close).unwrap();
    let _ = handle.await;
    server.stop().await;
}

#[rstest]
#[tokio::test]
async fn test_instrument_def_with_exchange_as_venue() {
    let server = MockLsgServer::new(TEST_DATASET).await;
    let (cmd_tx, mut msg_rx, mut handler) = create_test_handler_with_config(
        &server.addr(),
        TEST_DATASET,
        &TestHandlerConfig {
            use_exchange_as_venue: true,
            ..Default::default()
        },
    );

    server.authenticate();
    server.expect_subscription();
    server.start();
    server.send_record(symbol_mapping_msg(INSTRUMENT_ID, RAW_SYMBOL));
    server.send_record(instrument_def_msg(INSTRUMENT_ID, b'F'));
    server.disconnect();

    let handle = tokio::spawn(async move { handler.run().await });

    cmd_tx
        .send(HandlerCommand::Subscribe(subscription(
            dbn::Schema::Definition,
        )))
        .unwrap();
    cmd_tx.send(HandlerCommand::Start).unwrap();

    let msg = recv_msg(&mut msg_rx).await;
    match msg {
        DatabentoMessage::Instrument(instrument) => {
            // Venue from exchange field, not publisher map
            assert_eq!(instrument.id().venue.as_str(), "XCME");
        }
        other => panic!("expected DatabentoMessage::Instrument, was {other:?}"),
    }

    cmd_tx.send(HandlerCommand::Close).unwrap();
    let _ = handle.await;
    server.stop().await;
}

#[rstest]
#[tokio::test]
async fn test_replay_subscription_buffers_until_past_start() {
    let server = MockLsgServer::new(TEST_DATASET).await;
    let (cmd_tx, mut msg_rx, mut handler) = create_test_handler(&server.addr(), TEST_DATASET);

    server.authenticate();
    server.expect_subscription();
    server.start();
    server.send_record(symbol_mapping_msg(INSTRUMENT_ID, RAW_SYMBOL));

    // Snapshot deltas with old timestamps buffered during replay
    server.send_record(mbo_msg(INSTRUMENT_ID, b'A', b'B', 128 | 32, 1_000_000_000));
    server.send_record(mbo_msg(INSTRUMENT_ID, b'A', b'A', 128 | 32, 2_000_000_000));

    // Non-snapshot F_LAST with far-future ts_event to pass the buffering_start check
    let far_future_ts = 9_000_000_000_000_000_000u64; // ~2255 CE
    server.send_record(mbo_msg_with_ts(
        INSTRUMENT_ID,
        b'A',
        b'B',
        128,
        100_000_000_000,
        far_future_ts,
    ));
    server.disconnect();

    let handle = tokio::spawn(async move { handler.run().await });

    // Subscription with start enables replay mode
    let sub = Subscription::builder()
        .symbols(RAW_SYMBOL)
        .schema(dbn::Schema::Mbo)
        .stype_in(dbn::SType::RawSymbol)
        .start(time::OffsetDateTime::from_unix_timestamp(0).unwrap())
        .build();
    cmd_tx.send(HandlerCommand::Subscribe(sub)).unwrap();
    cmd_tx.send(HandlerCommand::Start).unwrap();

    let msg = recv_msg(&mut msg_rx).await;
    match msg {
        DatabentoMessage::Data(Data::BookDeltas(deltas)) => {
            assert_eq!(
                deltas.deltas.len(),
                3,
                "expected 3 deltas (2 replay + 1 live), was {}",
                deltas.deltas.len()
            );
        }
        other => panic!("expected Data::BookDeltas, was {other:?}"),
    }

    cmd_tx.send(HandlerCommand::Close).unwrap();
    let _ = handle.await;
    server.stop().await;
}

#[rstest]
#[tokio::test]
async fn test_replay_mbo_trades_dropped_until_past_start() {
    let server = MockLsgServer::new(TEST_DATASET).await;
    let (cmd_tx, mut msg_rx, mut handler) = create_test_handler(&server.addr(), TEST_DATASET);

    server.authenticate();
    server.expect_subscription();
    server.start();
    server.send_record(symbol_mapping_msg(INSTRUMENT_ID, RAW_SYMBOL));

    // Replayed: the snapshot initializes the book, and the trade precedes the session start
    server.send_record(mbo_msg(
        INSTRUMENT_ID,
        b'A',
        b'A',
        128 | 32,
        100_000_000_000,
    ));
    server.send_record(mbo_msg_with_ts(
        INSTRUMENT_ID,
        b'T',
        b'A',
        128,
        98_000_000_000,
        2_000_000_000,
    ));

    // Live: the trade lacks F_LAST, so it arrives before the Cancel's F_LAST ends the replay
    let far_future_ts = 9_000_000_000_000_000_000u64;
    server.send_record(mbo_msg_with_ts(
        INSTRUMENT_ID,
        b'T',
        b'B',
        0,
        100_000_000_000,
        far_future_ts,
    ));
    server.send_record(mbo_msg_with_ts(
        INSTRUMENT_ID,
        b'C',
        b'A',
        128,
        100_000_000_000,
        far_future_ts + 1,
    ));
    server.disconnect();

    let handle = tokio::spawn(async move { handler.run().await });

    let sub = Subscription::builder()
        .symbols(RAW_SYMBOL)
        .schema(dbn::Schema::Mbo)
        .stype_in(dbn::SType::RawSymbol)
        .start(time::OffsetDateTime::from_unix_timestamp(0).unwrap())
        .build();
    cmd_tx.send(HandlerCommand::Subscribe(sub)).unwrap();
    cmd_tx.send(HandlerCommand::Start).unwrap();

    let msg = recv_msg(&mut msg_rx).await;
    match msg {
        DatabentoMessage::Data(Data::Trade(trade)) => {
            assert_eq!(trade.price, Price::from("100.00"));
            assert_eq!(trade.aggressor_side, AggressorSide::Buy);
            assert_eq!(trade.ts_event, far_future_ts);
        }
        other => panic!("expected Data::Trade, was {other:?}"),
    }

    let msg = recv_msg(&mut msg_rx).await;
    match msg {
        DatabentoMessage::Data(Data::BookDeltas(deltas)) => {
            assert_eq!(deltas.deltas.len(), 2);
            assert_eq!(deltas.deltas[0].action, BookAction::Add);
            assert_eq!(deltas.deltas[1].action, BookAction::Delete);
            assert_eq!(deltas.deltas[1].ts_event, far_future_ts + 1);
        }
        other => panic!("expected Data::BookDeltas, was {other:?}"),
    }

    cmd_tx.send(HandlerCommand::Close).unwrap();
    let _ = handle.await;
    server.stop().await;
}

#[rstest]
#[tokio::test]
async fn test_replay_after_failed_connect_drops_replayed_trades() {
    let server = MockLsgServer::new(TEST_DATASET).await;
    let (cmd_tx, mut msg_rx, mut handler) = create_test_handler(&server.addr(), TEST_DATASET);

    // The rejected first connection buffers the replay subscription and start for the retry
    server.authenticate_reject("test rejection");
    server.authenticate();
    server.expect_subscription();
    server.start();
    server.send_record(symbol_mapping_msg(INSTRUMENT_ID, RAW_SYMBOL));

    // Replayed: the snapshot initializes the book, and the trade precedes the session start
    server.send_record(mbo_msg(
        INSTRUMENT_ID,
        b'A',
        b'A',
        128 | 32,
        100_000_000_000,
    ));
    server.send_record(mbo_msg_with_ts(
        INSTRUMENT_ID,
        b'T',
        b'A',
        128,
        98_000_000_000,
        2_000_000_000,
    ));

    // Live: a far-future timestamp passes the session start
    let far_future_ts = 9_000_000_000_000_000_000u64;
    server.send_record(mbo_msg_with_ts(
        INSTRUMENT_ID,
        b'C',
        b'A',
        128,
        100_000_000_000,
        far_future_ts,
    ));
    server.disconnect();

    let handle = tokio::spawn(async move { handler.run().await });

    let sub = Subscription::builder()
        .symbols(RAW_SYMBOL)
        .schema(dbn::Schema::Mbo)
        .stype_in(dbn::SType::RawSymbol)
        .start(time::OffsetDateTime::from_unix_timestamp(0).unwrap())
        .build();
    cmd_tx.send(HandlerCommand::Subscribe(sub)).unwrap();
    cmd_tx.send(HandlerCommand::Start).unwrap();

    let msg = recv_msg(&mut msg_rx).await;
    match msg {
        DatabentoMessage::Data(Data::BookDeltas(deltas)) => {
            assert_eq!(deltas.deltas.len(), 2);
            assert_eq!(deltas.deltas[0].action, BookAction::Add);
            assert_eq!(deltas.deltas[1].action, BookAction::Delete);
            assert_eq!(deltas.deltas[1].ts_event, far_future_ts);
        }
        other => panic!("expected Data::BookDeltas, was {other:?}"),
    }

    cmd_tx.send(HandlerCommand::Close).unwrap();
    let _ = handle.await;
    server.stop().await;
}

#[rstest]
#[tokio::test]
async fn test_session_success_resets_backoff() {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    let server = MockLsgServer::new(TEST_DATASET).await;
    let (cmd_tx, mut msg_rx, mut handler) = create_test_handler(&server.addr(), TEST_DATASET);

    // Threshold of 0: any session that runs at all counts as successful
    handler = handler.with_success_threshold(Duration::ZERO);

    // First session: sends data then disconnects (counts as success -> resets backoff)
    server.authenticate();
    server.expect_subscription();
    server.start();
    server.send_record(symbol_mapping_msg(INSTRUMENT_ID, RAW_SYMBOL));
    server.send_record(trade_msg(INSTRUMENT_ID, 100_000_000_000, 50));
    server.disconnect();

    // Second session after backoff reset
    server.authenticate();
    server.expect_subscription();
    server.start();
    server.send_record(symbol_mapping_msg(INSTRUMENT_ID, RAW_SYMBOL));
    server.send_record(trade_msg(INSTRUMENT_ID, 101_000_000_000, 25));
    server.disconnect();

    let trade_count = Arc::new(AtomicUsize::new(0));
    let trade_count_rx = trade_count.clone();

    let handle = tokio::spawn(async move { handler.run().await });

    cmd_tx
        .send(HandlerCommand::Subscribe(subscription(dbn::Schema::Trades)))
        .unwrap();
    cmd_tx.send(HandlerCommand::Start).unwrap();

    let collect_handle = tokio::spawn(async move {
        while let Ok(Some(msg)) = tokio::time::timeout(Duration::from_secs(10), msg_rx.recv()).await
        {
            if matches!(msg, DatabentoMessage::Data(Data::Trade(_))) {
                trade_count_rx.fetch_add(1, Ordering::Relaxed);
            }
        }
    });

    wait_until_async(
        || {
            let c = trade_count.clone();
            async move { c.load(Ordering::Relaxed) >= 2 }
        },
        Duration::from_secs(15),
    )
    .await;

    assert!(trade_count.load(Ordering::Relaxed) >= 2);

    cmd_tx.send(HandlerCommand::Close).unwrap();
    let _ = handle.await;
    collect_handle.abort();
    server.stop().await;
}

#[rstest]
#[tokio::test]
async fn test_buffered_subscribe_during_backoff() {
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };

    let server = MockLsgServer::new(TEST_DATASET).await;
    let (cmd_tx, mut msg_rx, mut handler) = create_test_handler(&server.addr(), TEST_DATASET);

    // First attempt: subscribe + start succeed, then session fails quickly
    // (no data, gateway closes immediately -> session error -> backoff)
    server.authenticate();
    server.expect_subscription();
    server.start();
    server.disconnect();

    // Second attempt: handler resubscribes stored subs from first session
    server.authenticate();
    server.expect_subscription(); // resubscribed from first session
    server.start();
    server.send_record(symbol_mapping_msg(1, "ESM4"));
    server.send_record(trade_msg(1, 100_000_000_000, 50));
    server.disconnect();

    let got_trade = Arc::new(AtomicBool::new(false));
    let got_trade_rx = got_trade.clone();

    let handle = tokio::spawn(async move { handler.run().await });

    cmd_tx
        .send(HandlerCommand::Subscribe(subscription(dbn::Schema::Trades)))
        .unwrap();
    cmd_tx.send(HandlerCommand::Start).unwrap();

    let collect_handle = tokio::spawn(async move {
        while let Ok(Some(msg)) = tokio::time::timeout(Duration::from_secs(10), msg_rx.recv()).await
        {
            if matches!(msg, DatabentoMessage::Data(Data::Trade(_))) {
                got_trade_rx.store(true, Ordering::Relaxed);
            }
        }
    });

    wait_until_async(
        || {
            let c = got_trade.clone();
            async move { c.load(Ordering::Relaxed) }
        },
        Duration::from_secs(15),
    )
    .await;

    assert!(got_trade.load(Ordering::Relaxed));

    cmd_tx.send(HandlerCommand::Close).unwrap();
    let _ = handle.await;
    collect_handle.abort();
    server.stop().await;
}

#[rstest]
#[case(false, "XNYM", "GLBX")]
#[case(true, "XNYM", "XNYM")]
#[tokio::test]
async fn test_futures_definition_replaces_inferred_year_and_preserves_venue(
    #[case] use_exchange: bool,
    #[case] exchange: &str,
    #[case] venue: &str,
) {
    let server = MockLsgServer::new(TEST_DATASET).await;

    let config = TestHandlerConfig {
        use_exchange_as_venue: use_exchange,
        ..Default::default()
    };

    let (cmd_tx, mut msg_rx, handler) =
        create_test_handler_with_config(&server.addr(), TEST_DATASET, &config);
    let mut handler = handler.with_futures_year_digits(FuturesYearDigits::Two);
    let mut quote = mbp1_msg(INSTRUMENT_ID, 70_000_000_000, 71_000_000_000, b'A');
    quote.ts_recv = 1_798_761_600_000_000_000;
    let mut definition = instrument_def_msg(INSTRUMENT_ID, b'F');
    definition.expiration = 1_766_102_400_000_000_000;
    definition.raw_symbol = crate::common::str_to_cchar_array::<{ dbn::SYMBOL_CSTR_LEN }>("CLF6");
    definition.exchange = crate::common::str_to_cchar_array::<5>(exchange);

    server.authenticate();
    server.expect_subscription();
    server.start();
    server.send_record(symbol_mapping_msg(INSTRUMENT_ID, "CLF6"));
    server.send_record(quote.clone());
    server.send_record(definition);
    server.send_record(symbol_mapping_msg(INSTRUMENT_ID, "CLF6"));
    server.send_record(quote);

    let handle = tokio::spawn(async move { handler.run().await });
    cmd_tx
        .send(HandlerCommand::Subscribe(
            Subscription::builder()
                .symbols("CLF6")
                .schema(dbn::Schema::Mbp1)
                .build(),
        ))
        .unwrap();
    cmd_tx.send(HandlerCommand::Start).unwrap();

    let DatabentoMessage::Data(Data::Quote(first)) = recv_msg(&mut msg_rx).await else {
        panic!("expected quote");
    };

    let DatabentoMessage::Instrument(instrument) = recv_msg(&mut msg_rx).await else {
        panic!("expected instrument");
    };

    let DatabentoMessage::Data(Data::Quote(last)) = recv_msg(&mut msg_rx).await else {
        panic!("expected quote");
    };

    cmd_tx.send(HandlerCommand::Close).unwrap();
    handle.await.unwrap().unwrap();
    server.stop().await;

    assert_eq!(first.instrument_id.to_string(), "CLF36.GLBX");
    assert_eq!(instrument.id().to_string(), format!("CLF26.{venue}"));
    assert_eq!(instrument.raw_symbol(), Symbol::from("CLF6"));
    assert_eq!(last.instrument_id.to_string(), format!("CLF26.{venue}"));
    assert_eq!(last.bid_price, Price::from("70.00"));
    assert_eq!(last.ask_price, Price::from("71.00"));
}

#[rstest]
#[case(false, "SR3Z26.XCME")]
#[case(true, "SR3Z26.XNYM")]
#[tokio::test]
async fn test_post_year_quotes_preserve_requested_and_definition_identity(
    #[case] use_exchange: bool,
    #[case] defined_id: &str,
) {
    let server = MockLsgServer::new(TEST_DATASET).await;

    let config = TestHandlerConfig {
        use_exchange_as_venue: use_exchange,
        ..Default::default()
    };

    config
        .symbol_venue_map
        .insert(Symbol::from("SR3Z26"), Venue::from("XCME"));
    let (cmd_tx, mut msg_rx, handler) =
        create_test_handler_with_config(&server.addr(), TEST_DATASET, &config);
    let mut handler = handler.with_futures_year_digits(FuturesYearDigits::Two);
    let mut quote = mbp1_msg(INSTRUMENT_ID, 95_120_000_000, 95_250_000_000, b'A');
    quote.ts_recv = "2027-02-01T00:00:00Z"
        .parse::<jiff::Timestamp>()
        .unwrap()
        .as_nanosecond() as u64;
    let mut definition = instrument_def_msg(INSTRUMENT_ID, b'F');
    definition.expiration = "2027-03-16T00:00:00Z"
        .parse::<jiff::Timestamp>()
        .unwrap()
        .as_nanosecond() as u64;
    definition.raw_symbol = crate::common::str_to_cchar_array::<{ dbn::SYMBOL_CSTR_LEN }>("SR3Z6");
    definition.exchange = crate::common::str_to_cchar_array::<5>("XNYM");

    server.authenticate();
    server.expect_subscription();
    server.start();
    server.send_record(symbol_mapping_msg(INSTRUMENT_ID, "SR3Z6"));
    server.send_record(quote.clone());
    server.send_record(definition);
    server.send_record(symbol_mapping_msg(INSTRUMENT_ID, "SR3Z6"));
    server.send_record(quote.clone());
    server.send_record(symbol_mapping_msg(INSTRUMENT_ID, "SR3H7"));
    server.send_record(quote);

    let handle = tokio::spawn(async move { handler.run().await });
    cmd_tx
        .send(HandlerCommand::Subscribe(
            Subscription::builder()
                .symbols("SR3Z6")
                .schema(dbn::Schema::Mbp1)
                .build(),
        ))
        .unwrap();
    cmd_tx.send(HandlerCommand::Start).unwrap();

    let DatabentoMessage::Data(Data::Quote(first)) = recv_msg(&mut msg_rx).await else {
        panic!("expected quote");
    };

    let DatabentoMessage::Instrument(instrument) = recv_msg(&mut msg_rx).await else {
        panic!("expected instrument");
    };

    let DatabentoMessage::Data(Data::Quote(repeated)) = recv_msg(&mut msg_rx).await else {
        panic!("expected quote");
    };

    let DatabentoMessage::Data(Data::Quote(changed)) = recv_msg(&mut msg_rx).await else {
        panic!("expected quote");
    };

    cmd_tx.send(HandlerCommand::Close).unwrap();
    handle.await.unwrap().unwrap();
    server.stop().await;

    assert_eq!(first.instrument_id.to_string(), "SR3Z26.XCME");
    assert_eq!(instrument.id().to_string(), defined_id);
    assert_eq!(instrument.raw_symbol(), Symbol::from("SR3Z6"));
    assert_eq!(repeated.instrument_id.to_string(), defined_id);
    assert_eq!(changed.instrument_id.to_string(), "SR3H27.GLBX");
    assert_eq!(repeated.bid_price, Price::from("95.12"));
    assert_eq!(repeated.ask_price, Price::from("95.25"));
}

#[rstest]
#[case(1, false, false, "SR3Z6.GLBX", "SR3Z6.XCME")]
#[case(1, true, false, "SR3Z6.GLBX", "SR3Z6.XCME")]
#[case(1, true, true, "SR3Z6.XNYM", "SR3Z6.XCME")]
#[case(2, false, false, "SR3Z36.GLBX", "SR3Z26.XCME")]
#[case(2, true, false, "SR3Z26.GLBX", "SR3Z26.XCME")]
#[case(2, true, true, "SR3Z26.XNYM", "SR3Z26.XNYM")]
#[tokio::test]
async fn test_later_contract_subscription_refreshes_inferred_identity(
    #[case] digits: u8,
    #[case] with_definition: bool,
    #[case] use_exchange: bool,
    #[case] first_id: &str,
    #[case] last_id: &str,
) {
    let server = MockLsgServer::new(TEST_DATASET).await;

    let config = TestHandlerConfig {
        use_exchange_as_venue: use_exchange,
        ..Default::default()
    };

    let (cmd_tx, mut msg_rx, handler) =
        create_test_handler_with_config(&server.addr(), TEST_DATASET, &config);
    let mut handler = handler.with_futures_year_digits(digits.try_into().unwrap());
    let mut quote = mbp1_msg(INSTRUMENT_ID, 95_120_000_000, 95_250_000_000, b'A');
    quote.ts_recv = "2027-02-01T00:00:00Z"
        .parse::<jiff::Timestamp>()
        .unwrap()
        .as_nanosecond() as u64;
    let mut definition = instrument_def_msg(INSTRUMENT_ID, b'F');
    definition.expiration = "2027-03-16T00:00:00Z"
        .parse::<jiff::Timestamp>()
        .unwrap()
        .as_nanosecond() as u64;
    definition.raw_symbol = crate::common::str_to_cchar_array::<{ dbn::SYMBOL_CSTR_LEN }>("SR3Z6");
    definition.exchange = crate::common::str_to_cchar_array::<5>("XNYM");

    server.authenticate();
    server.expect_subscription();
    if with_definition {
        server.expect_subscription();
    }

    server.start();
    server.send_record(symbol_mapping_msg(INSTRUMENT_ID, "SR3Z6"));
    if with_definition {
        server.send_record(definition);
    }

    server.send_record(quote.clone());

    let handle = tokio::spawn(async move { handler.run().await });
    cmd_tx
        .send(HandlerCommand::Subscribe(
            Subscription::builder()
                .symbols("SR3.c.0")
                .schema(dbn::Schema::Mbp1)
                .stype_in(dbn::SType::Continuous)
                .build(),
        ))
        .unwrap();

    if with_definition {
        cmd_tx
            .send(HandlerCommand::Subscribe(
                Subscription::builder()
                    .symbols("SR3.FUT")
                    .schema(dbn::Schema::Definition)
                    .stype_in(dbn::SType::Parent)
                    .build(),
            ))
            .unwrap();
    }

    cmd_tx.send(HandlerCommand::Start).unwrap();

    let instrument = if with_definition {
        let DatabentoMessage::Instrument(instrument) = recv_msg(&mut msg_rx).await else {
            panic!("expected instrument");
        };

        Some(instrument)
    } else {
        None
    };

    let DatabentoMessage::Data(Data::Quote(first)) = recv_msg(&mut msg_rx).await else {
        panic!("expected quote");
    };

    let symbol = if digits == 1 { "SR3Z6" } else { "SR3Z26" };
    config
        .symbol_venue_map
        .insert(Symbol::from(symbol), Venue::from("XCME"));
    server.expect_subscription();
    server.send_record(symbol_mapping_msg(INSTRUMENT_ID, "SR3Z6"));
    server.send_record(quote);
    cmd_tx
        .send(HandlerCommand::Subscribe(
            Subscription::builder()
                .symbols("SR3Z6")
                .schema(dbn::Schema::Mbp1)
                .build(),
        ))
        .unwrap();

    let DatabentoMessage::Data(Data::Quote(last)) = recv_msg(&mut msg_rx).await else {
        panic!("expected quote");
    };

    cmd_tx.send(HandlerCommand::Close).unwrap();
    handle.await.unwrap().unwrap();
    server.stop().await;

    assert_eq!(first.instrument_id.to_string(), first_id);

    if let Some(instrument) = instrument {
        assert_eq!(instrument.id().to_string(), first_id);
        assert_eq!(instrument.raw_symbol(), Symbol::from("SR3Z6"));
    }

    assert_eq!(last.instrument_id.to_string(), last_id);
    assert_eq!(last.bid_price, Price::from("95.12"));
    assert_eq!(last.ask_price, Price::from("95.25"));
    assert_eq!(last.bid_size, Quantity::from(100));
    assert_eq!(last.ask_size, Quantity::from(50));
    assert_eq!(last.ts_event.as_u64(), 1_801_440_000_000_000_000);
}
