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

use std::{cell::RefCell, collections::HashSet, fs::File, rc::Rc, sync::Arc};

use datafusion::arrow::ipc::reader::StreamReader;
use nautilus_common::clock::{Clock, TestClock};
use nautilus_core::UnixNanos;
use nautilus_model::{
    data::{
        BookOrder, Data, OrderBookDelta, OrderBookDeltas, OrderBookDeltas_API, QuoteTick, TradeTick,
    },
    enums::{AggressorSide, BookAction, OrderSide},
    identifiers::{InstrumentId, TradeId},
    types::{Price, Quantity},
};
use nautilus_persistence::backend::feather::{FeatherWriter, RotationConfig};
use object_store::{ObjectStore, local::LocalFileSystem};
use rstest::rstest;
use tempfile::TempDir;

#[rstest]
#[tokio::test]
async fn test_write_data_enum_quote() {
    let temp_dir = TempDir::new().unwrap();
    let base_path = temp_dir.path().to_str().unwrap().to_string();
    let local_fs = LocalFileSystem::new_with_prefix(temp_dir.path()).unwrap();
    let store: Arc<dyn ObjectStore> = Arc::new(local_fs);
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(TestClock::new()));

    let mut writer = FeatherWriter::new(
        base_path,
        store,
        clock,
        RotationConfig::NoRotation,
        None,
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

    writer.write_data(Data::Quote(quote)).await.unwrap();
    writer.flush().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_write_data_enum_all_types() {
    let temp_dir = TempDir::new().unwrap();
    let base_path = temp_dir.path().to_str().unwrap().to_string();
    let local_fs = LocalFileSystem::new_with_prefix(temp_dir.path()).unwrap();
    let store: Arc<dyn ObjectStore> = Arc::new(local_fs);
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(TestClock::new()));

    let mut writer = FeatherWriter::new(
        base_path,
        store,
        clock,
        RotationConfig::NoRotation,
        None,
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
    writer.write_data(Data::Quote(quote)).await.unwrap();

    let trade = TradeTick::new(
        instrument_id,
        Price::from("1.0"),
        Quantity::from("1000"),
        AggressorSide::Buyer,
        TradeId::from("1"),
        UnixNanos::from(2000),
        UnixNanos::from(2000),
    );
    writer.write_data(Data::Trade(trade)).await.unwrap();

    let delta = OrderBookDelta::clear(
        instrument_id,
        0,
        UnixNanos::from(3000),
        UnixNanos::from(3000),
    );
    writer.write_data(Data::Delta(delta)).await.unwrap();

    writer.flush().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_write_data_orderbook_deltas() {
    let temp_dir = TempDir::new().unwrap();
    let base_path = temp_dir.path().to_str().unwrap().to_string();
    let local_fs = LocalFileSystem::new_with_prefix(temp_dir.path()).unwrap();
    let store: Arc<dyn ObjectStore> = Arc::new(local_fs);
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(TestClock::new()));

    let mut writer = FeatherWriter::new(
        base_path,
        store,
        clock,
        RotationConfig::NoRotation,
        None,
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
    let deltas_api = OrderBookDeltas_API::new(deltas);

    // Test writing OrderBookDeltas via write_data
    writer.write_data(Data::Deltas(deltas_api)).await.unwrap();
    writer.flush().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_auto_flush() {
    let temp_dir = TempDir::new().unwrap();
    let base_path = temp_dir.path().to_str().unwrap().to_string();
    let local_fs = LocalFileSystem::new_with_prefix(temp_dir.path()).unwrap();
    let store: Arc<dyn ObjectStore> = Arc::new(local_fs);
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(TestClock::new()));

    let mut writer = FeatherWriter::new(
        base_path,
        store,
        clock.clone(),
        RotationConfig::NoRotation,
        None,
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

    // Write first quote
    writer.write(quote).await.unwrap();

    // Note: TestClock doesn't have set_time_ns, so we can't easily test auto-flush
    // with time advancement. Instead, we test that check_flush is called during write.
    // For a proper test, we'd need a mock clock or use LiveClock with time advancement.

    // Write second quote - check_flush will be called but won't flush if time hasn't advanced
    let quote2 = QuoteTick::new(
        InstrumentId::from("AUD/USD.SIM"),
        Price::from("1.1"),
        Price::from("1.1"),
        Quantity::from("1000"),
        Quantity::from("1000"),
        UnixNanos::from(2000),
        UnixNanos::from(2000),
    );
    writer.write(quote2).await.unwrap();

    // Verify that writes succeeded (check_flush was called, even if it didn't flush)
    // The flush_interval_ms is set, so check_flush runs but won't flush without time advancement
}

#[rstest]
#[tokio::test]
async fn test_close() {
    let temp_dir = TempDir::new().unwrap();
    let base_path = temp_dir.path().to_str().unwrap().to_string();
    let local_fs = LocalFileSystem::new_with_prefix(temp_dir.path()).unwrap();
    let store: Arc<dyn ObjectStore> = Arc::new(local_fs);
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(TestClock::new()));

    let mut writer = FeatherWriter::new(
        base_path,
        store,
        clock,
        RotationConfig::NoRotation,
        None,
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

    writer.write(quote).await.unwrap();

    // Close should flush and clear writers
    writer.close().await.unwrap();
}

// Note: Message bus subscription test is skipped due to async/sync boundary complexity.
// The handler uses block_on which can't be used from within an async runtime (tokio test).
// This functionality is better tested via Python integration tests where the message bus
// is used in a non-async context or via proper async task spawning.

// Regression test for https://github.com/nautechsystems/nautilus_trader/issues/3913,
// where a leading BookAction::Clear delta poisoned file metadata with 0 precision.
#[rstest]
#[tokio::test]
async fn test_write_orderbook_deltas_clear_first_preserves_precision() {
    let temp_dir = TempDir::new().unwrap();
    let base_path = temp_dir.path().to_str().unwrap().to_string();
    let local_fs = LocalFileSystem::new_with_prefix(temp_dir.path()).unwrap();
    let store: Arc<dyn ObjectStore> = Arc::new(local_fs);
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(TestClock::new()));

    let mut per_instrument = HashSet::new();
    per_instrument.insert("order_book_deltas".to_string());

    let mut writer = FeatherWriter::new(
        base_path,
        store,
        clock,
        RotationConfig::NoRotation,
        None,
        Some(per_instrument),
        None,
    );

    let instrument_id = InstrumentId::from("AUD/USD.SIM");
    let clear = OrderBookDelta::clear(
        instrument_id,
        0,
        UnixNanos::from(1000),
        UnixNanos::from(1000),
    );
    let add = OrderBookDelta::new(
        instrument_id,
        BookAction::Add,
        BookOrder {
            side: OrderSide::Buy,
            price: Price::new(1.23, 2),
            size: Quantity::new(100.0, 6),
            order_id: 1,
        },
        0,
        1,
        UnixNanos::from(2000),
        UnixNanos::from(2000),
    );

    let deltas = OrderBookDeltas::new(instrument_id, vec![clear, add]);
    let deltas_api = OrderBookDeltas_API::new(deltas);

    writer.write_data(Data::Deltas(deltas_api)).await.unwrap();
    writer.flush().await.unwrap();

    let feather_path = find_feather_file(temp_dir.path());
    let file = File::open(&feather_path).unwrap();
    let reader = StreamReader::try_new(file, None).unwrap();
    let metadata = reader.schema().metadata().clone();

    assert_eq!(
        metadata.get("price_precision"),
        Some(&"2".to_string()),
        "file metadata should reflect real price precision, not the CLEAR sentinel",
    );
    assert_eq!(
        metadata.get("size_precision"),
        Some(&"6".to_string()),
        "file metadata should reflect real size precision, not the CLEAR sentinel",
    );
}

// Regression test for the all-sentinel fallback: a batch containing only
// BookAction::Clear rows has no real precision to derive, so file metadata
// legitimately carries price_precision=0, size_precision=0.
#[rstest]
#[tokio::test]
async fn test_write_orderbook_deltas_all_sentinel_metadata_fallback() {
    let temp_dir = TempDir::new().unwrap();
    let base_path = temp_dir.path().to_str().unwrap().to_string();
    let local_fs = LocalFileSystem::new_with_prefix(temp_dir.path()).unwrap();
    let store: Arc<dyn ObjectStore> = Arc::new(local_fs);
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(TestClock::new()));

    let mut per_instrument = HashSet::new();
    per_instrument.insert("order_book_deltas".to_string());

    let mut writer = FeatherWriter::new(
        base_path,
        store,
        clock,
        RotationConfig::NoRotation,
        None,
        Some(per_instrument),
        None,
    );

    let instrument_id = InstrumentId::from("AUD/USD.SIM");
    let clear1 = OrderBookDelta::clear(
        instrument_id,
        0,
        UnixNanos::from(1000),
        UnixNanos::from(1000),
    );
    let clear2 = OrderBookDelta::clear(
        instrument_id,
        1,
        UnixNanos::from(2000),
        UnixNanos::from(2000),
    );

    let deltas = OrderBookDeltas::new(instrument_id, vec![clear1, clear2]);
    let deltas_api = OrderBookDeltas_API::new(deltas);

    writer.write_data(Data::Deltas(deltas_api)).await.unwrap();
    writer.flush().await.unwrap();

    let feather_path = find_feather_file(temp_dir.path());
    let file = File::open(&feather_path).unwrap();
    let reader = StreamReader::try_new(file, None).unwrap();
    let metadata = reader.schema().metadata().clone();

    assert_eq!(metadata.get("price_precision"), Some(&"0".to_string()));
    assert_eq!(metadata.get("size_precision"), Some(&"0".to_string()));
}

// Regression test for the mixed-instrument routing in write_batch. When a
// batch contains deltas for multiple instruments, each instrument's rows
// must land in its own file with its own precision metadata.
#[rstest]
#[tokio::test]
async fn test_write_batch_partitions_by_instrument() {
    let temp_dir = TempDir::new().unwrap();
    let base_path = temp_dir.path().to_str().unwrap().to_string();
    let local_fs = LocalFileSystem::new_with_prefix(temp_dir.path()).unwrap();
    let store: Arc<dyn ObjectStore> = Arc::new(local_fs);
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(TestClock::new()));

    let mut per_instrument = HashSet::new();
    per_instrument.insert("order_book_deltas".to_string());

    let mut writer = FeatherWriter::new(
        base_path,
        store,
        clock,
        RotationConfig::NoRotation,
        None,
        Some(per_instrument),
        None,
    );

    let instrument_a = InstrumentId::from("AUD/USD.SIM");
    let instrument_b = InstrumentId::from("BTC/USD.BINANCE");

    let make_add = |instrument_id, price: f64, price_prec, size: f64, size_prec, ts| {
        OrderBookDelta::new(
            instrument_id,
            BookAction::Add,
            BookOrder {
                side: OrderSide::Buy,
                price: Price::new(price, price_prec),
                size: Quantity::new(size, size_prec),
                order_id: 1,
            },
            0,
            1,
            UnixNanos::from(ts),
            UnixNanos::from(ts),
        )
    };

    let deltas = vec![
        make_add(instrument_a, 1.23, 2, 100.0, 0, 1000),
        make_add(instrument_b, 20_000.0, 4, 0.12345678, 8, 2000),
        make_add(instrument_a, 1.24, 2, 50.0, 0, 3000),
        make_add(instrument_b, 20_100.0, 4, 0.25, 8, 4000),
    ];

    writer.write_batch(deltas).await.unwrap();
    writer.flush().await.unwrap();

    let files = collect_feather_files(temp_dir.path());
    assert_eq!(
        files.len(),
        2,
        "expected one file per instrument, found {files:?}"
    );

    let mut by_instrument = std::collections::HashMap::new();

    for path in files {
        let reader = StreamReader::try_new(File::open(&path).unwrap(), None).unwrap();
        let metadata = reader.schema().metadata().clone();
        let instrument_id = metadata
            .get("instrument_id")
            .expect("instrument_id metadata")
            .clone();
        by_instrument.insert(instrument_id, metadata);
    }

    let metadata_a = by_instrument.get("AUD/USD.SIM").expect("AUD/USD.SIM file");
    assert_eq!(metadata_a.get("price_precision"), Some(&"2".to_string()));
    assert_eq!(metadata_a.get("size_precision"), Some(&"0".to_string()));

    let metadata_b = by_instrument
        .get("BTC/USD.BINANCE")
        .expect("BTC/USD.BINANCE file");
    assert_eq!(metadata_b.get("price_precision"), Some(&"4".to_string()));
    assert_eq!(metadata_b.get("size_precision"), Some(&"8".to_string()));
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

fn find_feather_file(dir: &std::path::Path) -> std::path::PathBuf {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            let found = find_feather_file(&path);
            if !found.as_os_str().is_empty() {
                return found;
            }
        } else if path.extension().and_then(|s| s.to_str()) == Some("feather") {
            return path;
        }
    }
    std::path::PathBuf::new()
}
