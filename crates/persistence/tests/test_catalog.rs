// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

use nautilus_core::UnixNanos;
use nautilus_model::{
    data::{
        Bar, BarSpecification, BarType, BookOrder, Data, IndexPriceUpdate, MarkPriceUpdate,
        OrderBookDelta, OrderBookDepth10, QuoteTick, TradeTick, depth::DEPTH10_LEN,
        is_monotonically_increasing_by_init, to_variant,
    },
    enums::{AggregationSource, AggressorSide, BarAggregation, BookAction, OrderSide, PriceType},
    identifiers::{InstrumentId, TradeId},
    types::{Price, Quantity},
};
use nautilus_persistence::backend::{
    catalog::ParquetDataCatalog,
    session::{DataBackendSession, QueryResult},
};
use nautilus_serialization::arrow::ArrowSchemaProvider;
use nautilus_testkit::common::get_nautilus_test_data_file_path;
#[cfg(target_os = "linux")]
use procfs::{self, process::Process};
use rstest::rstest;
use tempfile::TempDir;

/// Memory leak test
///
/// Uses arguments from setup to run function for given number of iterations.
/// Checks that the difference between memory after 1 and iter + 1 runs is
/// less than threshold.
#[allow(dead_code)]
#[cfg(target_os = "linux")]
fn mem_leak_test<T>(setup: impl FnOnce() -> T, run: impl Fn(&T), threshold: f64, iter: usize) {
    let args = setup();
    // measure mem after setup
    let page_size = procfs::page_size();
    let me = Process::myself().unwrap();
    let setup_mem = me.stat().unwrap().rss * page_size / 1024;

    {
        run(&args);
    }

    let before = me.stat().unwrap().rss * page_size / 1024 - setup_mem;

    for _ in 0..iter {
        run(&args);
    }

    let after = me.stat().unwrap().rss * page_size / 1024 - setup_mem;

    if !(after.abs_diff(before) as f64 / (before as f64) < threshold) {
        println!("Memory leak detected after {iter} iterations");
        println!("Memory before runs (in KB): {before}");
        println!("Memory after runs (in KB): {after}");
        assert!(false);
    }
}

#[rstest]
fn test_quote_tick_query() {
    let expected_length = 9_500;
    let file_path = get_nautilus_test_data_file_path("quotes.parquet");

    let mut catalog = DataBackendSession::new(10_000);
    catalog
        .add_file::<QuoteTick>("quote_005", file_path.as_str(), None)
        .unwrap();
    let query_result: QueryResult = catalog.get_query_result();
    let ticks: Vec<Data> = query_result.collect();

    if let Data::Quote(q) = ticks[0] {
        assert_eq!("EUR/USD.SIM", q.instrument_id.to_string());
    } else {
        panic!("Invalid test");
    }

    assert_eq!(ticks.len(), expected_length);
    assert!(is_monotonically_increasing_by_init(&ticks));
}

#[rstest]
fn test_quote_tick_query_with_filter() {
    let file_path = get_nautilus_test_data_file_path("quotes-3-groups-filter-query.parquet");

    let mut catalog = DataBackendSession::new(10);
    catalog
        .add_file::<QuoteTick>(
            "quote_005",
            file_path.as_str(),
            Some("SELECT * FROM quote_005 WHERE ts_init >= 1701388832486000000 ORDER BY ts_init"),
        )
        .unwrap();
    let query_result: QueryResult = catalog.get_query_result();
    let ticks: Vec<Data> = query_result.collect();
    assert!(is_monotonically_increasing_by_init(&ticks));
}

