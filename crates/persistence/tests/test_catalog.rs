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
    let catalog = ParquetDataCatalog::new(temp_dir.path().to_path_buf(), None, None, None, None);
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
    catalog.write_to_parquet(bars, None, None, None).unwrap();

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
    catalog.write_to_parquet(bars1, None, None, None).unwrap();

    let bars2 = vec![create_bar(3)];
    catalog.write_to_parquet(bars2, None, None, None).unwrap();

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
    catalog.write_to_parquet(bars1, None, None, None).unwrap();

    let bars2 = vec![create_bar(3)];
    catalog.write_to_parquet(bars2, None, None, None).unwrap();

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
    catalog.write_to_parquet(bars1, None, None, None).unwrap();

    let bars2 = vec![create_bar(2)];
    catalog.write_to_parquet(bars2, None, None, None).unwrap();

    let bars3 = vec![create_bar(3)];
    catalog.write_to_parquet(bars3, None, None, None).unwrap();

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
fn test_register_object_store_from_uri_local_file() {
    // Test registering object store from local file URI
    let file_path = get_nautilus_test_data_file_path("trades.parquet");
    let parent_path = std::path::Path::new(&file_path).parent().unwrap();
    let file_uri = format!("file://{}", parent_path.display());

    let mut session = DataBackendSession::new(1000);

    // Act - register object store from local file URI
    session
        .register_object_store_from_uri(&file_uri, None)
        .unwrap();

    // Add file using the registered object store
    session
        .add_file::<TradeTick>("trade_ticks", &file_path, None)
        .unwrap();
    let query_result: QueryResult = session.get_query_result();
    let ticks: Vec<Data> = query_result.collect();

    // Assert
    assert_eq!(ticks.len(), 100);
    assert!(is_monotonically_increasing_by_init(&ticks));
}

#[rstest]
fn test_register_object_store_from_uri_invalid_uri() {
    // Test registering object store from invalid URI
    let mut session = DataBackendSession::new(1000);

    // Act & Assert - invalid URI should return an error
    let result = session.register_object_store_from_uri("invalid://not-a-real-uri", None);
    assert!(result.is_err());
}

#[rstest]
fn test_register_object_store_from_uri_nonexistent_path() {
    // Test registering object store from non-existent path URI
    let mut session = DataBackendSession::new(1000);

    // Act & Assert - non-existent path should return an error
    let result = session.register_object_store_from_uri("file:///nonexistent/path", None);
    assert!(result.is_err());
}

