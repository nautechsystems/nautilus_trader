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

mod common;

use std::time::Duration;

use common::{
    TEST_DATASET, TestHandlerConfig, create_test_handler, create_test_handler_with_config,
    error_msg, imbalance_msg, instrument_def_msg, mbo_msg, mbo_msg_with_ts, mbp1_msg, mbp10_msg,
    mock_server::MockLsgServer, ohlcv_msg, statistics_msg, status_msg, symbol_mapping_msg,
    system_msg, trade_msg,
};
use databento::{
    dbn::{self},
    live::Subscription,
};
use nautilus_common::testing::wait_until_async;
use nautilus_databento::live::{DatabentoMessage, HandlerCommand};
use nautilus_model::{data::Data, instruments::Instrument};
use rstest::rstest;

const INSTRUMENT_ID: u32 = 1;
const RAW_SYMBOL: &str = "ESM4";
const RECV_TIMEOUT: Duration = Duration::from_secs(5);

async fn recv_msg(rx: &mut tokio::sync::mpsc::Receiver<DatabentoMessage>) -> DatabentoMessage {
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
    server.send_record(trade_msg(INSTRUMENT_ID, 100_000_000_000, 50));
    server.disconnect();

    let handle = tokio::spawn(async move { handler.run().await });

    cmd_tx
        .send(HandlerCommand::Subscribe(subscription(dbn::Schema::Trades)))
        .unwrap();
    cmd_tx.send(HandlerCommand::Start).unwrap();

    let msg = recv_msg(&mut msg_rx).await;
    match msg {
        DatabentoMessage::Data(Data::Trade(trade)) => {
            assert_eq!(trade.instrument_id.symbol.as_str(), RAW_SYMBOL);
            assert!(trade.price.as_f64() > 0.0);
            assert!(trade.size.as_f64() > 0.0);
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
        100_000_000_000, // bid 100.00
        101_000_000_000, // ask 101.00
        b'A',            // Add action
    ));
    server.disconnect();

    let handle = tokio::spawn(async move { handler.run().await });

    cmd_tx
        .send(HandlerCommand::Subscribe(subscription(dbn::Schema::Mbp1)))
        .unwrap();
    cmd_tx.send(HandlerCommand::Start).unwrap();

    let msg = recv_msg(&mut msg_rx).await;
    match msg {
        DatabentoMessage::Data(Data::Quote(quote)) => {
            assert_eq!(quote.instrument_id.symbol.as_str(), RAW_SYMBOL);
            assert!(quote.bid_price.as_f64() < quote.ask_price.as_f64());
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
        DatabentoMessage::Data(Data::Depth10(depth)) => {
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
        other => panic!("expected Data::Depth10, was {other:?}"),
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
        DatabentoMessage::Data(Data::Deltas(deltas)) => {
            assert!(!deltas.deltas.is_empty(), "expected at least one delta");
        }
        other => panic!("expected Data::Deltas, was {other:?}"),
    }

    cmd_tx.send(HandlerCommand::Close).unwrap();
    let _ = handle.await;
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
        DatabentoMessage::Data(Data::Deltas(deltas)) => {
            assert_eq!(
                deltas.deltas.len(),
                3,
                "expected 3 buffered deltas, was {}",
                deltas.deltas.len()
            );
        }
        other => panic!("expected Data::Deltas, was {other:?}"),
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
        DatabentoMessage::Data(Data::Deltas(deltas)) => {
            assert_eq!(
                deltas.deltas.len(),
                3,
                "expected 3 deltas (2 snapshot + 1 delta), was {}",
                deltas.deltas.len()
            );
        }
        other => panic!("expected Data::Deltas, was {other:?}"),
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
            DatabentoMessage::Data(Data::Deltas(deltas)) => {
                symbols.push(deltas.instrument_id.symbol.to_string());
            }
            other => panic!("expected Data::Deltas, was {other:?}"),
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
        DatabentoMessage::Data(Data::Deltas(deltas)) => {
            assert_eq!(
                deltas.deltas.len(),
                3,
                "expected 3 deltas (2 replay + 1 live), was {}",
                deltas.deltas.len()
            );
        }
        other => panic!("expected Data::Deltas, was {other:?}"),
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