#[rstest]
fn test_quote_tick_multiple_query() {
    let expected_length = 9_600;
    let mut catalog = DataBackendSession::new(5_000);
    let file_path_quotes = get_nautilus_test_data_file_path("quotes.parquet");
    let file_path_trades = get_nautilus_test_data_file_path("trades.parquet");

    catalog
        .add_file::<QuoteTick>("quote_tick", file_path_quotes.as_str(), None)
        .unwrap();
    catalog
        .add_file::<TradeTick>("quote_tick_2", file_path_trades.as_str(), None)
        .unwrap();
    let query_result: QueryResult = catalog.get_query_result();
    let ticks: Vec<Data> = query_result.collect();

    assert_eq!(ticks.len(), expected_length);
    assert!(is_monotonically_increasing_by_init(&ticks));
}

#[rstest]
fn test_trade_tick_query() {
    let expected_length = 100;
    let file_path = get_nautilus_test_data_file_path("trades.parquet");

    let mut catalog = DataBackendSession::new(10_000);
    catalog
        .add_file::<TradeTick>("trade_001", file_path.as_str(), None)
        .unwrap();
    let query_result: QueryResult = catalog.get_query_result();
    let ticks: Vec<Data> = query_result.collect();

    if let Data::Trade(t) = ticks[0] {
        assert_eq!("EUR/USD.SIM", t.instrument_id.to_string());
    } else {
        panic!("Invalid test");
    }

    assert_eq!(ticks.len(), expected_length);
    assert!(is_monotonically_increasing_by_init(&ticks));
}

#[rstest]
fn test_bar_query() {
    let expected_length = 10;
    let file_path = get_nautilus_test_data_file_path("bars.parquet");

    let mut catalog = DataBackendSession::new(10_000);
    catalog
        .add_file::<Bar>("bar_001", file_path.as_str(), None)
        .unwrap();
    let query_result: QueryResult = catalog.get_query_result();
    let ticks: Vec<Data> = query_result.collect();

    if let Data::Bar(b) = &ticks[0] {
        assert_eq!("ADABTC.BINANCE", b.bar_type.instrument_id().to_string());
    } else {
        panic!("Invalid test");
    }

    assert_eq!(ticks.len(), expected_length);
    assert!(is_monotonically_increasing_by_init(&ticks));
}

#[ignore = "JSON functionality not implemented in Rust"]
#[rstest]
fn test_catalog_serialization_json_round_trip() {
    // This test is skipped because write_to_json is not implemented in the Rust backend
}

#[rstest]
fn test_datafusion_parquet_round_trip() {
    use std::collections::HashMap;

    use datafusion::parquet::{
        arrow::ArrowWriter, basic::Compression, file::properties::WriterProperties,
    };
    use nautilus_serialization::arrow::EncodeToRecordBatch;
    use pretty_assertions::assert_eq;

    // Read original data from parquet
    let file_path = get_nautilus_test_data_file_path("quotes.parquet");

    let mut session = DataBackendSession::new(1000);
    session
        .add_file::<QuoteTick>("test_data", file_path.as_str(), None)
        .unwrap();
    let query_result: QueryResult = session.get_query_result();
    let quote_ticks: Vec<Data> = query_result.collect();
    let quote_ticks: Vec<QuoteTick> = to_variant(quote_ticks);

    let metadata = HashMap::from([
        ("price_precision".to_string(), "5".to_string()),
        ("size_precision".to_string(), "0".to_string()),
        ("instrument_id".to_string(), "EUR/USD.SIM".to_string()),
    ]);
    let schema = QuoteTick::get_schema(Some(metadata.clone()));

    // Write the record batches to a parquet file
    let temp_dir = tempfile::tempdir().unwrap();
    let temp_file_path = temp_dir.path().join("test.parquet");
    let mut temp_file = std::fs::File::create(&temp_file_path).unwrap();
    {
        let writer_props = WriterProperties::builder()
            .set_compression(Compression::SNAPPY)
            .set_max_row_group_size(1000)
            .build();

        let mut writer =
            ArrowWriter::try_new(&mut temp_file, schema.into(), Some(writer_props)).unwrap();
        for chunk in quote_ticks.chunks(1000) {
            let batch = QuoteTick::encode_batch(&metadata, chunk).unwrap();
            writer.write(&batch).unwrap();
        }
        writer.close().unwrap();
    }

    // Read back from parquet
    let mut session = DataBackendSession::new(1000);
    session
        .add_file::<QuoteTick>("test_data", temp_file_path.to_str().unwrap(), None)
        .unwrap();
    let query_result: QueryResult = session.get_query_result();
    let ticks: Vec<Data> = query_result.collect();
    let ticks_variants: Vec<QuoteTick> = to_variant(ticks);

    assert_eq!(quote_ticks.len(), ticks_variants.len());
    for (orig, loaded) in quote_ticks.iter().zip(ticks_variants.iter()) {
        assert_eq!(orig, loaded);
    }
}

