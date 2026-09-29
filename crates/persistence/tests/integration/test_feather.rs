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

use std::{
    fs::{self, File},
    sync::{Arc, atomic::AtomicU64},
};

use datafusion::arrow::ipc::reader::StreamReader;
use nautilus_core::UnixNanos;
use nautilus_model::{
    data::{
        BookOrder, Data, FundingRateUpdate, OrderBookDelta, OrderBookDeltas, QuoteTick, TradeTick,
    },
    enums::{AggressorSide, BookAction, OrderSide},
    identifiers::{InstrumentId, TradeId},
    types::{Price, Quantity},
};
use nautilus_persistence::{
    backend::parquet::catalog::ParquetDataCatalog,
    writer::feather::{FeatherWriter, RotationConfig, WriterClock},
};
use rstest::rstest;
use tempfile::TempDir;

#[rstest]
fn test_write_data_enum_quote() {
    let temp_dir = TempDir::new().unwrap();
    let clock = WriterClock::Test(Arc::new(AtomicU64::new(0)));

    let mut writer = FeatherWriter::new(
        temp_dir.path().to_path_buf(),
        clock,
        RotationConfig::NoRotation,
        None,
        None,
    );

    let quote = QuoteTick::new(
        InstrumentId::from("AUD/USD.SIM"),
        Price::from("1.0"),
        Price::from("1.0"),
        Quantity::from("1000"),
        Quantity::from("1000"),
        UnixNanos::from(1000),
        UnixNanos::from(1000),
    );

    writer.write_data(Data::Quote(quote)).unwrap();
    writer.close().unwrap();
}

#[rstest]
fn test_write_data_enum_all_types() {
    let temp_dir = TempDir::new().unwrap();
    let clock = WriterClock::Test(Arc::new(AtomicU64::new(0)));

    let mut writer = FeatherWriter::new(
        temp_dir.path().to_path_buf(),
        clock,
        RotationConfig::NoRotation,
        None,
        None,
    );

    let instrument_id = InstrumentId::from("AUD/USD.SIM");

    // Test all data types via write_data
    let quote = QuoteTick::new(
        instrument_id,
        Price::from("1.0"),
        Price::from("1.0"),
        Quantity::from("1000"),
        Quantity::from("1000"),
        UnixNanos::from(1000),
        UnixNanos::from(1000),
    );
    writer.write_data(Data::Quote(quote)).unwrap();

    let trade = TradeTick::new(
        instrument_id,
        Price::from("1.0"),
        Quantity::from("1000"),
        AggressorSide::Buy,
        TradeId::from("1"),
        UnixNanos::from(2000),
        UnixNanos::from(2000),
    );
    writer.write_data(Data::Trade(trade)).unwrap();

    let delta = OrderBookDelta::clear(
        instrument_id,
        0,
        UnixNanos::from(3000),
        UnixNanos::from(3000),
    );
    writer.write_data(Data::BookDelta(delta)).unwrap();

    let funding_rate = FundingRateUpdate::new(
        instrument_id,
        "0.0001".parse().unwrap(),
        Some(480),
        Some(UnixNanos::from(5_000)),
        UnixNanos::from(4_000),
        UnixNanos::from(4_000),
    );
    writer.write_data(Data::FundingRate(funding_rate)).unwrap();

    writer.close().unwrap();
}

#[rstest]
fn test_write_data_orderbook_deltas() {
    let temp_dir = TempDir::new().unwrap();
    let clock = WriterClock::Test(Arc::new(AtomicU64::new(0)));

    let mut writer = FeatherWriter::new(
        temp_dir.path().to_path_buf(),
        clock,
        RotationConfig::NoRotation,
        None,
        None,
    );

    let instrument_id = InstrumentId::from("AUD/USD.SIM");
    let delta1 = OrderBookDelta::clear(
        instrument_id,
        0,
        UnixNanos::from(1000),
        UnixNanos::from(1000),
    );
    let delta2 = OrderBookDelta::clear(
        instrument_id,
        0,
        UnixNanos::from(2000),
        UnixNanos::from(2000),
    );

    let deltas = OrderBookDeltas::new(instrument_id, vec![delta1, delta2]);
    // Test writing OrderBookDeltas via write_data
    writer
        .write_data(Data::BookDeltas(Box::new(deltas)))
        .unwrap();
    writer.close().unwrap();
}

#[rstest]
fn test_auto_flush() {
    let temp_dir = TempDir::new().unwrap();
    let shared_time = Arc::new(AtomicU64::new(0));
    let clock = WriterClock::Test(Arc::clone(&shared_time));

    let mut writer = FeatherWriter::new(
        temp_dir.path().to_path_buf(),
        clock,
        RotationConfig::NoRotation,
        None,
        Some(100), // 100ms flush interval
    );

    let quote = QuoteTick::new(
        InstrumentId::from("AUD/USD.SIM"),
        Price::from("1.0"),
        Price::from("1.0"),
        Quantity::from("1000"),
        Quantity::from("1000"),
        UnixNanos::from(1000),
        UnixNanos::from(1000),
    );

    // Write first quote; the interval has not elapsed so no bytes reach disk yet
    writer.write(quote).unwrap();
    let partial = temp_dir
        .path()
        .join("quotes")
        .join("quotes_0.feather.partial");
    assert_eq!(fs::metadata(&partial).unwrap().len(), 0);

    // Advance the shared time source past the 100ms flush interval
    shared_time.store(200_000_000, std::sync::atomic::Ordering::Relaxed);

    // Second write hits the auto-flush boundary and appends both quotes to the same file
    let quote2 = QuoteTick::new(
        InstrumentId::from("AUD/USD.SIM"),
        Price::from("1.1"),
        Price::from("1.1"),
        Quantity::from("1000"),
        Quantity::from("1000"),
        UnixNanos::from(2000),
        UnixNanos::from(2000),
    );
    writer.write(quote2).unwrap();

    let rows = StreamReader::try_new(File::open(&partial).unwrap(), None)
        .unwrap()
        .map(|batch| batch.unwrap().num_rows())
        .sum::<usize>();
    assert_eq!(rows, 2);
    assert_eq!(temp_dir.path().read_dir().unwrap().count(), 1);
}