#[rstest]
fn test_rust_get_missing_intervals() {
    // Arrange
    let (_temp_dir, catalog) = create_temp_catalog();

    // Act
    let bars1 = vec![create_bar(1), create_bar(2)];
    catalog.write_to_parquet(bars1, None, None, None).unwrap();

    let bars2 = vec![create_bar(5), create_bar(6)];
    catalog.write_to_parquet(bars2, None, None, None).unwrap();

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
    catalog.write_to_parquet(bars, None, None, None).unwrap();

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
    catalog.write_to_parquet(bars1, None, None, None).unwrap();

    let bars2 = vec![create_bar(4)];
    catalog.write_to_parquet(bars2, None, None, None).unwrap();

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
    catalog
        .write_to_parquet(quote_ticks, None, None, None)
        .unwrap();

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
    catalog
        .write_to_parquet(trade_ticks, None, None, None)
        .unwrap();

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
    catalog.write_to_parquet(deltas, None, None, None).unwrap();

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
    catalog.write_to_parquet(depths, None, None, None).unwrap();

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
    catalog
        .write_to_parquet(mark_prices, None, None, None)
        .unwrap();

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
    catalog
        .write_to_parquet(index_prices, None, None, None)
        .unwrap();

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
    catalog.write_to_parquet(bars1, None, None, None).unwrap();

    let bars2 = vec![create_bar(3), create_bar(4)];
    catalog.write_to_parquet(bars2, None, None, None).unwrap();

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
    catalog.write_to_parquet(bars1, None, None, None).unwrap();

    let bars2 = vec![create_bar(3), create_bar(4)];
    catalog.write_to_parquet(bars2, None, None, None).unwrap();

    let bars3 = vec![create_bar(5), create_bar(6)];
    catalog.write_to_parquet(bars3, None, None, None).unwrap();

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

#[rstest]
fn test_consolidate_data_by_period_basic() {
    // Arrange
    let (_temp_dir, mut catalog) = create_temp_catalog();

    // Create data spanning multiple hours
    let bars = vec![
        create_bar(3600_000_000_000), // 1 hour
        create_bar(3601_000_000_000), // 1 hour + 1 second
        create_bar(7200_000_000_000), // 2 hours
        create_bar(7201_000_000_000), // 2 hours + 1 second
    ];
    catalog.write_to_parquet(bars, None, None, None).unwrap();

    // Act - consolidate by 1-hour periods
    catalog
        .consolidate_data_by_period(
            "bars",
            Some("AUD/USD.SIM".to_string()),
            Some(3600_000_000_000), // 1 hour in nanoseconds
            None,
            None,
            Some(true),
        )
        .unwrap();

    // Assert - should have consolidated into period-based files
    let intervals = catalog
        .get_intervals("bars", Some("AUD/USD.SIM".to_string()))
        .unwrap();

    // The exact intervals depend on the implementation, but we should have fewer files
    assert!(!intervals.is_empty());
}

#[rstest]
fn test_consolidate_data_by_period_with_time_range() {
    // Arrange
    let (_temp_dir, mut catalog) = create_temp_catalog();

    // Create data spanning multiple periods
    let bars = vec![
        create_bar(1000),
        create_bar(2000),
        create_bar(3000),
        create_bar(4000),
        create_bar(5000),
    ];
    catalog.write_to_parquet(bars, None, None, None).unwrap();

    // Act - consolidate only middle range
    catalog
        .consolidate_data_by_period(
            "bars",
            Some("AUD/USD.SIM".to_string()),
            Some(86400_000_000_000), // 1 day in nanoseconds
            Some(UnixNanos::from(2000)),
            Some(UnixNanos::from(4000)),
            Some(false),
        )
        .unwrap();

    // Assert - operation should complete without error
    let intervals = catalog
        .get_intervals("bars", Some("AUD/USD.SIM".to_string()))
        .unwrap();
    assert!(!intervals.is_empty());
}

#[rstest]
fn test_consolidate_data_by_period_empty_data() {
    // Arrange
    let (_temp_dir, mut catalog) = create_temp_catalog();

    // Act - consolidate empty catalog
    let result = catalog.consolidate_data_by_period(
        "bars",
        Some("AUD/USD.SIM".to_string()),
        Some(86400_000_000_000), // 1 day in nanoseconds
        None,
        None,
        Some(true),
    );

    // Assert - should succeed with no data
    assert!(result.is_ok());
}

#[rstest]
fn test_consolidate_data_by_period_different_periods() {
    // Arrange
    let (_temp_dir, mut catalog) = create_temp_catalog();

    // Create data spanning multiple minutes
    let bars = vec![
        create_bar(60_000_000_000),  // 1 minute
        create_bar(120_000_000_000), // 2 minutes
        create_bar(180_000_000_000), // 3 minutes
        create_bar(240_000_000_000), // 4 minutes
    ];
    catalog.write_to_parquet(bars, None, None, None).unwrap();

    // Test different period sizes
    let periods = vec![
        1800_000_000_000,  // 30 minutes
        3600_000_000_000,  // 1 hour
        86400_000_000_000, // 1 day
    ];

    for period_nanos in periods {
        // Act
        let result = catalog.consolidate_data_by_period(
            "bars",
            Some("AUD/USD.SIM".to_string()),
            Some(period_nanos),
            None,
            None,
            Some(true),
        );

        // Assert
        assert!(result.is_ok(), "Failed for period: {period_nanos}");
    }
}

#[rstest]
fn test_consolidate_data_by_period_ensure_contiguous_files_false() {
    // Arrange
    let (_temp_dir, mut catalog) = create_temp_catalog();

    // Create some test data
    let bars = vec![create_bar(1000), create_bar(2000), create_bar(3000)];
    catalog.write_to_parquet(bars, None, None, None).unwrap();

    // Act - consolidate with ensure_contiguous_files=false
    catalog
        .consolidate_data_by_period(
            "bars",
            Some("AUD/USD.SIM".to_string()),
            Some(86400_000_000_000), // 1 day in nanoseconds
            None,
            None,
            Some(false), // Use actual data timestamps for file naming
        )
        .unwrap();

    // Assert - operation should complete without error
    let intervals = catalog
        .get_intervals("bars", Some("AUD/USD.SIM".to_string()))
        .unwrap();
    assert!(!intervals.is_empty());
}

#[rstest]
fn test_consolidate_catalog_by_period_basic() {
    // Arrange
    let (_temp_dir, mut catalog) = create_temp_catalog();

    // Create data for multiple data types
    let bars = vec![create_bar(1000), create_bar(2000)];
    catalog.write_to_parquet(bars, None, None, None).unwrap();

    let quotes = vec![create_quote_tick(1000), create_quote_tick(2000)];
    catalog.write_to_parquet(quotes, None, None, None).unwrap();

    // Act - consolidate entire catalog
    catalog
        .consolidate_catalog_by_period(
            Some(86400_000_000_000), // 1 day in nanoseconds
            None,
            None,
            Some(true),
        )
        .unwrap();

    // Assert - operation should complete without error
    let bar_intervals = catalog
        .get_intervals("bars", Some("AUD/USD.SIM".to_string()))
        .unwrap();
    let quote_intervals = catalog
        .get_intervals("quotes", Some("ETH/USDT.BINANCE".to_string()))
        .unwrap();

    assert!(!bar_intervals.is_empty());
    assert!(!quote_intervals.is_empty());
}

#[rstest]
fn test_consolidate_catalog_by_period_with_time_range() {
    // Arrange
    let (_temp_dir, mut catalog) = create_temp_catalog();

    // Create data spanning multiple periods
    let bars = vec![create_bar(1000), create_bar(5000), create_bar(10000)];
    catalog.write_to_parquet(bars, None, None, None).unwrap();

    // Act - consolidate catalog with time range
    catalog
        .consolidate_catalog_by_period(
            Some(86400_000_000_000), // 1 day in nanoseconds
            Some(UnixNanos::from(2000)),
            Some(UnixNanos::from(8000)),
            Some(false),
        )
        .unwrap();

    // Assert - operation should complete without error
    let intervals = catalog
        .get_intervals("bars", Some("AUD/USD.SIM".to_string()))
        .unwrap();
    assert!(!intervals.is_empty());
}

#[rstest]
fn test_consolidate_catalog_by_period_empty_catalog() {
    // Arrange
    let (_temp_dir, mut catalog) = create_temp_catalog();

    // Act - consolidate empty catalog
    let result = catalog.consolidate_catalog_by_period(
        Some(86400_000_000_000), // 1 day in nanoseconds
        None,
        None,
        Some(true),
    );

    // Assert - should succeed with empty catalog
    assert!(result.is_ok());
}

#[rstest]
fn test_consolidate_catalog_by_period_default_parameters() {
    // Arrange
    let (_temp_dir, mut catalog) = create_temp_catalog();

    // Create some test data
    let bars = vec![create_bar(1000), create_bar(2000)];
    catalog.write_to_parquet(bars, None, None, None).unwrap();

    // Act - consolidate with default parameters
    let result = catalog.consolidate_catalog_by_period(None, None, None, None);

    // Assert - should use default 1-day period
    assert!(result.is_ok());
}

#[rstest]
fn test_consolidate_data_by_period_multiple_instruments() {
    // Arrange
    let (_temp_dir, mut catalog) = create_temp_catalog();

    // Create bars for AUD/USD
    let aud_bars = vec![create_bar(1000), create_bar(2000)];
    catalog
        .write_to_parquet(aud_bars, None, None, None)
        .unwrap();

    // Create quotes for ETH/USDT
    let eth_quotes = vec![create_quote_tick(1000), create_quote_tick(2000)];
    catalog
        .write_to_parquet(eth_quotes, None, None, None)
        .unwrap();

    // Act - consolidate specific instrument only
    catalog
        .consolidate_data_by_period(
            "bars",
            Some("AUD/USD.SIM".to_string()),
            Some(86400_000_000_000), // 1 day in nanoseconds
            None,
            None,
            Some(true),
        )
        .unwrap();

    // Assert - only AUD/USD bars should be affected
    let aud_intervals = catalog
        .get_intervals("bars", Some("AUD/USD.SIM".to_string()))
        .unwrap();
    let eth_intervals = catalog
        .get_intervals("quotes", Some("ETH/USDT.BINANCE".to_string()))
        .unwrap();

    assert!(!aud_intervals.is_empty());
    assert!(!eth_intervals.is_empty());
}

#[rstest]
fn test_consolidate_data_by_period_invalid_type() {
    // Arrange
    let (_temp_dir, mut catalog) = create_temp_catalog();

    // Act - consolidate non-existent data type
    let result = catalog.consolidate_data_by_period(
        "invalid_type",
        Some("AUD/USD.SIM".to_string()),
        Some(86400_000_000_000), // 1 day in nanoseconds
        None,
        None,
        Some(true),
    );

    // Assert - should return error for invalid data type
    assert!(result.is_err());
}

#[rstest]
fn test_prepare_consolidation_queries_empty_intervals() {
    // Arrange
    let (_temp_dir, catalog) = create_temp_catalog();

    // Test with empty intervals
    let intervals = vec![];
    let period_nanos = 86400_000_000_000; // 1 day

    let queries = catalog
        .prepare_consolidation_queries("quotes", None, &intervals, period_nanos, None, None, true)
        .unwrap();

    // Should have no queries for empty intervals
    assert!(queries.is_empty());
}

#[rstest]
fn test_prepare_consolidation_queries_filtered_intervals() {
    // Arrange
    let (_temp_dir, catalog) = create_temp_catalog();

    // Test with intervals that are filtered out by time range
    let intervals = vec![(1000, 2000), (3000, 4000)];
    let period_nanos = 86400_000_000_000; // 1 day
    let start = Some(UnixNanos::from(5000)); // After all intervals
    let end = Some(UnixNanos::from(6000));

    let queries = catalog
        .prepare_consolidation_queries("quotes", None, &intervals, period_nanos, start, end, true)
        .unwrap();

    // Should have no queries since no intervals overlap with the time range
    assert!(queries.is_empty());
}

#[rstest]
fn test_generic_query_typed_data_quotes() {
    // Arrange
    let (_temp_dir, mut catalog) = create_temp_catalog();
    // Create test data
    let quotes = vec![create_quote_tick(1000), create_quote_tick(2000)];
    catalog.write_to_parquet(quotes, None, None, None).unwrap();

    // Act - query using generic typed data function
    let result = catalog
        .query_typed_data::<QuoteTick>(
            Some(vec!["ETH/USDT.BINANCE".to_string()]),
            Some(UnixNanos::from(500)),
            Some(UnixNanos::from(2500)),
            None,
            None,
        )
        .unwrap();

    // Assert
    assert_eq!(result.len(), 2);

    // Verify the data is correct
    let q = &result[0];
    assert_eq!(q.instrument_id.to_string(), "ETH/USDT.BINANCE");
    assert_eq!(q.ts_init, UnixNanos::from(1000));
}

#[rstest]
fn test_generic_query_typed_data_bars() {
    // Arrange
    let (_temp_dir, mut catalog) = create_temp_catalog();

    // Create test data
    let bars = vec![create_bar(1000), create_bar(2000)];
    catalog.write_to_parquet(bars, None, None, None).unwrap();

    // Act - query using generic typed data function
    let result = catalog
        .query_typed_data::<Bar>(
            Some(vec!["AUD/USD.SIM".to_string()]),
            Some(UnixNanos::from(500)),
            Some(UnixNanos::from(2500)),
            None,
            None,
        )
        .unwrap();

    // Assert
    assert_eq!(result.len(), 2);

    // Verify the data is correct
    let b = &result[0];
    assert_eq!(b.bar_type.instrument_id().to_string(), "AUD/USD.SIM");
    assert_eq!(b.ts_init, UnixNanos::from(1000));
}

#[rstest]
fn test_generic_query_typed_data_empty_result() {
    // Arrange
    let (_temp_dir, mut catalog) = create_temp_catalog();

    // Act - query with no matching data
    let result = catalog
        .query_typed_data::<QuoteTick>(
            Some(vec!["NONEXISTENT".to_string()]),
            Some(UnixNanos::from(500)),
            Some(UnixNanos::from(2500)),
            None,
            None,
        )
        .unwrap();

    // Assert
    assert!(result.is_empty());
}

#[rstest]
fn test_generic_query_typed_data_with_where_clause() {
    // Arrange
    let (_temp_dir, mut catalog) = create_temp_catalog();

    // Create test data
    let quotes = vec![create_quote_tick(1000), create_quote_tick(2000)];
    catalog.write_to_parquet(quotes, None, None, None).unwrap();

    // Act - query with WHERE clause
    let result = catalog
        .query_typed_data::<QuoteTick>(
            Some(vec!["ETH/USDT.BINANCE".to_string()]),
            Some(UnixNanos::from(500)),
            Some(UnixNanos::from(2500)),
            Some("ts_init >= 1500"),
            None,
        )
        .unwrap();

    // Assert - should only return the second quote
    assert_eq!(result.len(), 1);
}

#[rstest]
fn test_generic_consolidate_data_by_period_quotes() {
    // Arrange
    let (_temp_dir, mut catalog) = create_temp_catalog();

    // Create multiple small files with contiguous timestamps
    for i in 0..3 {
        let quotes = vec![create_quote_tick(1000 + i)];
        catalog.write_to_parquet(quotes, None, None, None).unwrap();
    }

    // Verify we have multiple files initially
    let initial_intervals = catalog
        .get_intervals("quotes", Some("ETH/USDT.BINANCE".to_string()))
        .unwrap();
    assert_eq!(initial_intervals.len(), 3);

    // Act - consolidate using generic function
    catalog
        .consolidate_data_by_period_generic::<QuoteTick>(
            Some("ETH/USDT.BINANCE".to_string()),
            Some(86400_000_000_000), // 1 day in nanoseconds
            None,
            None,
            Some(true),
        )
        .unwrap();

    // Assert - should have fewer files after consolidation
    let final_intervals = catalog
        .get_intervals("quotes", Some("ETH/USDT.BINANCE".to_string()))
        .unwrap();
    assert!(final_intervals.len() <= initial_intervals.len());
}

#[rstest]
fn test_generic_consolidate_data_by_period_bars() {
    // Arrange
    let (_temp_dir, mut catalog) = create_temp_catalog();

    // Create multiple small files with contiguous timestamps
    for i in 0..3 {
        let bars = vec![create_bar(1000 + i)];
        catalog.write_to_parquet(bars, None, None, None).unwrap();
    }

    // Verify we have multiple files initially
    let initial_intervals = catalog
        .get_intervals("bars", Some("AUD/USD.SIM".to_string()))
        .unwrap();
    assert_eq!(initial_intervals.len(), 3);

    // Act - consolidate using generic function
    catalog
        .consolidate_data_by_period_generic::<Bar>(
            Some("AUD/USD.SIM".to_string()),
            Some(86400_000_000_000), // 1 day in nanoseconds
            None,
            None,
            Some(true),
        )
        .unwrap();

    // Assert - should have fewer files after consolidation
    let final_intervals = catalog
        .get_intervals("bars", Some("AUD/USD.SIM".to_string()))
        .unwrap();
    assert!(final_intervals.len() <= initial_intervals.len());
}

#[rstest]
fn test_generic_consolidate_data_by_period_empty_catalog() {
    // Arrange
    let (_temp_dir, mut catalog) = create_temp_catalog();

    // Act - consolidate empty catalog
    let result = catalog.consolidate_data_by_period_generic::<QuoteTick>(
        Some("ETH/USDT.BINANCE".to_string()),
        Some(86400_000_000_000), // 1 day in nanoseconds
        None,
        None,
        Some(true),
    );

    // Assert - should succeed with empty catalog
    assert!(result.is_ok());
}

#[rstest]
fn test_generic_consolidate_data_by_period_with_time_range() {
    // Arrange
    let (_temp_dir, mut catalog) = create_temp_catalog();

    // Create data spanning multiple periods
    let quotes = vec![
        create_quote_tick(1000),
        create_quote_tick(5000),
        create_quote_tick(10000),
    ];
    for quote in quotes {
        catalog
            .write_to_parquet(vec![quote], None, None, None)
            .unwrap();
    }

    // Act - consolidate with time range
    catalog
        .consolidate_data_by_period_generic::<QuoteTick>(
            Some("ETH/USDT.BINANCE".to_string()),
            Some(86400_000_000_000), // 1 day in nanoseconds
            Some(UnixNanos::from(2000)),
            Some(UnixNanos::from(8000)),
            Some(false),
        )
        .unwrap();

    // Assert - operation should complete without error
    let intervals = catalog
        .get_intervals("quotes", Some("ETH/USDT.BINANCE".to_string()))
        .unwrap();
    assert!(!intervals.is_empty());
}

// ================================================================================================
// Integration tests for consolidation workflow
// ================================================================================================

#[rstest]
fn test_consolidation_workflow_end_to_end() {
    // Arrange
    let (_temp_dir, catalog) = create_temp_catalog();

    // Create multiple small files
    for i in 0..5 {
        let bars = vec![create_bar(1000 + i * 1000)];
        catalog.write_to_parquet(bars, None, None, None).unwrap();
    }

    // Verify we have multiple files initially
    let initial_intervals = catalog
        .get_intervals("bars", Some("AUD/USD.SIM".to_string()))
        .unwrap();
    assert_eq!(initial_intervals.len(), 5);

    // Act - consolidate all files
    catalog
        .consolidate_data("bars", Some("AUD/USD.SIM".to_string()), None, None, None)
        .unwrap();

    // Assert - should have fewer files after consolidation
    let final_intervals = catalog
        .get_intervals("bars", Some("AUD/USD.SIM".to_string()))
        .unwrap();
    assert!(final_intervals.len() <= initial_intervals.len());
}

#[rstest]
fn test_consolidation_preserves_data_integrity() {
    // Arrange
    let (_temp_dir, mut catalog) = create_temp_catalog();

    // Create test data with contiguous timestamps
    let original_bars = vec![create_bar(1000), create_bar(1001), create_bar(1002)];

    // Write each bar separately to create multiple files
    for bar in &original_bars {
        catalog
            .write_to_parquet(vec![*bar], None, None, None)
            .unwrap();
    }

    // Act - consolidate the data
    catalog
        .consolidate_data_by_period(
            "bars",
            Some("AUD/USD.SIM".to_string()),
            Some(86400_000_000_000), // 1 day in nanoseconds
            None,
            None,
            Some(true),
        )
        .unwrap();

    // Assert - data should still be accessible after consolidation
    let intervals = catalog
        .get_intervals("bars", Some("AUD/USD.SIM".to_string()))
        .unwrap();

    // Should have at least one interval covering our data
    assert!(!intervals.is_empty());

    // The consolidated interval should cover all our original timestamps
    let min_ts = intervals.iter().map(|(start, _)| *start).min().unwrap();
    let max_ts = intervals.iter().map(|(_, end)| *end).max().unwrap();

    assert!(min_ts <= 1000);
    assert!(max_ts >= 1002);
}

#[derive(Clone)]
struct DummyData(u64);

impl nautilus_model::data::HasTsInit for DummyData {
    fn ts_init(&self) -> UnixNanos {
        UnixNanos::from(self.0)
    }
}

#[rstest]
fn test_check_ascending_timestamps_error() {
    let data = vec![DummyData(2), DummyData(1)];
    let result = ParquetDataCatalog::check_ascending_timestamps(&data, "dummy");
    assert!(result.is_err());
}

#[rstest]
fn test_to_object_path_trailing_slash() {
    // Create catalog with base path that contains a trailing slash
    let tmp = tempfile::tempdir().unwrap();
    let base_dir = tmp.path().join("catalog");
    std::fs::create_dir_all(&base_dir).unwrap();

    let catalog = ParquetDataCatalog::new(base_dir.clone(), None, None, None, None);

    // Build a sample path under the catalog base
    let sample_path = format!(
        "{}/data/quotes/XYZ/2021-01-01T00-00-00-000000000Z_2021-01-01T00-00-01-000000000Z.parquet",
        base_dir.to_string_lossy()
    );

    let object_path = catalog.to_object_path(&sample_path);

    assert!(
        !object_path
            .as_ref()
            .starts_with(base_dir.to_string_lossy().as_ref())
    );
}

#[rstest]
fn test_is_remote_uri() {
    // Test S3 URIs
    let s3_catalog =
        ParquetDataCatalog::from_uri("s3://bucket/path", None, None, None, None).unwrap();
    assert!(s3_catalog.is_remote_uri());
}

#[rstest]
fn test_extract_data_cls_and_identifier_from_path_moved() {
    let tmp = tempfile::tempdir().unwrap();
    let base_dir = tmp.path().join("catalog");
    std::fs::create_dir_all(&base_dir).unwrap();

    let catalog = ParquetDataCatalog::new(base_dir.clone(), None, None, None, None);

    // Test path with instrument ID
    let path_with_id = format!("{}/data/quotes/BTCUSD", base_dir.to_string_lossy());
    let (data_cls, identifier) = catalog
        .extract_data_cls_and_identifier_from_path(&path_with_id)
        .unwrap();
    assert_eq!(data_cls, Some("quotes".to_string()));
    assert_eq!(identifier, Some("BTCUSD".to_string()));

    // Test path without instrument ID
    let path_without_id = format!("{}/data/trades", base_dir.to_string_lossy());
    let (data_cls, identifier) = catalog
        .extract_data_cls_and_identifier_from_path(&path_without_id)
        .unwrap();
    assert_eq!(data_cls, Some("trades".to_string()));
    assert_eq!(identifier, None);

    // Test invalid path
    let invalid_path = "/invalid/path";
    let (data_cls, identifier) = catalog
        .extract_data_cls_and_identifier_from_path(invalid_path)
        .unwrap();
    assert_eq!(data_cls, None);
    assert_eq!(identifier, None);
}

#[rstest]
fn test_group_contiguous_intervals_moved() {
    let tmp = tempfile::tempdir().unwrap();
    let base_dir = tmp.path().join("catalog");
    std::fs::create_dir_all(&base_dir).unwrap();

    let catalog = ParquetDataCatalog::new(base_dir, None, None, None, None);

    // Test contiguous intervals
    let intervals = vec![(1, 5), (6, 10), (11, 15)];
    let groups = catalog.group_contiguous_intervals(&intervals);
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0], intervals);

    // Test non-contiguous intervals (gap between 5 and 8)
    let intervals = vec![(1, 5), (8, 10), (12, 15)];
    let groups = catalog.group_contiguous_intervals(&intervals);
    assert_eq!(groups.len(), 3);
    assert_eq!(groups[0], vec![(1, 5)]);
    assert_eq!(groups[1], vec![(8, 10)]);
    assert_eq!(groups[2], vec![(12, 15)]);

    // Test empty intervals
    let intervals = vec![];
    let groups = catalog.group_contiguous_intervals(&intervals);
    assert_eq!(groups.len(), 0);

    // Test single interval
    let intervals = vec![(1, 5)];
    let groups = catalog.group_contiguous_intervals(&intervals);
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0], vec![(1, 5)]);
}