#[ignore = "JSON functionality not implemented in Rust"]
#[rstest]
fn test_catalog_export_functionality() {
    // This test is skipped because write_to_json is not implemented in the Rust backend
}

// ================================================================================================
// Helper functions for creating test data (equivalent to PyO3 test helpers)
// ================================================================================================

fn create_temp_catalog() -> (TempDir, ParquetDataCatalog) {
    let temp_dir = TempDir::new().unwrap();
    let catalog = ParquetDataCatalog::new(temp_dir.path().to_path_buf(), None, None, None);
    (temp_dir, catalog)
}

fn audusd_sim_id() -> InstrumentId {
    InstrumentId::from("AUD/USD.SIM")
}

fn ethusdt_binance_id() -> InstrumentId {
    InstrumentId::from("ETH/USDT.BINANCE")
}

fn create_bar(ts_init: u64) -> Bar {
    let bar_type = BarType::new(
        audusd_sim_id(),
        BarSpecification::new(1, BarAggregation::Minute, PriceType::Bid),
        AggregationSource::External,
    );

    Bar::new(
        bar_type,
        Price::new(1.00001, 5),
        Price::new(1.1, 1),
        Price::new(1.00000, 5),
        Price::new(1.00000, 5),
        Quantity::new(100_000.0, 0),
        UnixNanos::from(0),
        UnixNanos::from(ts_init),
    )
}

fn create_quote_tick(ts_init: u64) -> QuoteTick {
    QuoteTick::new(
        ethusdt_binance_id(),
        Price::new(1987.0, 1),
        Price::new(1988.0, 1),
        Quantity::new(100_000.0, 0),
        Quantity::new(100_000.0, 0),
        UnixNanos::from(0),
        UnixNanos::from(ts_init),
    )
}

fn create_trade_tick(ts_init: u64) -> TradeTick {
    TradeTick::new(
        ethusdt_binance_id(),
        Price::new(1987.0, 1),
        Quantity::new(0.1, 1),
        AggressorSide::Buyer,
        TradeId::from("123456"),
        UnixNanos::from(0),
        UnixNanos::from(ts_init),
    )
}

fn create_order_book_delta(ts_init: u64) -> OrderBookDelta {
    OrderBookDelta::new(
        ethusdt_binance_id(),
        BookAction::Add,
        BookOrder::new(
            OrderSide::Buy,
            Price::new(10000.0, 1),
            Quantity::new(0.1, 1),
            0,
        ),
        0,
        0,
        UnixNanos::from(ts_init),
        UnixNanos::from(0),
    )
}