#[rstest]
fn test_close() {
    let temp_dir = TempDir::new().unwrap();
    let clock = WriterClock::Test(Arc::new(AtomicU64::new(0)));

    let mut writer = FeatherWriter::new(
        temp_dir.path().to_path_buf(),
        clock,
        RotationConfig::NoRotation,
        None,
        None,
    );

    let quote = QuoteTick::new(
        InstrumentId::from("AUD/USD.SIM"),
        Price::from("1.0"),
        Price::from("1.0"),
        Quantity::from("1000"),
        Quantity::from("1000"),
        UnixNanos::from(1000),
        UnixNanos::from(1000),
    );

    writer.write(quote).unwrap();

    // Close should seal and clear writers
    writer.close().unwrap();
}

// Note: Message bus subscription test is skipped due to async/sync boundary complexity.
// The handler uses block_on which can't be used from within an async runtime (tokio test).
// This functionality is better tested via Python integration tests where the message bus
// is used in a non-async context or via proper async task spawning.

// Regression test for https://github.com/nautechsystems/nautilus_trader/issues/3913,
// where a leading BookAction::Clear delta poisoned file metadata with 0 precision.
#[rstest]
#[case::clear_first(vec![
    OrderBookDelta::clear(InstrumentId::from("AUD/USD.SIM"), 0, UnixNanos::from(1000), UnixNanos::from(1000)),
    book_add(InstrumentId::from("AUD/USD.SIM"), Price::new(1.23, 2), Quantity::new(100.0, 6), 2000),
])]
#[case::all_sentinels(vec![
    OrderBookDelta::clear(InstrumentId::from("AUD/USD.SIM"), 0, UnixNanos::from(1000), UnixNanos::from(1000)),
    OrderBookDelta::clear(InstrumentId::from("AUD/USD.SIM"), 1, UnixNanos::from(2000), UnixNanos::from(2000)),
])]
fn test_write_orderbook_deltas_round_trip_precision(#[case] deltas: Vec<OrderBookDelta>) {
    let temp_dir = TempDir::new().unwrap();
    let mut writer = run_writer(temp_dir.path());

    let book_deltas = OrderBookDeltas::new(InstrumentId::from("AUD/USD.SIM"), deltas.clone());
    writer
        .write_data(Data::BookDeltas(Box::new(book_deltas)))
        .unwrap();
    writer.close().unwrap();

    assert_eq!(
        read_run(temp_dir.path()),
        deltas.into_iter().map(Data::from).collect::<Vec<_>>(),
    );
}

#[rstest]
fn test_write_batch_keeps_each_instrument_precision_in_one_file() {
    let temp_dir = TempDir::new().unwrap();
    let mut writer = run_writer(temp_dir.path());
    let instrument_a = InstrumentId::from("AUD/USD.SIM");
    let instrument_b = InstrumentId::from("BTC/USD.BINANCE");
    let deltas = vec![
        book_add(
            instrument_a,
            Price::new(1.23, 2),
            Quantity::new(100.0, 0),
            1000,
        ),
        book_add(
            instrument_b,
            Price::new(20_000.0, 4),
            Quantity::new(0.123_456_78, 8),
            2000,
        ),
        book_add(
            instrument_a,
            Price::new(1.24, 2),
            Quantity::new(50.0, 0),
            3000,
        ),
        book_add(
            instrument_b,
            Price::new(20_100.0, 4),
            Quantity::new(0.25, 8),
            4000,
        ),
    ];

    writer.write_batch(deltas.clone()).unwrap();
    writer.close().unwrap();

    let files = collect_feather_files(temp_dir.path());
    assert_eq!(
        files.len(),
        1,
        "expected one file for the type, found {files:?}"
    );
    assert_eq!(
        read_run(temp_dir.path()),
        deltas.into_iter().map(Data::from).collect::<Vec<_>>(),
    );
}

fn book_add(instrument_id: InstrumentId, price: Price, size: Quantity, ts: u64) -> OrderBookDelta {
    OrderBookDelta::new(
        instrument_id,
        BookAction::Add,
        BookOrder {
            side: OrderSide::Buy.into(),
            price,
            size,
            order_id: 1,
        },
        0,
        1,
        UnixNanos::from(ts),
        UnixNanos::from(ts),
    )
}

// Writes into the backtest run folder `read_run` reads back
fn run_writer(root: &std::path::Path) -> FeatherWriter {
    FeatherWriter::new(
        root.join("backtest").join("run-001"),
        WriterClock::Test(Arc::new(AtomicU64::new(0))),
        RotationConfig::NoRotation,
        None,
        None,
    )
}

fn read_run(root: &std::path::Path) -> Vec<Data> {
    ParquetDataCatalog::new(root, None, None, None, None)
        .read_backtest("run-001")
        .unwrap()
}

fn collect_feather_files(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    collect_feather_files_into(dir, &mut out);
    out
}

fn collect_feather_files_into(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            collect_feather_files_into(&path, out);
        } else if path.extension().and_then(|s| s.to_str()) == Some("feather") {
            out.push(path);
        }
    }
}