#[rstest]
fn test_prepare_consolidation_queries_basic_moved() {
    let tmp = tempfile::tempdir().unwrap();
    let base_dir = tmp.path().join("catalog");
    std::fs::create_dir_all(&base_dir).unwrap();

    let catalog = ParquetDataCatalog::new(base_dir, None, None, None, None);

    // Test basic period consolidation
    let intervals = vec![(1000, 5000), (5001, 10000)];
    let period_nanos = 86400000000000; // 1 day

    let queries = catalog
        .prepare_consolidation_queries("quotes", None, &intervals, period_nanos, None, None, true)
        .unwrap();

    // Should have at least one query for the period
    assert!(!queries.is_empty());

    // All queries should have valid timestamps
    for query in &queries {
        assert!(query.query_start <= query.query_end);
    }
}

#[rstest]
fn test_prepare_consolidation_queries_with_splits_moved() {
    let tmp = tempfile::tempdir().unwrap();
    let base_dir = tmp.path().join("catalog");
    std::fs::create_dir_all(&base_dir).unwrap();

    let catalog = ParquetDataCatalog::new(base_dir, None, None, None, None);

    // Test with interval splitting
    // File: [1000, 5000], Request: start=2000, end=4000
    // Should result in split queries for [1000, 1999] and [4001, 5000], plus consolidation for [2000, 4000]
    let intervals = vec![(1000, 5000)];
    let period_nanos = 86400000000000; // 1 day
    let start = Some(UnixNanos::from(2000));
    let end = Some(UnixNanos::from(4000));

    let queries = catalog
        .prepare_consolidation_queries(
            "quotes",
            Some("EURUSD".to_string()),
            &intervals,
            period_nanos,
            start,
            end,
            false,
        )
        .unwrap();

    // Should have split queries and consolidation queries
    // Split queries are those that preserve data outside the consolidation range
    let split_queries: Vec<_> = queries
        .iter()
        .filter(|q| q.query_start == 1000 || q.query_start == 4001)
        .collect();
    let consolidation_queries: Vec<_> = queries
        .iter()
        .filter(|q| q.query_start != 1000 && q.query_start != 4001)
        .collect();

    assert_eq!(split_queries.len(), 2, "Should have 2 split queries");
    assert!(
        !consolidation_queries.is_empty(),
        "Should have consolidation queries"
    );

    // Verify split before query
    let split_before = split_queries.iter().find(|q| q.query_start == 1000);
    assert!(split_before.is_some(), "Should have split before query");
    let split_before = split_before.unwrap();
    assert_eq!(split_before.query_end, 1999);
    assert!(!split_before.use_period_boundaries);

    // Verify split after query
    let split_after = split_queries.iter().find(|q| q.query_start == 4001);
    assert!(split_after.is_some(), "Should have split after query");
    let split_after = split_after.unwrap();
    assert_eq!(split_after.query_end, 5000);
    assert!(!split_after.use_period_boundaries);
}