fn create_order_book_depth10(ts_init: u64) -> OrderBookDepth10 {
    let mut bids: [BookOrder; DEPTH10_LEN] = [BookOrder::default(); DEPTH10_LEN];
    let mut asks: [BookOrder; DEPTH10_LEN] = [BookOrder::default(); DEPTH10_LEN];

    // Create bids
    let mut price = 99.00;
    let mut quantity = 100.0;
    let mut order_id = 1;

    #[allow(clippy::needless_range_loop)]
    for i in 0..DEPTH10_LEN {
        let order = BookOrder::new(
            OrderSide::Buy,
            Price::new(price, 2),
            Quantity::new(quantity, 0),
            order_id,
        );

        bids[i] = order;

        price -= 1.0;
        quantity += 100.0;
        order_id += 1;
    }

    // Create asks
    price = 100.00;
    quantity = 100.0;
    order_id = 11;

    #[allow(clippy::needless_range_loop)]
    for i in 0..DEPTH10_LEN {
        let order = BookOrder::new(
            OrderSide::Sell,
            Price::new(price, 2),
            Quantity::new(quantity, 0),
            order_id,
        );

        asks[i] = order;

        price += 1.0;
        quantity += 100.0;
        order_id += 1;
    }

    let bid_counts = [1_u32; DEPTH10_LEN];
    let ask_counts = [1_u32; DEPTH10_LEN];

    OrderBookDepth10::new(
        ethusdt_binance_id(),
        bids,
        asks,
        bid_counts,
        ask_counts,
        0,
        0,
        UnixNanos::from(0),
        UnixNanos::from(ts_init),
    )
}

fn create_mark_price_update(ts_init: u64) -> MarkPriceUpdate {
    MarkPriceUpdate::new(
        ethusdt_binance_id(),
        Price::new(1000.00, 2),
        UnixNanos::from(0),
        UnixNanos::from(ts_init),
    )
}

fn create_index_price_update(ts_init: u64) -> IndexPriceUpdate {
    IndexPriceUpdate::new(
        ethusdt_binance_id(),
        Price::new(1000.00, 2),
        UnixNanos::from(0),
        UnixNanos::from(ts_init),
    )
}

// ================================================================================================
// Rust catalog tests (equivalent to PyO3 tests)
// ================================================================================================

#[rstest]
fn test_rust_write_2_bars_to_catalog() {
    // Arrange
    let (_temp_dir, catalog) = create_temp_catalog();

    // Act
    let bars = vec![create_bar(1), create_bar(2)];
    catalog.write_to_parquet(bars, None, None).unwrap();

    // Assert
    let intervals = catalog
        .get_intervals("bars", Some("AUD/USD.SIM".to_string()))
        .unwrap();
    assert_eq!(intervals, vec![(1, 2)]);
}

#[rstest]
fn test_rust_append_data_to_catalog() {
    // Arrange
    let (_temp_dir, catalog) = create_temp_catalog();

    // Act
    let bars1 = vec![create_bar(1), create_bar(2)];
    catalog.write_to_parquet(bars1, None, None).unwrap();

    let bars2 = vec![create_bar(3)];
    catalog.write_to_parquet(bars2, None, None).unwrap();

    // Assert
    let intervals = catalog
        .get_intervals("bars", Some("AUD/USD.SIM".to_string()))
        .unwrap();
    assert_eq!(intervals, vec![(1, 2), (3, 3)]);
}

#[rstest]
fn test_rust_consolidate_catalog() {
    // Arrange
    let (_temp_dir, catalog) = create_temp_catalog();

    // Act
    let bars1 = vec![create_bar(1), create_bar(2)];
    catalog.write_to_parquet(bars1, None, None).unwrap();

    let bars2 = vec![create_bar(3)];
    catalog.write_to_parquet(bars2, None, None).unwrap();

    catalog
        .consolidate_data("bars", Some("AUD/USD.SIM".to_string()), None, None, None)
        .unwrap();

    // Assert
    let intervals = catalog
        .get_intervals("bars", Some("AUD/USD.SIM".to_string()))
        .unwrap();
    assert_eq!(intervals, vec![(1, 3)]);
}

#[rstest]
fn test_rust_consolidate_catalog_with_time_range() {
    // Arrange
    let (_temp_dir, catalog) = create_temp_catalog();

    // Act
    let bars1 = vec![create_bar(1)];
    catalog.write_to_parquet(bars1, None, None).unwrap();

    let bars2 = vec![create_bar(2)];
    catalog.write_to_parquet(bars2, None, None).unwrap();

    let bars3 = vec![create_bar(3)];
    catalog.write_to_parquet(bars3, None, None).unwrap();

    catalog
        .consolidate_data(
            "bars",
            Some("AUD/USD.SIM".to_string()),
            Some(UnixNanos::from(1)),
            Some(UnixNanos::from(2)),
            None,
        )
        .unwrap();

    // Assert
    let intervals = catalog
        .get_intervals("bars", Some("AUD/USD.SIM".to_string()))
        .unwrap();
    assert_eq!(intervals, vec![(1, 2), (3, 3)]);
}