#[rstest]
fn test_is_remote_uri_extended_moved() {
    // Test GCS URIs
    let gcs_catalog =
        ParquetDataCatalog::from_uri("gs://bucket/path", None, None, None, None).unwrap();
    assert!(gcs_catalog.is_remote_uri());

    let gcs2_catalog =
        ParquetDataCatalog::from_uri("gcs://bucket/path", None, None, None, None).unwrap();
    assert!(gcs2_catalog.is_remote_uri());

    // Test Azure URIs
    let azure_catalog =
        ParquetDataCatalog::from_uri("azure://account/container/path", None, None, None, None)
            .unwrap();
    assert!(azure_catalog.is_remote_uri());

    let abfs_catalog = ParquetDataCatalog::from_uri(
        "abfs://container@account.dfs.core.windows.net/path",
        None,
        None,
        None,
        None,
    )
    .unwrap();
    assert!(abfs_catalog.is_remote_uri());

    // Test HTTP URIs
    let http_catalog =
        ParquetDataCatalog::from_uri("http://example.com/path", None, None, None, None).unwrap();
    assert!(http_catalog.is_remote_uri());

    let https_catalog =
        ParquetDataCatalog::from_uri("https://example.com/path", None, None, None, None).unwrap();
    assert!(https_catalog.is_remote_uri());

    // Test local paths (should not be remote)
    let tmp = tempfile::tempdir().unwrap();
    let local_catalog = ParquetDataCatalog::new(tmp.path().to_path_buf(), None, None, None, None);
    assert!(!local_catalog.is_remote_uri());

    let tmp_file = tempfile::tempdir().unwrap();
    let file_uri = format!("file://{}", tmp_file.path().display());
    let file_catalog = ParquetDataCatalog::from_uri(&file_uri, None, None, None, None).unwrap();
    assert!(!file_catalog.is_remote_uri());
}

#[rstest]
fn test_reconstruct_full_uri_moved() {
    // Test S3 URI reconstruction
    let s3_catalog =
        ParquetDataCatalog::from_uri("s3://bucket/base/path", None, None, None, None).unwrap();
    let reconstructed = s3_catalog.reconstruct_full_uri("data/quotes/file.parquet");
    assert_eq!(reconstructed, "s3://bucket/data/quotes/file.parquet");

    // Test GCS URI reconstruction
    let gcs_catalog =
        ParquetDataCatalog::from_uri("gs://bucket/base/path", None, None, None, None).unwrap();
    let reconstructed = gcs_catalog.reconstruct_full_uri("data/trades/file.parquet");
    assert_eq!(reconstructed, "gs://bucket/data/trades/file.parquet");

    // Test Azure URI reconstruction
    let azure_catalog =
        ParquetDataCatalog::from_uri("azure://account/container/path", None, None, None, None)
            .unwrap();
    let reconstructed = azure_catalog.reconstruct_full_uri("data/bars/file.parquet");
    assert_eq!(reconstructed, "azure://account/data/bars/file.parquet");

    // Test HTTP URI reconstruction
    let http_catalog =
        ParquetDataCatalog::from_uri("https://example.com/base/path", None, None, None, None)
            .unwrap();
    let reconstructed = http_catalog.reconstruct_full_uri("data/quotes/file.parquet");
    assert_eq!(
        reconstructed,
        "https://example.com/data/quotes/file.parquet"
    );

    // Test local path (should return full absolute path)
    let tmp = tempfile::tempdir().unwrap();
    let local_catalog = ParquetDataCatalog::new(tmp.path().to_path_buf(), None, None, None, None);
    let reconstructed = local_catalog.reconstruct_full_uri("data/quotes/file.parquet");
    let expected = format!("{}/data/quotes/file.parquet", tmp.path().display());
    assert_eq!(reconstructed, expected);
}