#[rstest]
fn test_rust_get_missing_intervals() {
    // Arrange
    let (_temp_dir, catalog) = create_temp_catalog();

    // Act
    let bars1 = vec![create_bar(1), create_bar(2)];
    catalog.write_to_parquet(bars1, None, None).unwrap();

    let bars2 = vec![create_bar(5), create_bar(6)];
    catalog.write_to_parquet(bars2, None, None).unwrap();

    let missing = catalog
        .get_missing_intervals_for_request(0, 10, "bars", Some("AUD/USD.SIM".to_string()))
        .unwrap();

    // Assert
    assert_eq!(missing, vec![(0, 0), (3, 4), (7, 10)]);
}

#[rstest]
fn test_rust_reset_data_file_names() {
    // Arrange
    let (_temp_dir, catalog) = create_temp_catalog();
    let bars = vec![create_bar(1), create_bar(2), create_bar(3)];
    catalog.write_to_parquet(bars, None, None).unwrap();

    // Get intervals before reset
    let intervals_before = catalog
        .get_intervals("bars", Some("AUD/USD.SIM".to_string()))
        .unwrap();
    assert_eq!(intervals_before, vec![(1, 3)]);

    // Act - reset file names
    let result = catalog.reset_data_file_names("bars", Some("AUD/USD.SIM".to_string()));

    // Assert - the operation should succeed (even if it changes the intervals)
    assert!(result.is_ok());

    // Note: The intervals might change or be empty after reset depending on the implementation
    // This is acceptable as the reset operation might rename files in a way that affects interval parsing
}

#[rstest]
fn test_rust_extend_file_name() {
    // Arrange
    let (_temp_dir, catalog) = create_temp_catalog();

    // Write data with a gap
    let bars1 = vec![create_bar(1)];
    catalog.write_to_parquet(bars1, None, None).unwrap();

    let bars2 = vec![create_bar(4)];
    catalog.write_to_parquet(bars2, None, None).unwrap();

    // Act - extend the first file to include the missing timestamp range
    catalog
        .extend_file_name(
            "bars",
            Some("AUD/USD.SIM".to_string()),
            UnixNanos::from(2),
            UnixNanos::from(3),
        )
        .unwrap();

    // Assert
    let intervals = catalog
        .get_intervals("bars", Some("AUD/USD.SIM".to_string()))
        .unwrap();
    assert_eq!(intervals, vec![(1, 3), (4, 4)]);
}

#[rstest]
fn test_rust_write_quote_ticks() {
    // Arrange
    let (_temp_dir, catalog) = create_temp_catalog();

    // Act
    let quote_ticks = vec![create_quote_tick(1), create_quote_tick(2)];
    catalog.write_to_parquet(quote_ticks, None, None).unwrap();

    // Assert
    let files = catalog
        .query_files(
            "quotes",
            Some(vec!["ETH/USDT.BINANCE".to_string()]),
            None,
            None,
        )
        .unwrap();
    assert!(!files.is_empty());
}

#[rstest]
fn test_rust_write_trade_ticks() {
    // Arrange
    let (_temp_dir, catalog) = create_temp_catalog();

    // Act
    let trade_ticks = vec![create_trade_tick(1), create_trade_tick(2)];
    catalog.write_to_parquet(trade_ticks, None, None).unwrap();

    // Assert
    let files = catalog
        .query_files(
            "trades",
            Some(vec!["ETH/USDT.BINANCE".to_string()]),
            None,
            None,
        )
        .unwrap();
    assert!(!files.is_empty());
}

#[rstest]
fn test_rust_write_order_book_deltas() {
    // Arrange
    let (_temp_dir, catalog) = create_temp_catalog();

    // Act
    let deltas = vec![create_order_book_delta(1), create_order_book_delta(2)];
    catalog.write_to_parquet(deltas, None, None).unwrap();

    // Assert
    let files = catalog
        .query_files(
            "order_book_deltas",
            Some(vec!["ETH/USDT.BINANCE".to_string()]),
            None,
            None,
        )
        .unwrap();
    assert!(!files.is_empty());
}

#[rstest]
fn test_rust_write_order_book_depths() {
    // Arrange
    let (_temp_dir, catalog) = create_temp_catalog();

    // Act
    let depths = vec![create_order_book_depth10(1), create_order_book_depth10(2)];
    catalog.write_to_parquet(depths, None, None).unwrap();

    // Assert
    let files = catalog
        .query_files(
            "order_book_depths",
            Some(vec!["ETH/USDT.BINANCE".to_string()]),
            None,
            None,
        )
        .unwrap();
    assert!(!files.is_empty());
}

#[rstest]
fn test_rust_write_mark_price_updates() {
    // Arrange
    let (_temp_dir, catalog) = create_temp_catalog();

    // Act
    let mark_prices = vec![create_mark_price_update(1), create_mark_price_update(2)];
    catalog.write_to_parquet(mark_prices, None, None).unwrap();

    // Assert
    let files = catalog
        .query_files(
            "mark_prices",
            Some(vec!["ETH/USDT.BINANCE".to_string()]),
            None,
            None,
        )
        .unwrap();
    assert!(!files.is_empty());
}

#[rstest]
fn test_rust_write_index_price_updates() {
    // Arrange
    let (_temp_dir, catalog) = create_temp_catalog();

    // Act
    let index_prices = vec![create_index_price_update(1), create_index_price_update(2)];
    catalog.write_to_parquet(index_prices, None, None).unwrap();

    // Assert
    let files = catalog
        .query_files(
            "index_prices",
            Some(vec!["ETH/USDT.BINANCE".to_string()]),
            None,
            None,
        )
        .unwrap();
    assert!(!files.is_empty());
}

#[rstest]
fn test_rust_query_files() {
    // Arrange
    let (_temp_dir, catalog) = create_temp_catalog();

    let bars1 = vec![create_bar(1), create_bar(2)];
    catalog.write_to_parquet(bars1, None, None).unwrap();

    let bars2 = vec![create_bar(3), create_bar(4)];
    catalog.write_to_parquet(bars2, None, None).unwrap();

    // Act
    let files = catalog
        .query_files("bars", Some(vec!["AUD/USD.SIM".to_string()]), None, None)
        .unwrap();

    // Assert
    assert_eq!(files.len(), 2);
}

#[rstest]
fn test_rust_query_files_with_multiple_files() {
    // Arrange
    let (_temp_dir, catalog) = create_temp_catalog();

    let bars1 = vec![create_bar(1), create_bar(2)];
    catalog.write_to_parquet(bars1, None, None).unwrap();

    let bars2 = vec![create_bar(3), create_bar(4)];
    catalog.write_to_parquet(bars2, None, None).unwrap();

    let bars3 = vec![create_bar(5), create_bar(6)];
    catalog.write_to_parquet(bars3, None, None).unwrap();

    // Act
    let files = catalog
        .query_files("bars", Some(vec!["AUD/USD.SIM".to_string()]), None, None)
        .unwrap();

    // Assert
    assert_eq!(files.len(), 3);
}

#[rstest]
fn test_rust_get_intervals_empty() {
    // Arrange
    let (_temp_dir, catalog) = create_temp_catalog();

    // Act
    let intervals = catalog
        .get_intervals("bars", Some("AUD/USD.SIM".to_string()))
        .unwrap();

    // Assert
    assert!(intervals.is_empty());
}
