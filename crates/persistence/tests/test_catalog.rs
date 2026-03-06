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

use std::{collections::HashMap, fs, io::Write, str::FromStr};

use nautilus_core::{Params, UnixNanos};
use nautilus_model::{
    data::{
        Bar, BarSpecification, BarType, BookOrder, Data, HasTsInit, IndexPriceUpdate,
        MarkPriceUpdate, OrderBookDelta, OrderBookDepth10, QuoteTick, TradeTick,
        depth::DEPTH10_LEN, is_monotonically_increasing_by_init, to_variant,
    },
    enums::{AggregationSource, AggressorSide, BarAggregation, BookAction, OrderSide, PriceType},
    identifiers::{InstrumentId, Symbol, TradeId},
    instruments::{CurrencyPair, Instrument, InstrumentAny},
    types::{Currency, Price, Quantity},
};
use nautilus_persistence::{
    backend::{
        catalog::ParquetDataCatalog,
        session::{DataBackendSession, QueryResult},
    },
    test_data::{MacroYieldCurveData, RustTestCustomData},
};
use nautilus_serialization::{arrow::ArrowSchemaProvider, ensure_custom_data_registered};
use nautilus_testkit::common::get_nautilus_test_data_file_path;
use rstest::rstest;
use rust_decimal::Decimal;
use serde_json::json;
use tempfile::TempDir;

fn create_temp_catalog() -> (TempDir, ParquetDataCatalog) {
    let temp_dir = TempDir::new().unwrap();
    let catalog = ParquetDataCatalog::new(temp_dir.path().to_path_buf(), None, None, None, None);
    (temp_dir, catalog)
}

/// Registers all test custom data types once so catalog decode can resolve them regardless of test order.
fn ensure_test_custom_data_registered() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        ensure_custom_data_registered::<MacroYieldCurveData>();
        ensure_custom_data_registered::<RustTestCustomData>();
    });
}

fn audusd_sim_id() -> InstrumentId {
    InstrumentId::from("AUD/USD.SIM")
}

fn spx_cboe_id() -> InstrumentId {
    InstrumentId::from("^SPX.CBOE")
}

fn ethusdt_binance_id() -> InstrumentId {
    InstrumentId::from("ETH/USDT.BINANCE")
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

fn create_quote_ticks_for_instrument(
    instrument_id: &str,
    base_ts: u64,
    count: usize,
) -> Vec<QuoteTick> {
    let instrument_id = InstrumentId::from_str(instrument_id).unwrap();
    (0..count)
        .map(|i| {
            QuoteTick::new(
                instrument_id,
                Price::from("1.0001"),
                Price::from("1.0002"),
                Quantity::from("100"),
                Quantity::from("100"),
                UnixNanos::from(base_ts + i as u64 * 1000),
                UnixNanos::from(base_ts + i as u64 * 1000),
            )
        })
        .collect()
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

fn create_index_bar(ts_init: u64) -> Bar {
    let bar_type = BarType::new(
        spx_cboe_id(),
        BarSpecification::new(1, BarAggregation::Minute, PriceType::Bid),
        AggregationSource::External,
    );

    Bar::new(
        bar_type,
        Price::new(1.00001, 5),
        Price::new(1.1, 1),
        Price::new(1.00000, 5),
        Price::new(1.00000, 5),
        Quantity::new(0.0, 0),
        UnixNanos::from(0),
        UnixNanos::from(ts_init),
    )
}

#[rstest]
fn test_quote_tick_query() {
    let expected_length = 9_500;
    let file_path = get_nautilus_test_data_file_path("quotes.parquet");

    let mut catalog = DataBackendSession::new(10_000);
    catalog
        .add_file::<QuoteTick>("quote_005", file_path.as_str(), None, None)
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
            None,
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
        .add_file::<QuoteTick>("quote_tick", file_path_quotes.as_str(), None, None)
        .unwrap();
    catalog
        .add_file::<TradeTick>("quote_tick_2", file_path_trades.as_str(), None, None)
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
        .add_file::<TradeTick>("trade_001", file_path.as_str(), None, None)
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
        .add_file::<Bar>("bar_001", file_path.as_str(), None, None)
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

#[rstest]
fn test_datafusion_parquet_round_trip() {
    use datafusion::parquet::{
        arrow::ArrowWriter, basic::Compression, file::properties::WriterProperties,
    };
    use nautilus_serialization::arrow::EncodeToRecordBatch;
    use pretty_assertions::assert_eq;

    // Read original data from parquet
    let file_path = get_nautilus_test_data_file_path("quotes.parquet");

    let mut session = DataBackendSession::new(1000);
    session
        .add_file::<QuoteTick>("test_data", file_path.as_str(), None, None)
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
        .add_file::<QuoteTick>("test_data", temp_file_path.to_str().unwrap(), None, None)
        .unwrap();
    let query_result: QueryResult = session.get_query_result();
    let ticks: Vec<Data> = query_result.collect();
    let ticks_variants: Vec<QuoteTick> = to_variant(ticks);

    assert_eq!(quote_ticks.len(), ticks_variants.len());
    for (orig, loaded) in quote_ticks.iter().zip(ticks_variants.iter()) {
        assert_eq!(orig, loaded);
    }
}

#[rstest]
fn test_rust_write_2_bars_to_catalog() {
    let (_temp_dir, catalog) = create_temp_catalog();

    let bars = vec![create_bar(1), create_bar(2)];
    catalog
        .write_to_parquet(bars.clone(), None, None, None)
        .unwrap();

    let bar_type = bars[0].bar_type.to_string();
    let intervals = catalog.get_intervals("bars", Some(bar_type)).unwrap();
    assert_eq!(intervals, vec![(1, 2)]);
}

#[rstest]
fn test_rust_append_data_to_catalog() {
    let (_temp_dir, catalog) = create_temp_catalog();

    let bars1 = vec![create_bar(1), create_bar(2)];
    catalog
        .write_to_parquet(bars1.clone(), None, None, None)
        .unwrap();

    let bars2 = vec![create_bar(3)];
    catalog.write_to_parquet(bars2, None, None, None).unwrap();

    let bar_type = bars1[0].bar_type.to_string();
    let intervals = catalog.get_intervals("bars", Some(bar_type)).unwrap();
    assert_eq!(intervals, vec![(1, 2), (3, 3)]);
}

#[rstest]
fn test_rust_consolidate_catalog() {
    let (_temp_dir, catalog) = create_temp_catalog();

    let bars1 = vec![create_bar(1), create_bar(2)];
    catalog
        .write_to_parquet(bars1.clone(), None, None, None)
        .unwrap();

    let bars2 = vec![create_bar(3)];
    catalog.write_to_parquet(bars2, None, None, None).unwrap();

    let bar_type = bars1[0].bar_type.to_string();
    catalog
        .consolidate_data("bars", Some(bar_type.clone()), None, None, None, None)
        .unwrap();

    let intervals = catalog.get_intervals("bars", Some(bar_type)).unwrap();
    assert_eq!(intervals, vec![(1, 3)]);
}

#[rstest]
fn test_rust_consolidate_catalog_with_time_range() {
    let (_temp_dir, catalog) = create_temp_catalog();

    let bars1 = vec![create_bar(1)];
    catalog
        .write_to_parquet(bars1.clone(), None, None, None)
        .unwrap();

    let bars2 = vec![create_bar(2)];
    catalog.write_to_parquet(bars2, None, None, None).unwrap();

    let bars3 = vec![create_bar(3)];
    catalog.write_to_parquet(bars3, None, None, None).unwrap();

    let bar_type = bars1[0].bar_type.to_string();
    catalog
        .consolidate_data(
            "bars",
            Some(bar_type.clone()),
            Some(UnixNanos::from(1)),
            Some(UnixNanos::from(2)),
            None,
            None,
        )
        .unwrap();

    let intervals = catalog.get_intervals("bars", Some(bar_type)).unwrap();
    assert_eq!(intervals, vec![(1, 2), (3, 3)]);
}

#[rstest]
fn test_rust_consolidate_with_deduplication() {
    let (_temp_dir, mut catalog) = create_temp_catalog();

    // Write bars [1, 2] and [2, 3] as separate files so ts=2 is duplicated
    let bars_a = vec![create_bar(1), create_bar(2)];
    catalog
        .write_to_parquet(bars_a.clone(), None, None, None)
        .unwrap();

    let bars_b = vec![create_bar(2), create_bar(3)];
    catalog
        .write_to_parquet(bars_b, None, None, Some(true))
        .unwrap();

    let bar_type = bars_a[0].bar_type.to_string();

    // Sanity check: two separate files exist
    let files_before = catalog
        .query_files("bars", Some(vec!["AUD/USD.SIM".to_string()]), None, None)
        .unwrap();
    assert_eq!(files_before.len(), 2);

    // Without deduplication the combined data has 4 rows (ts=2 appears twice)
    let raw = catalog
        .query_typed_data::<Bar>(
            Some(vec!["AUD/USD.SIM".to_string()]),
            None,
            None,
            None,
            None,
            true,
        )
        .unwrap();
    assert_eq!(raw.len(), 4);

    // Consolidate with deduplication enabled; disable disjoint check since
    // we intentionally wrote overlapping files to seed the duplicates
    catalog
        .consolidate_data("bars", Some(bar_type), None, None, Some(false), Some(true))
        .unwrap();

    // After consolidation there should be a single file
    let files_after = catalog
        .query_files("bars", Some(vec!["AUD/USD.SIM".to_string()]), None, None)
        .unwrap();
    assert_eq!(files_after.len(), 1);

    // The data should contain exactly 3 unique rows (duplicate ts=2 removed)
    let result = catalog
        .query_typed_data::<Bar>(
            Some(vec!["AUD/USD.SIM".to_string()]),
            None,
            None,
            None,
            None,
            true,
        )
        .unwrap();
    assert_eq!(result.len(), 3);
    assert_eq!(result[0].bar_type, bars_a[0].bar_type);
}

#[rstest]
fn test_rust_consolidate_index_with_deduplication() {
    let (_temp_dir, mut catalog) = create_temp_catalog();

    // Write bars [1, 2] and [2, 3] as separate files so ts=2 is duplicated
    let bars_a = vec![create_index_bar(1), create_index_bar(2)];
    catalog
        .write_to_parquet(bars_a.clone(), None, None, None)
        .unwrap();

    let bars_b = vec![create_index_bar(2), create_index_bar(3)];
    catalog
        .write_to_parquet(bars_b, None, None, Some(true))
        .unwrap();

    let bar_type = bars_a[0].bar_type.to_string();

    // Sanity check: two separate files exist
    let files_before = catalog
        .query_files("bars", Some(vec!["^SPX.CBOE".to_string()]), None, None)
        .unwrap();
    assert_eq!(files_before.len(), 2);

    // Without deduplication the combined data has 4 rows (ts=2 appears twice)
    let raw = catalog
        .query_typed_data::<Bar>(
            Some(vec!["^SPX.CBOE".to_string()]),
            None,
            None,
            None,
            None,
            true,
        )
        .unwrap();
    assert_eq!(raw.len(), 4);

    // Consolidate with deduplication enabled; disable disjoint check since
    // we intentionally wrote overlapping files to seed the duplicates
    catalog
        .consolidate_data("bars", Some(bar_type), None, None, Some(false), Some(true))
        .unwrap();

    // After consolidation there should be a single file
    let files_after = catalog
        .query_files("bars", Some(vec!["^SPX.CBOE".to_string()]), None, None)
        .unwrap();
    assert_eq!(files_after.len(), 1);

    // The data should contain exactly 3 unique rows (duplicate ts=2 removed)
    let result = catalog
        .query_typed_data::<Bar>(
            Some(vec!["^SPX.CBOE".to_string()]),
            None,
            None,
            None,
            None,
            true,
        )
        .unwrap();
    assert_eq!(result.len(), 3);
    assert_eq!(result[0].bar_type, bars_a[0].bar_type);
}

#[rstest]
fn test_register_object_store_from_uri_local_file() {
    // Test registering object store from local file URI
    let file_path = get_nautilus_test_data_file_path("trades.parquet");
    let parent_path = std::path::Path::new(&file_path).parent().unwrap();
    let file_uri = format!("file://{}", parent_path.display());

    let mut session = DataBackendSession::new(1000);

    // Register object store from local file URI
    session
        .register_object_store_from_uri(&file_uri, None)
        .unwrap();

    // Add file using the registered object store
    session
        .add_file::<TradeTick>("trade_ticks", &file_path, None, None)
        .unwrap();
    let query_result: QueryResult = session.get_query_result();
    let ticks: Vec<Data> = query_result.collect();

    assert_eq!(ticks.len(), 100);
    assert!(is_monotonically_increasing_by_init(&ticks));
}

#[rstest]
fn test_register_object_store_from_uri_invalid_uri() {
    // Test registering object store from invalid URI
    let mut session = DataBackendSession::new(1000);

    // Invalid URI should return an error
    let result = session.register_object_store_from_uri("invalid://not-a-real-uri", None);
    assert!(result.is_err());
}

#[rstest]
fn test_register_object_store_from_uri_nonexistent_path() {
    // Test registering object store from non-existent path URI
    let mut session = DataBackendSession::new(1000);

    // Non-existent path should return an error
    let result = session.register_object_store_from_uri("file:///nonexistent/path", None);
    assert!(result.is_err());
}

#[rstest]
fn test_rust_get_missing_intervals() {
    let (_temp_dir, catalog) = create_temp_catalog();

    let bars1 = vec![create_bar(1), create_bar(2)];
    catalog
        .write_to_parquet(bars1.clone(), None, None, None)
        .unwrap();

    let bars2 = vec![create_bar(5), create_bar(6)];
    catalog.write_to_parquet(bars2, None, None, None).unwrap();

    let bar_type = bars1[0].bar_type.to_string();
    let missing = catalog
        .get_missing_intervals_for_request(0, 10, "bars", Some(bar_type))
        .unwrap();

    assert_eq!(missing, vec![(0, 0), (3, 4), (7, 10)]);
}

#[rstest]
fn test_rust_reset_data_file_names() {
    let (_temp_dir, catalog) = create_temp_catalog();
    let bars = vec![create_bar(1), create_bar(2), create_bar(3)];
    catalog
        .write_to_parquet(bars.clone(), None, None, None)
        .unwrap();

    let bar_type = bars[0].bar_type.to_string();
    // Get intervals before reset
    let intervals_before = catalog
        .get_intervals("bars", Some(bar_type.clone()))
        .unwrap();
    assert_eq!(intervals_before, vec![(1, 3)]);

    // Reset file names
    let result = catalog.reset_data_file_names("bars", Some(bar_type));

    // The operation should succeed (even if it changes the intervals)
    assert!(result.is_ok());

    // Note: The intervals might change or be empty after reset depending on the implementation
    // This is acceptable as the reset operation might rename files in a way that affects interval parsing
}

#[rstest]
fn test_rust_extend_file_name() {
    let (_temp_dir, catalog) = create_temp_catalog();

    // Write data with a gap
    let bars1 = vec![create_bar(1)];
    catalog
        .write_to_parquet(bars1.clone(), None, None, None)
        .unwrap();

    let bars2 = vec![create_bar(4)];
    catalog.write_to_parquet(bars2, None, None, None).unwrap();

    let bar_type = bars1[0].bar_type.to_string();
    // Extend the first file to include the missing timestamp range
    catalog
        .extend_file_name(
            "bars",
            Some(bar_type.clone()),
            UnixNanos::from(2),
            UnixNanos::from(3),
        )
        .unwrap();

    let intervals = catalog.get_intervals("bars", Some(bar_type)).unwrap();
    assert_eq!(intervals, vec![(1, 3), (4, 4)]);
}

#[rstest]
fn test_rust_write_quote_ticks() {
    let (_temp_dir, catalog) = create_temp_catalog();

    let quote_ticks = vec![create_quote_tick(1), create_quote_tick(2)];
    catalog
        .write_to_parquet(quote_ticks, None, None, None)
        .unwrap();

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
    let (_temp_dir, catalog) = create_temp_catalog();

    let trade_ticks = vec![create_trade_tick(1), create_trade_tick(2)];
    catalog
        .write_to_parquet(trade_ticks, None, None, None)
        .unwrap();

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
    let (_temp_dir, catalog) = create_temp_catalog();

    let deltas = vec![create_order_book_delta(1), create_order_book_delta(2)];
    catalog.write_to_parquet(deltas, None, None, None).unwrap();

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
    let (_temp_dir, catalog) = create_temp_catalog();

    let depths = vec![create_order_book_depth10(1), create_order_book_depth10(2)];
    catalog.write_to_parquet(depths, None, None, None).unwrap();

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
    let (_temp_dir, catalog) = create_temp_catalog();

    let mark_prices = vec![create_mark_price_update(1), create_mark_price_update(2)];
    catalog
        .write_to_parquet(mark_prices, None, None, None)
        .unwrap();

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
    let (_temp_dir, catalog) = create_temp_catalog();

    let index_prices = vec![create_index_price_update(1), create_index_price_update(2)];
    catalog
        .write_to_parquet(index_prices, None, None, None)
        .unwrap();

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
    let (_temp_dir, catalog) = create_temp_catalog();

    let bars1 = vec![create_bar(1), create_bar(2)];
    catalog.write_to_parquet(bars1, None, None, None).unwrap();

    let bars2 = vec![create_bar(3), create_bar(4)];
    catalog.write_to_parquet(bars2, None, None, None).unwrap();

    let files = catalog
        .query_files("bars", Some(vec!["AUD/USD.SIM".to_string()]), None, None)
        .unwrap();

    assert_eq!(files.len(), 2);
}

#[rstest]
fn test_rust_query_files_with_multiple_files() {
    let (_temp_dir, catalog) = create_temp_catalog();

    let bars1 = vec![create_bar(1), create_bar(2)];
    catalog.write_to_parquet(bars1, None, None, None).unwrap();

    let bars2 = vec![create_bar(3), create_bar(4)];
    catalog.write_to_parquet(bars2, None, None, None).unwrap();

    let bars3 = vec![create_bar(5), create_bar(6)];
    catalog.write_to_parquet(bars3, None, None, None).unwrap();

    let files = catalog
        .query_files("bars", Some(vec!["AUD/USD.SIM".to_string()]), None, None)
        .unwrap();

    assert_eq!(files.len(), 3);
}

#[rstest]
fn test_rust_get_intervals_empty() {
    let (_temp_dir, catalog) = create_temp_catalog();

    let intervals = catalog
        .get_intervals("bars", Some("AUD/USD.SIM".to_string()))
        .unwrap();

    assert!(intervals.is_empty());
}

#[rstest]
fn test_consolidate_data_by_period_basic() {
    let (_temp_dir, mut catalog) = create_temp_catalog();

    // Create data spanning multiple hours
    let bars = vec![
        create_bar(3_600_000_000_000), // 1 hour
        create_bar(3_601_000_000_000), // 1 hour + 1 second
        create_bar(7_200_000_000_000), // 2 hours
        create_bar(7_201_000_000_000), // 2 hours + 1 second
    ];
    catalog.write_to_parquet(bars, None, None, None).unwrap();

    // Consolidate by 1-hour periods
    catalog
        .consolidate_data_by_period(
            "bars",
            Some("AUD/USD.SIM".to_string()),
            Some(3_600_000_000_000), // 1 hour in nanoseconds
            None,
            None,
            Some(true),
        )
        .unwrap();

    let bars = vec![
        create_bar(3_600_000_000_000), // 1 hour
        create_bar(3_601_000_000_000), // 1 hour + 1 second
        create_bar(7_200_000_000_000), // 2 hours
        create_bar(7_201_000_000_000), // 2 hours + 1 second
    ];
    catalog
        .write_to_parquet(bars.clone(), None, None, None)
        .unwrap();

    let bar_type = bars[0].bar_type.to_string();
    // Consolidate by 1-hour periods
    catalog
        .consolidate_data_by_period(
            "bars",
            Some(bar_type.clone()),
            Some(3_600_000_000_000), // 1 hour in nanoseconds
            None,
            None,
            Some(true),
        )
        .unwrap();

    // Should have consolidated into period-based files
    let intervals = catalog.get_intervals("bars", Some(bar_type)).unwrap();

    // The exact intervals depend on the implementation, but we should have fewer files
    assert!(!intervals.is_empty());
}

#[rstest]
fn test_consolidate_data_by_period_with_time_range() {
    let (_temp_dir, mut catalog) = create_temp_catalog();

    // Create data spanning multiple periods
    let bars = vec![
        create_bar(1000),
        create_bar(2000),
        create_bar(3000),
        create_bar(4000),
        create_bar(5000),
    ];
    catalog
        .write_to_parquet(bars.clone(), None, None, None)
        .unwrap();

    let bar_type = bars[0].bar_type.to_string();
    // Consolidate only middle range
    catalog
        .consolidate_data_by_period(
            "bars",
            Some(bar_type.clone()),
            Some(86_400_000_000_000), // 1 day in nanoseconds
            Some(UnixNanos::from(2000)),
            Some(UnixNanos::from(4000)),
            Some(false),
        )
        .unwrap();

    // Operation should complete without error
    let intervals = catalog.get_intervals("bars", Some(bar_type)).unwrap();
    assert!(!intervals.is_empty());
}

#[rstest]
fn test_consolidate_data_by_period_empty_data() {
    let (_temp_dir, mut catalog) = create_temp_catalog();

    let bar_type = create_bar(1).bar_type.to_string();
    // Consolidate empty catalog
    let result = catalog.consolidate_data_by_period(
        "bars",
        Some(bar_type),
        Some(86_400_000_000_000), // 1 day in nanoseconds
        None,
        None,
        Some(true),
    );

    // Should succeed with no data
    assert!(result.is_ok());
}

#[rstest]
fn test_consolidate_data_by_period_different_periods() {
    let (_temp_dir, mut catalog) = create_temp_catalog();

    // Create data spanning multiple minutes
    let bars = vec![
        create_bar(60_000_000_000),  // 1 minute
        create_bar(120_000_000_000), // 2 minutes
        create_bar(180_000_000_000), // 3 minutes
        create_bar(240_000_000_000), // 4 minutes
    ];
    catalog
        .write_to_parquet(bars.clone(), None, None, None)
        .unwrap();

    let bar_type = bars[0].bar_type.to_string();
    // Test different period sizes
    let periods = vec![
        1_800_000_000_000,  // 30 minutes
        3_600_000_000_000,  // 1 hour
        86_400_000_000_000, // 1 day
    ];

    for period_nanos in periods {
        let result = catalog.consolidate_data_by_period(
            "bars",
            Some(bar_type.clone()),
            Some(period_nanos),
            None,
            None,
            Some(true),
        );

        assert!(result.is_ok(), "Failed for period: {period_nanos}");
    }
}

#[rstest]
fn test_consolidate_data_by_period_ensure_contiguous_files_false() {
    let (_temp_dir, mut catalog) = create_temp_catalog();

    // Create some test data
    let bars = vec![create_bar(1000), create_bar(2000), create_bar(3000)];
    catalog
        .write_to_parquet(bars.clone(), None, None, None)
        .unwrap();

    let bar_type = bars[0].bar_type.to_string();
    // Consolidate with ensure_contiguous_files=false
    catalog
        .consolidate_data_by_period(
            "bars",
            Some(bar_type.clone()),
            Some(86_400_000_000_000), // 1 day in nanoseconds
            None,
            None,
            Some(false), // Use actual data timestamps for file naming
        )
        .unwrap();

    // Operation should complete without error
    let intervals = catalog.get_intervals("bars", Some(bar_type)).unwrap();
    assert!(!intervals.is_empty());
}

#[rstest]
fn test_consolidate_catalog_by_period_basic() {
    let (_temp_dir, mut catalog) = create_temp_catalog();

    // Create data for multiple data types
    let bars = vec![create_bar(1000), create_bar(2000)];
    catalog
        .write_to_parquet(bars.clone(), None, None, None)
        .unwrap();

    let quotes = vec![create_quote_tick(1000), create_quote_tick(2000)];
    catalog.write_to_parquet(quotes, None, None, None).unwrap();

    // Consolidate entire catalog
    catalog
        .consolidate_catalog_by_period(
            Some(86_400_000_000_000), // 1 day in nanoseconds
            None,
            None,
            Some(true),
        )
        .unwrap();

    // Operation should complete without error
    let bar_type = bars[0].bar_type.to_string();
    let bar_intervals = catalog.get_intervals("bars", Some(bar_type)).unwrap();
    let quote_intervals = catalog
        .get_intervals("quotes", Some("ETH/USDT.BINANCE".to_string()))
        .unwrap();

    assert!(!bar_intervals.is_empty());
    assert!(!quote_intervals.is_empty());
}

#[rstest]
fn test_consolidate_catalog_by_period_with_time_range() {
    let (_temp_dir, mut catalog) = create_temp_catalog();

    // Create data spanning multiple periods
    let bars = vec![create_bar(1000), create_bar(5000), create_bar(10000)];
    catalog
        .write_to_parquet(bars.clone(), None, None, None)
        .unwrap();

    // Consolidate catalog with time range
    catalog
        .consolidate_catalog_by_period(
            Some(86_400_000_000_000), // 1 day in nanoseconds
            Some(UnixNanos::from(2000)),
            Some(UnixNanos::from(8000)),
            Some(false),
        )
        .unwrap();

    // Operation should complete without error
    let bar_type = bars[0].bar_type.to_string();
    let intervals = catalog.get_intervals("bars", Some(bar_type)).unwrap();
    assert!(!intervals.is_empty());
}

#[rstest]
fn test_consolidate_catalog_by_period_empty_catalog() {
    let (_temp_dir, mut catalog) = create_temp_catalog();

    // Consolidate empty catalog
    let result = catalog.consolidate_catalog_by_period(
        Some(86_400_000_000_000), // 1 day in nanoseconds
        None,
        None,
        Some(true),
    );

    // Should succeed with empty catalog
    assert!(result.is_ok());
}

#[rstest]
fn test_consolidate_catalog_by_period_default_parameters() {
    let (_temp_dir, mut catalog) = create_temp_catalog();

    // Create some test data
    let bars = vec![create_bar(1000), create_bar(2000)];
    catalog.write_to_parquet(bars, None, None, None).unwrap();

    // Consolidate with default parameters
    let result = catalog.consolidate_catalog_by_period(None, None, None, None);

    // Should use default 1-day period
    assert!(result.is_ok());
}

#[rstest]
fn test_consolidate_data_by_period_multiple_instruments() {
    let (_temp_dir, mut catalog) = create_temp_catalog();

    // Create bars for AUD/USD
    let aud_bars = vec![create_bar(1000), create_bar(2000)];
    catalog
        .write_to_parquet(aud_bars.clone(), None, None, None)
        .unwrap();

    // Create quotes for ETH/USDT
    let eth_quotes = vec![create_quote_tick(1000), create_quote_tick(2000)];
    catalog
        .write_to_parquet(eth_quotes, None, None, None)
        .unwrap();

    let bar_type = aud_bars[0].bar_type.to_string();
    // Consolidate specific instrument only
    catalog
        .consolidate_data_by_period(
            "bars",
            Some(bar_type.clone()),
            Some(86_400_000_000_000), // 1 day in nanoseconds
            None,
            None,
            Some(true),
        )
        .unwrap();

    // Only AUD/USD bars should be affected
    let aud_intervals = catalog.get_intervals("bars", Some(bar_type)).unwrap();
    let eth_intervals = catalog
        .get_intervals("quotes", Some("ETH/USDT.BINANCE".to_string()))
        .unwrap();

    assert!(!aud_intervals.is_empty());
    assert!(!eth_intervals.is_empty());
}

#[rstest]
fn test_consolidate_data_by_period_invalid_type() {
    let (_temp_dir, mut catalog) = create_temp_catalog();

    // Consolidate non-existent data type
    let result = catalog.consolidate_data_by_period(
        "invalid_type",
        Some("AUD/USD.SIM-1-MINUTE-BID-EXTERNAL".to_string()),
        Some(86_400_000_000_000), // 1 day in nanoseconds
        None,
        None,
        Some(true),
    );

    // Should return error for invalid data type
    assert!(result.is_err());
}

#[rstest]
fn test_prepare_consolidation_queries_empty_intervals() {
    let (_temp_dir, catalog) = create_temp_catalog();

    // Test with empty intervals
    let intervals = vec![];
    let period_nanos = 86_400_000_000_000; // 1 day

    let queries = catalog
        .prepare_consolidation_queries("quotes", None, &intervals, period_nanos, None, None, true)
        .unwrap();

    // Should have no queries for empty intervals
    assert!(queries.is_empty());
}

#[rstest]
fn test_prepare_consolidation_queries_filtered_intervals() {
    let (_temp_dir, catalog) = create_temp_catalog();

    // Test with intervals that are filtered out by time range
    let intervals = vec![(1000, 2000), (3000, 4000)];
    let period_nanos = 86_400_000_000_000; // 1 day
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
    let (_temp_dir, mut catalog) = create_temp_catalog();
    // Create test data
    let quotes = vec![create_quote_tick(1000), create_quote_tick(2000)];
    catalog.write_to_parquet(quotes, None, None, None).unwrap();

    // query using generic typed data function
    let result = catalog
        .query_typed_data::<QuoteTick>(
            Some(vec!["ETH/USDT.BINANCE".to_string()]),
            Some(UnixNanos::from(500)),
            Some(UnixNanos::from(2500)),
            None,
            None,
            true, // optimize_file_loading=true (default)
        )
        .unwrap();
    assert_eq!(result.len(), 2);

    // Verify the data is correct
    let q = &result[0];
    assert_eq!(q.instrument_id.to_string(), "ETH/USDT.BINANCE");
    assert_eq!(q.ts_init, UnixNanos::from(1000));
}

#[rstest]
fn test_generic_query_typed_data_bars() {
    let (_temp_dir, mut catalog) = create_temp_catalog();

    // Create test data
    let bars = vec![create_bar(1000), create_bar(2000)];
    catalog.write_to_parquet(bars, None, None, None).unwrap();

    // query using generic typed data function
    let result = catalog
        .query_typed_data::<Bar>(
            Some(vec!["AUD/USD.SIM".to_string()]),
            Some(UnixNanos::from(500)),
            Some(UnixNanos::from(2500)),
            None,
            None,
            true, // optimize_file_loading=true (default)
        )
        .unwrap();
    assert_eq!(result.len(), 2);

    // Verify the data is correct
    let b = &result[0];
    assert_eq!(b.bar_type.instrument_id().to_string(), "AUD/USD.SIM");
    assert_eq!(b.ts_init, UnixNanos::from(1000));
}

#[rstest]
fn test_generic_query_typed_data_empty_result() {
    let (_temp_dir, mut catalog) = create_temp_catalog();

    // query with no matching data
    let result = catalog
        .query_typed_data::<QuoteTick>(
            Some(vec!["NONEXISTENT".to_string()]),
            Some(UnixNanos::from(500)),
            Some(UnixNanos::from(2500)),
            None,
            None,
            true, // optimize_file_loading=true (default)
        )
        .unwrap();
    assert!(result.is_empty());
}

#[rstest]
fn test_generic_query_typed_data_with_where_clause() {
    let (_temp_dir, mut catalog) = create_temp_catalog();

    // Create test data
    let quotes = vec![create_quote_tick(1000), create_quote_tick(2000)];
    catalog.write_to_parquet(quotes, None, None, None).unwrap();

    // query with WHERE clause
    let result = catalog
        .query_typed_data::<QuoteTick>(
            Some(vec!["ETH/USDT.BINANCE".to_string()]),
            Some(UnixNanos::from(500)),
            Some(UnixNanos::from(2500)),
            Some("ts_init >= 1500"),
            None,
            true, // optimize_file_loading=true (default)
        )
        .unwrap();

    // should only return the second quote
    assert_eq!(result.len(), 1);
}

#[rstest]
fn test_generic_consolidate_data_by_period_quotes() {
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

    // consolidate using generic function
    catalog
        .consolidate_data_by_period_generic::<QuoteTick>(
            Some("ETH/USDT.BINANCE".to_string()),
            Some(86_400_000_000_000), // 1 day in nanoseconds
            None,
            None,
            Some(true),
        )
        .unwrap();

    // should have fewer files after consolidation
    let final_intervals = catalog
        .get_intervals("quotes", Some("ETH/USDT.BINANCE".to_string()))
        .unwrap();
    assert!(final_intervals.len() <= initial_intervals.len());
}

#[rstest]
fn test_generic_consolidate_data_by_period_bars() {
    let (_temp_dir, mut catalog) = create_temp_catalog();

    // Create multiple small files with contiguous timestamps
    let mut bars_list = Vec::new();
    for i in 0..3 {
        let bars = vec![create_bar(1000 + i)];
        bars_list.push(bars[0]);
        catalog.write_to_parquet(bars, None, None, None).unwrap();
    }

    let bar_type = bars_list[0].bar_type.to_string();
    // Verify we have multiple files initially
    let initial_intervals = catalog
        .get_intervals("bars", Some(bar_type.clone()))
        .unwrap();
    assert_eq!(initial_intervals.len(), 3);

    // consolidate using generic function
    catalog
        .consolidate_data_by_period_generic::<Bar>(
            Some(bar_type.clone()),
            Some(86_400_000_000_000), // 1 day in nanoseconds
            None,
            None,
            Some(true),
        )
        .unwrap();

    // should have fewer files after consolidation
    let final_intervals = catalog.get_intervals("bars", Some(bar_type)).unwrap();
    assert!(final_intervals.len() <= initial_intervals.len());
}

#[rstest]
fn test_generic_consolidate_data_by_period_empty_catalog() {
    let (_temp_dir, mut catalog) = create_temp_catalog();

    // consolidate empty catalog
    let result = catalog.consolidate_data_by_period_generic::<QuoteTick>(
        Some("ETH/USDT.BINANCE".to_string()),
        Some(86_400_000_000_000), // 1 day in nanoseconds
        None,
        None,
        Some(true),
    );

    // should succeed with empty catalog
    assert!(result.is_ok());
}

#[rstest]
fn test_generic_consolidate_data_by_period_with_time_range() {
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

    // consolidate with time range
    catalog
        .consolidate_data_by_period_generic::<QuoteTick>(
            Some("ETH/USDT.BINANCE".to_string()),
            Some(86_400_000_000_000), // 1 day in nanoseconds
            Some(UnixNanos::from(2000)),
            Some(UnixNanos::from(8000)),
            Some(false),
        )
        .unwrap();

    // operation should complete without error
    let intervals = catalog
        .get_intervals("quotes", Some("ETH/USDT.BINANCE".to_string()))
        .unwrap();
    assert!(!intervals.is_empty());
}

#[rstest]
fn test_consolidation_workflow_end_to_end() {
    let (_temp_dir, catalog) = create_temp_catalog();

    // Create multiple small files
    let mut bars_list = Vec::new();
    for i in 0..5 {
        let bars = vec![create_bar(1000 + i * 1000)];
        bars_list.push(bars[0]);
        catalog.write_to_parquet(bars, None, None, None).unwrap();
    }

    let bar_type = bars_list[0].bar_type.to_string();
    // Verify we have multiple files initially
    let initial_intervals = catalog
        .get_intervals("bars", Some(bar_type.clone()))
        .unwrap();
    assert_eq!(initial_intervals.len(), 5);

    // consolidate all files
    catalog
        .consolidate_data("bars", Some(bar_type.clone()), None, None, None, None)
        .unwrap();

    // should have fewer files after consolidation
    let final_intervals = catalog.get_intervals("bars", Some(bar_type)).unwrap();
    assert!(final_intervals.len() <= initial_intervals.len());
}

#[rstest]
fn test_consolidation_preserves_data_integrity() {
    let (_temp_dir, mut catalog) = create_temp_catalog();

    // Create test data with contiguous timestamps
    let original_bars = vec![create_bar(1000), create_bar(1001), create_bar(1002)];

    // Write each bar separately to create multiple files
    for bar in &original_bars {
        catalog
            .write_to_parquet(vec![*bar], None, None, None)
            .unwrap();
    }

    let bar_type = original_bars[0].bar_type.to_string();
    // consolidate the data
    catalog
        .consolidate_data_by_period(
            "bars",
            Some(bar_type.clone()),
            Some(86_400_000_000_000), // 1 day in nanoseconds
            None,
            None,
            Some(true),
        )
        .unwrap();

    // data should still be accessible after consolidation
    let intervals = catalog.get_intervals("bars", Some(bar_type)).unwrap();

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

impl HasTsInit for DummyData {
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

#[cfg(feature = "cloud")]
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

    // Test custom data path with identifier
    let path_custom_with_id = format!(
        "{}/data/custom/RustTestCustomData/RUST.TEST",
        base_dir.to_string_lossy()
    );
    let (data_cls, identifier) = catalog
        .extract_data_cls_and_identifier_from_path(&path_custom_with_id)
        .unwrap();
    assert_eq!(data_cls, Some("custom/RustTestCustomData".to_string()));
    assert_eq!(identifier, Some("RUST.TEST".to_string()));

    // Test custom data path with identifier subdirs
    let path_custom_subdirs = format!("{}/data/custom/MyType/foo/bar", base_dir.to_string_lossy());
    let (data_cls, identifier) = catalog
        .extract_data_cls_and_identifier_from_path(&path_custom_subdirs)
        .unwrap();
    assert_eq!(data_cls, Some("custom/MyType".to_string()));
    assert_eq!(identifier, Some("foo/bar".to_string()));

    // Test custom data path without identifier
    let path_custom_no_id = format!("{}/data/custom/MyType", base_dir.to_string_lossy());
    let (data_cls, identifier) = catalog
        .extract_data_cls_and_identifier_from_path(&path_custom_no_id)
        .unwrap();
    assert_eq!(data_cls, Some("custom/MyType".to_string()));
    assert_eq!(identifier, None);

    // Test invalid path
    let invalid_path = "/invalid/path";
    let (data_cls, identifier) = catalog
        .extract_data_cls_and_identifier_from_path(invalid_path)
        .unwrap();
    assert_eq!(data_cls, None);
    assert_eq!(identifier, None);
}

/// Ensures custom data path built by make_path_custom_data (via custom module) matches
/// the format expected by extract_data_cls_and_identifier_from_path (catalog behavior unchanged after extraction).
#[rstest]
fn test_make_path_custom_data_roundtrip() {
    let tmp = tempfile::tempdir().unwrap();
    let base_dir = tmp.path().join("catalog");
    std::fs::create_dir_all(&base_dir).unwrap();

    let catalog = ParquetDataCatalog::new(base_dir, None, None, None, None);

    let path_no_id = catalog.make_path_custom_data("MyCustomType", None).unwrap();
    assert!(path_no_id.contains("data/custom/MyCustomType"));
    let (data_cls, identifier) = catalog
        .extract_data_cls_and_identifier_from_path(&path_no_id)
        .unwrap();
    assert_eq!(data_cls.as_deref(), Some("custom/MyCustomType"));
    assert_eq!(identifier, None);

    let path_with_id = catalog
        .make_path_custom_data("RustTestCustomData", Some("RUST.TEST".to_string()))
        .unwrap();
    assert!(path_with_id.contains("data/custom/RustTestCustomData"));
    let (data_cls2, identifier2) = catalog
        .extract_data_cls_and_identifier_from_path(&path_with_id)
        .unwrap();
    assert_eq!(data_cls2.as_deref(), Some("custom/RustTestCustomData"));
    assert_eq!(identifier2.as_deref(), Some("RUST.TEST"));
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
#[allow(clippy::needless_collect)] // Collect needed for .len() and .iter().find()
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

#[cfg(feature = "cloud")]
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
    let azure_test_options = Some(
        [("account_name".to_string(), "test".to_string())]
            .iter()
            .cloned()
            .collect(),
    );
    let azure_catalog =
        ParquetDataCatalog::from_uri("az://container/path", azure_test_options, None, None, None)
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

#[cfg(feature = "cloud")]
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
    let azure_test_options = Some(
        [("account_name".to_string(), "test".to_string())]
            .iter()
            .cloned()
            .collect(),
    );
    let azure_catalog =
        ParquetDataCatalog::from_uri("az://container/path", azure_test_options, None, None, None)
            .unwrap();
    let reconstructed = azure_catalog.reconstruct_full_uri("data/bars/file.parquet");
    assert_eq!(reconstructed, "az://container/data/bars/file.parquet");

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

#[rstest]
fn test_delete_data_range_complete_file_deletion() {
    let (_temp_dir, mut catalog) = create_temp_catalog();

    // Create test data
    let quotes = vec![
        create_quote_tick(1_000_000_000),
        create_quote_tick(2_000_000_000),
    ];

    // Write data
    catalog.write_to_parquet(quotes, None, None, None).unwrap();

    // Verify initial state
    let initial_data = catalog
        .query_typed_data::<QuoteTick>(None, None, None, None, None, true)
        .unwrap();
    assert_eq!(initial_data.len(), 2);

    // delete all data
    catalog
        .delete_data_range(
            "quotes",
            Some("ETH/USDT.BINANCE".to_string()),
            Some(UnixNanos::from(0)),
            Some(UnixNanos::from(3_000_000_000)),
        )
        .unwrap();

    // verify deletion
    let remaining_data = catalog
        .query_typed_data::<QuoteTick>(None, None, None, None, None, true)
        .unwrap();
    assert_eq!(remaining_data.len(), 0);
}

#[rstest]
fn test_delete_data_range_partial_file_overlap_start() {
    let (_temp_dir, mut catalog) = create_temp_catalog();

    // Create test data
    let quotes = vec![
        create_quote_tick(1_000_000_000),
        create_quote_tick(2_000_000_000),
        create_quote_tick(3_000_000_000),
    ];

    // Write data
    catalog.write_to_parquet(quotes, None, None, None).unwrap();

    // delete first part of the data
    catalog
        .delete_data_range(
            "quotes",
            Some("ETH/USDT.BINANCE".to_string()),
            Some(UnixNanos::from(0)),
            Some(UnixNanos::from(1_500_000_000)),
        )
        .unwrap();

    // verify remaining data
    let remaining_data = catalog
        .query_typed_data::<QuoteTick>(None, None, None, None, None, true)
        .unwrap();
    assert_eq!(remaining_data.len(), 2);
    assert_eq!(remaining_data[0].ts_init.as_u64(), 2_000_000_000);
    assert_eq!(remaining_data[1].ts_init.as_u64(), 3_000_000_000);
}

#[rstest]
fn test_delete_data_range_partial_file_overlap_end() {
    let (_temp_dir, mut catalog) = create_temp_catalog();

    // Create test data
    let quotes = vec![
        create_quote_tick(1_000_000_000),
        create_quote_tick(2_000_000_000),
        create_quote_tick(3_000_000_000),
    ];

    // Write data
    catalog.write_to_parquet(quotes, None, None, None).unwrap();

    // delete last part of the data
    catalog
        .delete_data_range(
            "quotes",
            Some("ETH/USDT.BINANCE".to_string()),
            Some(UnixNanos::from(2_500_000_000)),
            Some(UnixNanos::from(4_000_000_000)),
        )
        .unwrap();

    // verify remaining data
    let remaining_data = catalog
        .query_typed_data::<QuoteTick>(None, None, None, None, None, true)
        .unwrap();
    assert_eq!(remaining_data.len(), 2);
    assert_eq!(remaining_data[0].ts_init.as_u64(), 1_000_000_000);
    assert_eq!(remaining_data[1].ts_init.as_u64(), 2_000_000_000);
}

#[rstest]
fn test_delete_data_range_partial_file_overlap_middle() {
    let (_temp_dir, mut catalog) = create_temp_catalog();

    // Create test data
    let quotes = vec![
        create_quote_tick(1_000_000_000),
        create_quote_tick(2_000_000_000),
        create_quote_tick(3_000_000_000),
        create_quote_tick(4_000_000_000),
    ];

    // Write data
    catalog.write_to_parquet(quotes, None, None, None).unwrap();

    // delete middle part of the data
    catalog
        .delete_data_range(
            "quotes",
            Some("ETH/USDT.BINANCE".to_string()),
            Some(UnixNanos::from(1_500_000_000)),
            Some(UnixNanos::from(3_500_000_000)),
        )
        .unwrap();

    // verify remaining data
    let remaining_data = catalog
        .query_typed_data::<QuoteTick>(None, None, None, None, None, true)
        .unwrap();
    assert_eq!(remaining_data.len(), 2);
    assert_eq!(remaining_data[0].ts_init.as_u64(), 1_000_000_000);
    assert_eq!(remaining_data[1].ts_init.as_u64(), 4_000_000_000);
}

#[rstest]
fn test_delete_data_range_no_data() {
    let (_temp_dir, mut catalog) = create_temp_catalog();

    // delete from empty catalog - should not raise any errors
    let result = catalog.delete_data_range(
        "quotes",
        Some("ETH/USDT.BINANCE".to_string()),
        Some(UnixNanos::from(1_000_000_000)),
        Some(UnixNanos::from(2_000_000_000)),
    );

    // should succeed
    assert!(result.is_ok());

    // Verify no data
    let remaining_data = catalog
        .query_typed_data::<QuoteTick>(None, None, None, None, None, true)
        .unwrap();
    assert_eq!(remaining_data.len(), 0);
}

#[rstest]
fn test_delete_data_range_no_intersection() {
    let (_temp_dir, mut catalog) = create_temp_catalog();

    // Create test data
    let quotes = vec![create_quote_tick(2_000_000_000)];

    // Write data
    catalog.write_to_parquet(quotes, None, None, None).unwrap();

    // delete data outside existing range
    catalog
        .delete_data_range(
            "quotes",
            Some("ETH/USDT.BINANCE".to_string()),
            Some(UnixNanos::from(3_000_000_000)),
            Some(UnixNanos::from(4_000_000_000)),
        )
        .unwrap();

    // verify all existing data remains
    let remaining_data = catalog
        .query_typed_data::<QuoteTick>(None, None, None, None, None, true)
        .unwrap();
    assert_eq!(remaining_data.len(), 1);
    assert_eq!(remaining_data[0].ts_init.as_u64(), 2_000_000_000);
}

#[rstest]
fn test_delete_catalog_range_multiple_data_types() {
    let (_temp_dir, mut catalog) = create_temp_catalog();

    // Create data for multiple data types
    let quotes = vec![
        create_quote_tick(1_000_000_000),
        create_quote_tick(2_000_000_000),
    ];
    let bars = vec![create_bar(1_500_000_000), create_bar(2_500_000_000)];

    catalog.write_to_parquet(quotes, None, None, None).unwrap();
    catalog.write_to_parquet(bars, None, None, None).unwrap();

    // Verify initial state
    let initial_quotes = catalog
        .query_typed_data::<QuoteTick>(None, None, None, None, None, true)
        .unwrap();
    let initial_bars = catalog
        .query_typed_data::<Bar>(None, None, None, None, None, true)
        .unwrap();
    assert_eq!(initial_quotes.len(), 2);
    assert_eq!(initial_bars.len(), 2);

    // delete data across all data types in a specific range
    catalog
        .delete_catalog_range(
            Some(UnixNanos::from(1_200_000_000)),
            Some(UnixNanos::from(2_200_000_000)),
        )
        .unwrap();

    // verify deletion from both data types within the range
    let remaining_quotes = catalog
        .query_typed_data::<QuoteTick>(None, None, None, None, None, true)
        .unwrap();
    let remaining_bars = catalog
        .query_typed_data::<Bar>(None, None, None, None, None, true)
        .unwrap();

    // Should keep quotes outside the deletion range
    assert_eq!(remaining_quotes.len(), 1);
    assert_eq!(remaining_quotes[0].ts_init.as_u64(), 1_000_000_000);

    // Should keep bars outside the deletion range
    assert_eq!(remaining_bars.len(), 1);
    assert_eq!(remaining_bars[0].ts_init.as_u64(), 2_500_000_000);
}

#[rstest]
fn test_delete_catalog_range_complete_deletion() {
    let (_temp_dir, mut catalog) = create_temp_catalog();

    // Create data for multiple data types
    let quotes = vec![create_quote_tick(1_000_000_000)];
    let bars = vec![create_bar(2_000_000_000)];

    catalog.write_to_parquet(quotes, None, None, None).unwrap();
    catalog.write_to_parquet(bars, None, None, None).unwrap();

    // Verify initial state
    assert_eq!(
        catalog
            .query_typed_data::<QuoteTick>(None, None, None, None, None, true)
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        catalog
            .query_typed_data::<Bar>(None, None, None, None, None, true)
            .unwrap()
            .len(),
        1
    );

    // delete all data
    catalog
        .delete_catalog_range(
            Some(UnixNanos::from(0)),
            Some(UnixNanos::from(3_000_000_000)),
        )
        .unwrap();

    // should have no data left
    assert_eq!(
        catalog
            .query_typed_data::<QuoteTick>(None, None, None, None, None, true)
            .unwrap()
            .len(),
        0
    );
    assert_eq!(
        catalog
            .query_typed_data::<Bar>(None, None, None, None, None, true)
            .unwrap()
            .len(),
        0
    );
}

#[rstest]
fn test_delete_catalog_range_empty_catalog() {
    let (_temp_dir, mut catalog) = create_temp_catalog();

    // delete from empty catalog
    let result = catalog.delete_catalog_range(
        Some(UnixNanos::from(1_000_000_000)),
        Some(UnixNanos::from(2_000_000_000)),
    );

    // should not raise any errors
    assert!(result.is_ok());
    assert_eq!(
        catalog
            .query_typed_data::<QuoteTick>(None, None, None, None, None, true)
            .unwrap()
            .len(),
        0
    );
    assert_eq!(
        catalog
            .query_typed_data::<Bar>(None, None, None, None, None, true)
            .unwrap()
            .len(),
        0
    );
}

#[rstest]
fn test_delete_catalog_range_open_boundaries() {
    let (_temp_dir, mut catalog) = create_temp_catalog();

    // Create test data
    let quotes = vec![
        create_quote_tick(1_000_000_000),
        create_quote_tick(2_000_000_000),
        create_quote_tick(3_000_000_000),
    ];
    let bars = vec![
        create_bar(1_500_000_000),
        create_bar(2_500_000_000),
        create_bar(3_500_000_000),
    ];

    catalog.write_to_parquet(quotes, None, None, None).unwrap();
    catalog.write_to_parquet(bars, None, None, None).unwrap();

    // delete from beginning to middle (open start)
    catalog
        .delete_catalog_range(None, Some(UnixNanos::from(2_200_000_000)))
        .unwrap();

    // should keep data after end boundary
    let remaining_quotes = catalog
        .query_typed_data::<QuoteTick>(None, None, None, None, None, true)
        .unwrap();
    let remaining_bars = catalog
        .query_typed_data::<Bar>(None, None, None, None, None, true)
        .unwrap();

    assert_eq!(remaining_quotes.len(), 1);
    assert_eq!(remaining_quotes[0].ts_init.as_u64(), 3_000_000_000);
    assert_eq!(remaining_bars.len(), 2);
    assert!(
        remaining_bars
            .iter()
            .any(|b| b.ts_init.as_u64() == 2_500_000_000)
    );
    assert!(
        remaining_bars
            .iter()
            .any(|b| b.ts_init.as_u64() == 3_500_000_000)
    );
}

#[rstest]
fn test_prepare_delete_operations_basic() {
    let (_temp_dir, catalog) = create_temp_catalog();

    // Test basic delete operation preparation
    let intervals = vec![(1000, 5000), (6000, 10000)];

    let operations = catalog
        .prepare_delete_operations(
            "quotes",
            Some("ETH/USDT.BINANCE".to_string()),
            &intervals,
            Some(UnixNanos::from(2000)),
            Some(UnixNanos::from(8000)),
        )
        .unwrap();

    // Should have operations for handling the deletion
    assert!(!operations.is_empty());

    // Verify operation types are valid
    for operation in &operations {
        assert!(matches!(
            operation.operation_type.as_str(),
            "remove" | "split_before" | "split_after"
        ));
    }
}

#[rstest]
fn test_prepare_delete_operations_no_intersection() {
    let (_temp_dir, catalog) = create_temp_catalog();

    // Test with no intersection between intervals and deletion range
    let intervals = vec![(1000, 2000)];

    let operations = catalog
        .prepare_delete_operations(
            "quotes",
            Some("ETH/USDT.BINANCE".to_string()),
            &intervals,
            Some(UnixNanos::from(5000)),
            Some(UnixNanos::from(6000)),
        )
        .unwrap();

    // Should have no operations since no intersection
    assert!(operations.is_empty());
}

#[rstest]
fn test_delete_data_range_nanosecond_precision_boundaries() {
    let (_temp_dir, mut catalog) = create_temp_catalog();

    // Create test data with precise nanosecond timestamps
    let quotes = vec![
        create_quote_tick(1_000_000_000),
        create_quote_tick(1_000_000_001), // +1 nanosecond
        create_quote_tick(1_000_000_002), // +2 nanoseconds
        create_quote_tick(1_000_000_003), // +3 nanoseconds
    ];

    catalog.write_to_parquet(quotes, None, None, None).unwrap();

    // delete exactly the middle two timestamps [1_000_000_001, 1_000_000_002]
    catalog
        .delete_data_range(
            "quotes",
            Some("ETH/USDT.BINANCE".to_string()),
            Some(UnixNanos::from(1_000_000_001)),
            Some(UnixNanos::from(1_000_000_002)),
        )
        .unwrap();

    // should keep only first and last timestamps
    let remaining_data = catalog
        .query_typed_data::<QuoteTick>(None, None, None, None, None, true)
        .unwrap();
    assert_eq!(remaining_data.len(), 2);
    assert_eq!(remaining_data[0].ts_init.as_u64(), 1_000_000_000);
    assert_eq!(remaining_data[1].ts_init.as_u64(), 1_000_000_003);
}

#[rstest]
fn test_delete_data_range_single_file_double_split() {
    let (_temp_dir, mut catalog) = create_temp_catalog();

    // Create test data in a single file that will need both split_before and split_after
    let quotes = vec![
        create_quote_tick(1_000_000_000),
        create_quote_tick(2_000_000_000),
        create_quote_tick(3_000_000_000),
        create_quote_tick(4_000_000_000),
        create_quote_tick(5_000_000_000),
    ];

    catalog.write_to_parquet(quotes, None, None, None).unwrap();

    // delete middle range [2_500_000_000, 3_500_000_000]
    // This should create both split_before and split_after operations
    catalog
        .delete_data_range(
            "quotes",
            Some("ETH/USDT.BINANCE".to_string()),
            Some(UnixNanos::from(2_500_000_000)),
            Some(UnixNanos::from(3_500_000_000)),
        )
        .unwrap();

    // should keep data before and after deletion range
    let remaining_data = catalog
        .query_typed_data::<QuoteTick>(None, None, None, None, None, true)
        .unwrap();
    assert_eq!(remaining_data.len(), 4);

    let timestamps: Vec<u64> = remaining_data.iter().map(|q| q.ts_init.as_u64()).collect();
    assert_eq!(
        timestamps,
        vec![1_000_000_000, 2_000_000_000, 4_000_000_000, 5_000_000_000]
    );
}

#[rstest]
fn test_delete_data_range_saturating_arithmetic_edge_cases() {
    let (_temp_dir, mut catalog) = create_temp_catalog();

    // Test edge case with timestamp 0 and 1
    let quotes = vec![
        create_quote_tick(0),
        create_quote_tick(1),
        create_quote_tick(2),
    ];

    catalog.write_to_parquet(quotes, None, None, None).unwrap();

    // delete range [0, 1] which tests saturating_sub(1) on timestamp 0
    catalog
        .delete_data_range(
            "quotes",
            Some("ETH/USDT.BINANCE".to_string()),
            Some(UnixNanos::from(0)),
            Some(UnixNanos::from(1)),
        )
        .unwrap();

    // should keep only timestamp 2
    let remaining_data = catalog
        .query_typed_data::<QuoteTick>(None, None, None, None, None, true)
        .unwrap();
    assert_eq!(remaining_data.len(), 1);
    assert_eq!(remaining_data[0].ts_init.as_u64(), 2);
}

#[rstest]
fn test_make_local_path() {
    use std::path::PathBuf;

    use nautilus_persistence::backend::catalog::make_local_path;

    // Test basic path construction
    let path = make_local_path("/base", &["data", "quotes", "EURUSD"]);
    let expected = PathBuf::from("/base")
        .join("data")
        .join("quotes")
        .join("EURUSD");
    assert_eq!(path, expected);

    // Test empty base path
    let path = make_local_path("", &["data", "quotes"]);
    let expected = PathBuf::from("data").join("quotes");
    assert_eq!(path, expected);

    // Test single component
    let path = make_local_path("/base", &["data"]);
    let expected = PathBuf::from("/base").join("data");
    assert_eq!(path, expected);
}

#[rstest]
fn test_safe_directory_identifier() {
    use nautilus_persistence::backend::catalog::safe_directory_identifier;

    assert_eq!(safe_directory_identifier("foo//bar"), "foo/bar");
    assert_eq!(safe_directory_identifier("foo/bar"), "foo/bar");
    assert_eq!(safe_directory_identifier("foo/../bar"), "foo/bar");
    assert_eq!(safe_directory_identifier(""), "");
    assert_eq!(safe_directory_identifier("RUST.TEST"), "RUST.TEST");
}

#[rstest]
fn test_make_object_store_path() {
    use nautilus_persistence::backend::catalog::make_object_store_path;

    // Test basic path construction
    let path = make_object_store_path("base", &["data", "quotes", "EURUSD"]);
    assert_eq!(path, "base/data/quotes/EURUSD");

    // Test empty base path
    let path = make_object_store_path("", &["data", "quotes"]);
    assert_eq!(path, "data/quotes");

    // Test with trailing slashes in base
    let path = make_object_store_path("base/", &["data", "quotes"]);
    assert_eq!(path, "base/data/quotes");

    // Test with leading slashes in components
    let path = make_object_store_path("base", &["/data", "/quotes"]);
    assert_eq!(path, "base/data/quotes");

    // Test with backslashes (Windows-style)
    let path = make_object_store_path("base\\", &["data\\", "quotes"]);
    assert_eq!(path, "base/data/quotes");
}

#[rstest]
fn test_make_object_store_path_owned() {
    use nautilus_persistence::backend::catalog::make_object_store_path_owned;

    // Test with owned strings
    let components = vec![
        "data".to_string(),
        "quotes".to_string(),
        "EURUSD".to_string(),
    ];
    let path = make_object_store_path_owned("base", components);
    assert_eq!(path, "base/data/quotes/EURUSD");

    // Test empty base path
    let components = vec!["data".to_string(), "quotes".to_string()];
    let path = make_object_store_path_owned("", components);
    assert_eq!(path, "data/quotes");
}

#[rstest]
fn test_local_to_object_store_path() {
    use std::path::PathBuf;

    use nautilus_persistence::backend::catalog::local_to_object_store_path;

    // Test Unix-style path
    let local_path = PathBuf::from("data").join("quotes").join("EURUSD");
    let object_path = local_to_object_store_path(&local_path);
    assert_eq!(object_path, "data/quotes/EURUSD");

    // Test with backslashes (simulating Windows)
    let path_str = "data\\quotes\\EURUSD";
    let local_path = PathBuf::from(path_str);
    let object_path = local_to_object_store_path(&local_path);
    // Should normalize backslashes to forward slashes
    assert!(object_path.contains('/') || !object_path.contains('\\'));
}

#[rstest]
fn test_extract_path_components() {
    use nautilus_persistence::backend::catalog::extract_path_components;

    // Test Unix-style path
    let components = extract_path_components("data/quotes/EURUSD");
    assert_eq!(components, vec!["data", "quotes", "EURUSD"]);

    // Test Windows-style path
    let components = extract_path_components("data\\quotes\\EURUSD");
    assert_eq!(components, vec!["data", "quotes", "EURUSD"]);

    // Test mixed separators
    let components = extract_path_components("data/quotes\\EURUSD");
    assert_eq!(components, vec!["data", "quotes", "EURUSD"]);

    // Test with leading/trailing separators
    let components = extract_path_components("/data/quotes/");
    assert_eq!(components, vec!["data", "quotes"]);

    // Test empty path
    let components = extract_path_components("");
    assert!(components.is_empty());
}

#[rstest]
fn test_extract_identifier_from_path() {
    use nautilus_persistence::backend::catalog::extract_identifier_from_path;

    // Test typical parquet file path
    let identifier = extract_identifier_from_path("data/quote_tick/EURUSD/file.parquet");
    assert_eq!(identifier, "EURUSD");

    // Test bar file path
    let identifier =
        extract_identifier_from_path("data/bar/BTCUSD-1-MINUTE-LAST-EXTERNAL/file.parquet");
    assert_eq!(identifier, "BTCUSD-1-MINUTE-LAST-EXTERNAL");

    // Test path with fewer components
    let identifier = extract_identifier_from_path("EURUSD/file.parquet");
    assert_eq!(identifier, "EURUSD");

    // Test path with single component
    let identifier = extract_identifier_from_path("file.parquet");
    assert_eq!(identifier, "unknown");

    // Test empty path
    let identifier = extract_identifier_from_path("");
    assert_eq!(identifier, "unknown");
}

#[rstest]
fn test_make_sql_safe_identifier() {
    use nautilus_persistence::backend::catalog::make_sql_safe_identifier;

    // Test identifier with forward slash
    let safe_id = make_sql_safe_identifier("EUR/USD");
    assert_eq!(safe_id, "eurusd");

    // Test identifier with dots and hyphens
    let safe_id = make_sql_safe_identifier("BTC-USD.COINBASE");
    assert_eq!(safe_id, "btc_usd_coinbase");

    // Test complex bar type identifier
    let safe_id = make_sql_safe_identifier("BTCUSD-1-MINUTE-LAST-EXTERNAL");
    assert_eq!(safe_id, "btcusd_1_minute_last_external");

    // Test already safe identifier
    let safe_id = make_sql_safe_identifier("btcusd");
    assert_eq!(safe_id, "btcusd");

    // Test mixed case with special characters
    let safe_id = make_sql_safe_identifier("ETH/USDT.Binance-Spot");
    assert_eq!(safe_id, "ethusdt_binance_spot");

    // Test identifier with spaces (like option symbols)
    let safe_id = make_sql_safe_identifier("ESM4 P5230.XCME");
    assert_eq!(safe_id, "esm4_p5230_xcme");
}

#[rstest]
fn test_extract_sql_safe_filename() {
    use nautilus_persistence::backend::catalog::extract_sql_safe_filename;

    // Test actual timestamp range filename format
    let filename = extract_sql_safe_filename(
        "data/quote_tick/EURUSD/2021-01-01T00-00-00-000000000Z_2021-01-02T00-00-00-000000000Z.parquet",
    );
    assert_eq!(
        filename,
        "2021_01_01t00_00_00_000000000z_2021_01_02t00_00_00_000000000z"
    );

    // Test bar filename with timestamp range
    let filename = extract_sql_safe_filename(
        "data/bar/BTCUSD/2021-01-01T00-00-00-000000000Z_2021-01-01T23-59-59-999999999Z.parquet",
    );
    assert_eq!(
        filename,
        "2021_01_01t00_00_00_000000000z_2021_01_01t23_59_59_999999999z"
    );

    // Test filename with various problematic characters
    let filename = extract_sql_safe_filename("path/to/data-file:with.dots.parquet");
    assert_eq!(filename, "data_file_with_dots");

    // Test simple filename
    let filename = extract_sql_safe_filename("simple_file.parquet");
    assert_eq!(filename, "simple_file");

    // Test filename without extension
    let filename = extract_sql_safe_filename("path/to/datafile");
    assert_eq!(filename, "datafile");

    // Test empty path
    let filename = extract_sql_safe_filename("");
    assert_eq!(filename, "unknown_file");
}

#[rstest]
fn test_catalog_query_multiple_instruments_table_naming() {
    // Test that querying multiple instruments with different identifiers works correctly
    // This verifies the table naming fix for identifier-dependent table names

    let temp_dir = TempDir::new().unwrap();
    let mut catalog =
        ParquetDataCatalog::new(temp_dir.path().to_path_buf(), None, None, None, None);

    // Create quote ticks for multiple instruments with different identifier patterns
    let eurusd_quotes = create_quote_ticks_for_instrument("EUR/USD.SIM", 1000, 3);
    let btcusd_quotes = create_quote_ticks_for_instrument("BTC-USD.COINBASE", 2000, 3);
    let ethusdt_quotes = create_quote_ticks_for_instrument("ETH/USDT.BINANCE", 3000, 3);

    // Write data for all instruments
    catalog
        .write_to_parquet(eurusd_quotes, None, None, None)
        .unwrap();
    catalog
        .write_to_parquet(btcusd_quotes, None, None, None)
        .unwrap();
    catalog
        .write_to_parquet(ethusdt_quotes, None, None, None)
        .unwrap();

    // Query all instruments simultaneously
    let instrument_ids = vec![
        "EUR/USD.SIM".to_string(),
        "BTC-USD.COINBASE".to_string(),
        "ETH/USDT.BINANCE".to_string(),
    ];

    let result = catalog.query::<QuoteTick>(Some(instrument_ids), None, None, None, None, true);
    assert!(
        result.is_ok(),
        "Query should succeed with multiple instruments"
    );

    let query_result = result.unwrap();
    let data: Vec<Data> = query_result.collect();

    // Should get all 9 quotes (3 from each instrument)
    assert_eq!(data.len(), 9);

    // Verify we have data from all three instruments
    let mut instrument_counts = HashMap::new();
    for item in &data {
        if let Data::Quote(quote) = item {
            *instrument_counts
                .entry(quote.instrument_id.to_string())
                .or_insert(0) += 1;
        }
    }

    assert_eq!(instrument_counts.len(), 3);
    assert_eq!(instrument_counts.get("EUR/USD.SIM"), Some(&3));
    assert_eq!(instrument_counts.get("BTC-USD.COINBASE"), Some(&3));
    assert_eq!(instrument_counts.get("ETH/USDT.BINANCE"), Some(&3));

    // Verify data is properly ordered by timestamp
    assert!(is_monotonically_increasing_by_init(&data));
}

#[rstest]
fn test_query_directory_based_registration() {
    // Test that directory-based registration (optimize_file_loading=true) reads all files in directory
    let temp_dir = TempDir::new().unwrap();
    let mut catalog =
        ParquetDataCatalog::new(temp_dir.path().to_path_buf(), None, None, None, None);

    // Create multiple batches of quotes for the same instrument with disjoint timestamp ranges
    // Each batch needs non-overlapping timestamps to create separate files
    let instrument_id = "EUR/USD.SIM";
    let batch1 = create_quote_ticks_for_instrument(instrument_id, 1000, 3);
    let batch2 = create_quote_ticks_for_instrument(instrument_id, 10000, 3); // Large gap to ensure disjoint
    let batch3 = create_quote_ticks_for_instrument(instrument_id, 20000, 3); // Large gap to ensure disjoint

    // Write each batch separately to create multiple files
    catalog.write_to_parquet(batch1, None, None, None).unwrap();
    catalog.write_to_parquet(batch2, None, None, None).unwrap();
    catalog.write_to_parquet(batch3, None, None, None).unwrap();

    // Query with directory-based registration (default)
    let result = catalog.query::<QuoteTick>(
        Some(vec![instrument_id.to_string()]),
        None,
        None,
        None,
        None,
        true, // optimize_file_loading = true (directory-based)
    );

    assert!(
        result.is_ok(),
        "Query should succeed with directory-based registration"
    );

    let query_result = result.unwrap();
    let data: Vec<Data> = query_result.collect();

    // Should get all 9 quotes from all 3 files in the directory
    assert_eq!(data.len(), 9, "Should read all files in directory");

    // Verify data is properly ordered
    assert!(is_monotonically_increasing_by_init(&data));
}

#[rstest]
fn test_query_file_based_registration() {
    // Test that file-based registration (optimize_file_loading=false) only reads specified files
    let temp_dir = TempDir::new().unwrap();
    let mut catalog =
        ParquetDataCatalog::new(temp_dir.path().to_path_buf(), None, None, None, None);

    // Create multiple batches of quotes for the same instrument with disjoint timestamp ranges
    let instrument_id = "GBP/USD.SIM";
    let batch1 = create_quote_ticks_for_instrument(instrument_id, 1000, 3);
    let batch2 = create_quote_ticks_for_instrument(instrument_id, 10000, 3); // Large gap to ensure disjoint
    let batch3 = create_quote_ticks_for_instrument(instrument_id, 20000, 3); // Large gap to ensure disjoint

    // Write each batch separately to create multiple files
    catalog.write_to_parquet(batch1, None, None, None).unwrap();
    catalog.write_to_parquet(batch2, None, None, None).unwrap();
    catalog.write_to_parquet(batch3, None, None, None).unwrap();

    // Get all files for this instrument
    let all_files = catalog
        .query_files("quotes", Some(vec![instrument_id.to_string()]), None, None)
        .unwrap();
    assert_eq!(all_files.len(), 3, "Should have 3 files");

    // Query with file-based registration, specifying only the first file
    let selected_files = vec![all_files[0].clone()];
    let result = catalog.query::<QuoteTick>(
        Some(vec![instrument_id.to_string()]),
        None,
        None,
        None,
        Some(selected_files),
        false, // optimize_file_loading = false (file-based)
    );

    assert!(
        result.is_ok(),
        "Query should succeed with file-based registration"
    );

    let query_result = result.unwrap();
    let data: Vec<Data> = query_result.collect();

    // Should only get 3 quotes from the first file
    assert_eq!(data.len(), 3, "Should only read the specified file");
    assert!(is_monotonically_increasing_by_init(&data));
}

#[rstest]
fn test_query_directory_based_vs_file_based() {
    // Test that directory-based and file-based registration produce same results when all files are specified
    let temp_dir = TempDir::new().unwrap();
    let mut catalog =
        ParquetDataCatalog::new(temp_dir.path().to_path_buf(), None, None, None, None);

    // Create multiple batches of quotes with disjoint timestamp ranges
    let instrument_id = "AUD/USD.SIM";
    let batch1 = create_quote_ticks_for_instrument(instrument_id, 1000, 2);
    let batch2 = create_quote_ticks_for_instrument(instrument_id, 10000, 2); // Large gap to ensure disjoint

    catalog.write_to_parquet(batch1, None, None, None).unwrap();
    catalog.write_to_parquet(batch2, None, None, None).unwrap();

    // Get all files
    let all_files = catalog
        .query_files("quotes", Some(vec![instrument_id.to_string()]), None, None)
        .unwrap();

    // Query with directory-based registration
    let result_dir = catalog.query::<QuoteTick>(
        Some(vec![instrument_id.to_string()]),
        None,
        None,
        None,
        None,
        true, // directory-based
    );
    let data_dir: Vec<Data> = result_dir.unwrap().collect();

    // Query with file-based registration (all files)
    let result_file = catalog.query::<QuoteTick>(
        Some(vec![instrument_id.to_string()]),
        None,
        None,
        None,
        Some(all_files),
        false, // file-based
    );
    let data_file: Vec<Data> = result_file.unwrap().collect();

    // Both should return the same data
    assert_eq!(data_dir.len(), data_file.len());
    assert_eq!(data_dir.len(), 4); // 2 + 2 quotes
    assert!(is_monotonically_increasing_by_init(&data_dir));
    assert!(is_monotonically_increasing_by_init(&data_file));
}

#[rstest]
fn test_rust_custom_data_roundtrip() {
    use std::sync::Arc;

    use nautilus_model::{
        data::{CustomData, Data, DataType},
        identifiers::InstrumentId,
    };

    ensure_test_custom_data_registered();
    let (_temp_dir, mut catalog) = create_temp_catalog();

    let instrument_id = InstrumentId::from("RUST.TEST");
    let data_type = DataType::new("RustTestCustomData", None, Some(instrument_id.to_string()));

    let original_data = [
        RustTestCustomData {
            instrument_id,
            value: 1.23,
            flag: true,
            ts_event: UnixNanos::from(1),
            ts_init: UnixNanos::from(1),
        },
        RustTestCustomData {
            instrument_id,
            value: 4.56,
            flag: false,
            ts_event: UnixNanos::from(2),
            ts_init: UnixNanos::from(2),
        },
    ];

    // Write as CustomData with identifier so path is custom/type/identifier
    let custom_data: Vec<CustomData> = original_data
        .iter()
        .cloned()
        .map(|item| CustomData::new(Arc::new(item), data_type.clone()))
        .collect();

    catalog
        .write_custom_data_batch(custom_data, None, None, Some(false))
        .unwrap();

    // Read back via dynamic custom data query
    let loaded: Vec<Data> = catalog
        .query_custom_data_dynamic(
            "RustTestCustomData",
            Some(vec![instrument_id.to_string()]),
            None,
            None,
            None,
            None,
            true,
        )
        .unwrap();

    assert_eq!(loaded.len(), original_data.len());

    for (expected, actual) in original_data.iter().zip(loaded.iter()) {
        if let Data::Custom(custom) = actual {
            assert_eq!(custom.data_type.type_name(), "RustTestCustomData");
            assert_eq!(
                custom.data_type.identifier(),
                Some(instrument_id.to_string().as_str())
            );
            let rust: &RustTestCustomData = custom
                .data
                .as_any()
                .downcast_ref::<RustTestCustomData>()
                .expect("Expected RustTestCustomData");
            assert_eq!(expected, rust);
        } else {
            panic!("Expected Data::Custom variant");
        }
    }
}

/// Regression: write_data_enum groups custom data by full DataType (type_name + identifier + metadata).
/// Same type_name with different identifiers must produce separate batches and be readable back.
#[rstest]
fn test_write_data_enum_mixed_custom_data_identifiers() {
    use std::sync::Arc;

    use nautilus_model::{
        data::{CustomData, Data, DataType},
        identifiers::InstrumentId,
    };

    ensure_test_custom_data_registered();
    let (_temp_dir, mut catalog) = create_temp_catalog();

    let id_a = InstrumentId::from("RUST.A");
    let id_b = InstrumentId::from("RUST.B");
    let data_type_a = DataType::new("RustTestCustomData", None, Some(id_a.to_string()));
    let data_type_b = DataType::new("RustTestCustomData", None, Some(id_b.to_string()));

    let custom_a = [
        RustTestCustomData {
            instrument_id: id_a,
            value: 1.0,
            flag: true,
            ts_event: UnixNanos::from(1),
            ts_init: UnixNanos::from(1),
        },
        RustTestCustomData {
            instrument_id: id_a,
            value: 2.0,
            flag: false,
            ts_event: UnixNanos::from(2),
            ts_init: UnixNanos::from(2),
        },
    ];
    let custom_b = [RustTestCustomData {
        instrument_id: id_b,
        value: 10.0,
        flag: true,
        ts_event: UnixNanos::from(10),
        ts_init: UnixNanos::from(10),
    }];

    let data: Vec<Data> = custom_a
        .iter()
        .cloned()
        .map(|item| Data::Custom(CustomData::new(Arc::new(item), data_type_a.clone())))
        .chain(
            custom_b
                .iter()
                .cloned()
                .map(|item| Data::Custom(CustomData::new(Arc::new(item), data_type_b.clone()))),
        )
        .collect();

    catalog
        .write_data_enum(data, None, None, Some(false))
        .unwrap();

    let loaded_a: Vec<Data> = catalog
        .query_custom_data_dynamic(
            "RustTestCustomData",
            Some(vec![id_a.to_string()]),
            None,
            None,
            None,
            None,
            true,
        )
        .unwrap();
    let loaded_b: Vec<Data> = catalog
        .query_custom_data_dynamic(
            "RustTestCustomData",
            Some(vec![id_b.to_string()]),
            None,
            None,
            None,
            None,
            true,
        )
        .unwrap();

    assert_eq!(loaded_a.len(), 2, "identifier A should have 2 items");
    assert_eq!(loaded_b.len(), 1, "identifier B should have 1 item");
}

#[rstest]
#[cfg(feature = "python")]
fn test_macro_yield_curve_data_roundtrip() {
    use std::sync::Arc;

    use nautilus_model::data::{CustomData, Data, DataType};

    ensure_test_custom_data_registered();
    let (_temp_dir, mut catalog) = create_temp_catalog();

    let data_type = DataType::new("MacroYieldCurveData", None, None);

    let tenors = vec![0.25, 0.5, 1.0, 2.0, 5.0];
    let interest_rates = vec![0.025, 0.03, 0.035, 0.04, 0.045];

    let original_data = [
        MacroYieldCurveData {
            curve_name: "USD".to_string(),
            tenors,
            interest_rates,
            ts_event: UnixNanos::from(1),
            ts_init: UnixNanos::from(1),
        },
        MacroYieldCurveData {
            curve_name: "EUR".to_string(),
            tenors: vec![1.0, 2.0],
            interest_rates: vec![0.02, 0.025],
            ts_event: UnixNanos::from(2),
            ts_init: UnixNanos::from(2),
        },
    ];

    let custom_data: Vec<CustomData> = original_data
        .iter()
        .cloned()
        .map(|item| CustomData::new(Arc::new(item), data_type.clone()))
        .collect();

    catalog
        .write_custom_data_batch(custom_data, None, None, Some(false))
        .unwrap();

    let loaded: Vec<Data> = catalog
        .query_custom_data_dynamic("MacroYieldCurveData", None, None, None, None, None, true)
        .unwrap();

    assert_eq!(loaded.len(), original_data.len());

    for (expected, actual) in original_data.iter().zip(loaded.iter()) {
        if let Data::Custom(custom) = actual {
            assert_eq!(custom.data_type.type_name(), "MacroYieldCurveData");
            let macro_curve: &MacroYieldCurveData = custom
                .data
                .as_any()
                .downcast_ref::<MacroYieldCurveData>()
                .expect("Expected MacroYieldCurveData");
            assert_eq!(expected, macro_curve);
        } else {
            panic!("Expected Data::Custom variant");
        }
    }
}

#[rstest]
#[cfg(feature = "cloud")]
fn test_query_directory_based_registration_with_cloud_uri() {
    // Test that directory-based registration works with cloud storage URIs
    // This test verifies that the directory path handling works correctly for remote URIs
    // Note: This creates a catalog with a cloud URI but doesn't actually connect to cloud storage
    // It verifies that the URI reconstruction and directory path handling works correctly

    // Create a catalog with an S3 URI (won't actually connect, but tests the logic)
    let s3_catalog_result =
        ParquetDataCatalog::from_uri("s3://test-bucket/catalog", None, None, None, None);

    // The catalog creation might fail if cloud features aren't properly configured,
    // but we can at least verify the URI detection works
    if let Ok(catalog) = s3_catalog_result {
        // Verify it's detected as a remote URI
        assert!(
            catalog.is_remote_uri(),
            "S3 URI should be detected as remote"
        );

        // Test that directory path reconstruction works for cloud URIs
        let test_directory = "data/quotes/EURUSD";
        let reconstructed = catalog.reconstruct_full_uri(test_directory);

        // For S3, the reconstructed URI should be: s3://test-bucket/data/quotes/EURUSD
        assert!(
            reconstructed.starts_with("s3://"),
            "Reconstructed URI should start with s3://"
        );
        assert!(
            reconstructed.contains("test-bucket"),
            "Reconstructed URI should contain bucket name"
        );
        assert!(
            reconstructed.contains("data/quotes/EURUSD"),
            "Reconstructed URI should contain the directory path"
        );
    }

    // Also test with a local path to ensure directory-based registration works there too
    let temp_dir = TempDir::new().unwrap();
    let mut local_catalog =
        ParquetDataCatalog::new(temp_dir.path().to_path_buf(), None, None, None, None);

    // Create multiple batches of quotes with disjoint timestamp ranges
    let instrument_id = "USD/JPY.SIM";
    let batch1 = create_quote_ticks_for_instrument(instrument_id, 1000, 2);
    let batch2 = create_quote_ticks_for_instrument(instrument_id, 10000, 2); // Large gap to ensure disjoint

    // Write each batch separately to create multiple files
    local_catalog
        .write_to_parquet(batch1, None, None, None)
        .unwrap();
    local_catalog
        .write_to_parquet(batch2, None, None, None)
        .unwrap();

    // Verify that directory-based registration works with local paths
    let result = local_catalog.query::<QuoteTick>(
        Some(vec![instrument_id.to_string()]),
        None,
        None,
        None,
        None,
        true, // optimize_file_loading = true (directory-based)
    );

    assert!(
        result.is_ok(),
        "Query should succeed with directory-based registration on local path"
    );

    let query_result = result.unwrap();
    let data: Vec<Data> = query_result.collect();

    // Should get all 4 quotes from both files in the directory
    assert_eq!(data.len(), 4, "Should read all files in directory");
    assert!(is_monotonically_increasing_by_init(&data));
}

#[rstest]
fn test_duplicate_table_registration() {
    // Test that registering the same table twice doesn't cause duplicate data
    let mut session = DataBackendSession::new(1000);
    let file_path = get_nautilus_test_data_file_path("quotes.parquet");

    // First registration
    session
        .add_file::<QuoteTick>("test_table", file_path.as_str(), None, None)
        .unwrap();

    // Second registration of the same table (should not add duplicate data)
    session
        .add_file::<QuoteTick>("test_table", file_path.as_str(), None, None)
        .unwrap();

    let query_result: QueryResult = session.get_query_result();
    let data: Vec<Data> = query_result.collect();

    // Should only get data once, not duplicated
    // The quotes.parquet file contains 9500 quotes
    assert_eq!(data.len(), 9500);
    assert!(is_monotonically_increasing_by_init(&data));
}

#[rstest]
fn test_query_typed_data_repeated_calls() {
    let (_temp_dir, mut catalog) = create_temp_catalog();
    let quotes = vec![create_quote_tick(1000), create_quote_tick(2000)];
    catalog.write_to_parquet(quotes, None, None, None).unwrap();

    let result1 = catalog
        .query_typed_data::<QuoteTick>(
            Some(vec!["ETH/USDT.BINANCE".to_string()]),
            None,
            None,
            None,
            None,
            true, // optimize_file_loading=true (default)
        )
        .unwrap();
    assert_eq!(result1.len(), 2);

    // This was returning empty before the fix
    let result2 = catalog
        .query_typed_data::<QuoteTick>(
            Some(vec!["ETH/USDT.BINANCE".to_string()]),
            None,
            None,
            None,
            None,
            true, // optimize_file_loading=true (default)
        )
        .unwrap();
    assert_eq!(result2.len(), 2);

    let result3 = catalog
        .query_typed_data::<QuoteTick>(
            Some(vec!["ETH/USDT.BINANCE".to_string()]),
            None,
            None,
            None,
            None,
            true, // optimize_file_loading=true (default)
        )
        .unwrap();
    assert_eq!(result3.len(), 2);

    assert_eq!(result1[0].ts_init, result2[0].ts_init);
    assert_eq!(result1[1].ts_init, result2[1].ts_init);
    assert_eq!(result2[0].ts_init, result3[0].ts_init);
    assert_eq!(result2[1].ts_init, result3[1].ts_init);
}

#[rstest]
fn test_write_skips_if_file_exists() {
    let (_temp_dir, catalog) = create_temp_catalog();

    // Write initial data
    let bars1 = vec![create_bar(1), create_bar(2)];
    let path1 = catalog.write_to_parquet(bars1, None, None, None).unwrap();

    // Attempt to write same interval again (same timestamps)
    let bars2 = vec![create_bar(1), create_bar(2)];
    let path2 = catalog.write_to_parquet(bars2, None, None, None).unwrap();

    // Should return the same path (file exists, write skipped)
    assert_eq!(path1, path2);

    // Verify only one file exists
    let bar_type = create_bar(1).bar_type.to_string();
    let intervals = catalog.get_intervals("bars", Some(bar_type)).unwrap();
    assert_eq!(intervals, vec![(1, 2)]);
}

#[rstest]
fn test_write_errors_on_overlapping_intervals() {
    let (_temp_dir, catalog) = create_temp_catalog();

    // Write initial data with interval (1, 5)
    let bars1 = vec![create_bar(1), create_bar(5)];
    catalog.write_to_parquet(bars1, None, None, None).unwrap();

    // Attempt to write overlapping interval (3, 7) - should fail
    let bars2 = vec![create_bar(3), create_bar(7)];
    let result = catalog.write_to_parquet(bars2, None, None, None);

    assert!(result.is_err());
    let err_msg = result.unwrap_err().to_string();
    assert!(err_msg.contains("non-disjoint intervals"));
}

#[rstest]
fn test_write_succeeds_with_disjoint_intervals() {
    let (_temp_dir, catalog) = create_temp_catalog();

    // Write first interval (1, 2)
    let bars1 = vec![create_bar(1), create_bar(2)];
    let bar_type = bars1[0].bar_type.to_string();
    catalog.write_to_parquet(bars1, None, None, None).unwrap();

    // Write non-overlapping interval (5, 6) - should succeed
    let bars2 = vec![create_bar(5), create_bar(6)];
    catalog.write_to_parquet(bars2, None, None, None).unwrap();
    let intervals = catalog.get_intervals("bars", Some(bar_type)).unwrap();
    assert_eq!(intervals, vec![(1, 2), (5, 6)]);
}

#[rstest]
fn test_write_with_skip_disjoint_check() {
    let (_temp_dir, catalog) = create_temp_catalog();

    // Write initial data with interval (1, 5)
    let bars1 = vec![create_bar(1), create_bar(5)];
    catalog.write_to_parquet(bars1, None, None, None).unwrap();

    // Write overlapping interval with skip_disjoint_check=true - should succeed
    let bars2 = vec![create_bar(3), create_bar(7)];
    let result = catalog.write_to_parquet(bars2, None, None, Some(true));

    // Should succeed because we skipped the check
    assert!(result.is_ok());
}

#[rstest]
fn test_query_first_timestamp() {
    let (_temp_dir, catalog) = create_temp_catalog();

    // Write some bars
    let bars = vec![create_bar(1000), create_bar(2000), create_bar(3000)];
    catalog
        .write_to_parquet(bars.clone(), None, None, None)
        .unwrap();

    let bar_type = bars[0].bar_type.to_string();
    // Query first timestamp
    let first_ts = catalog
        .query_first_timestamp("bars", Some(bar_type))
        .unwrap();

    assert!(first_ts.is_some());
    assert_eq!(first_ts.unwrap(), 1000);
}

#[rstest]
fn test_query_first_timestamp_empty() {
    let (_temp_dir, catalog) = create_temp_catalog();

    let bar_type = create_bar(1).bar_type.to_string();
    // Query first timestamp when no data exists
    let first_ts = catalog
        .query_first_timestamp("bars", Some(bar_type))
        .unwrap();

    assert!(first_ts.is_none());
}

#[rstest]
fn test_query_last_timestamp() {
    let (_temp_dir, catalog) = create_temp_catalog();

    // Write some bars
    let bars = vec![create_bar(1000), create_bar(2000), create_bar(3000)];
    catalog
        .write_to_parquet(bars.clone(), None, None, None)
        .unwrap();

    let bar_type = bars[0].bar_type.to_string();
    // Query last timestamp
    let last_ts = catalog
        .query_last_timestamp("bars", Some(bar_type))
        .unwrap();

    assert!(last_ts.is_some());
    assert_eq!(last_ts.unwrap(), 3000);
}

#[rstest]
fn test_list_data_types() {
    let (_temp_dir, catalog) = create_temp_catalog();

    // Initially should be empty or have no data types
    let data_types = catalog.list_data_types().unwrap();
    assert!(data_types.is_empty() || !data_types.contains(&"bars".to_string()));

    // Write some data
    let bars = vec![create_bar(1000)];
    catalog.write_to_parquet(bars, None, None, None).unwrap();

    // Now should have bars
    let data_types = catalog.list_data_types().unwrap();
    assert!(data_types.contains(&"bars".to_string()));
}

#[rstest]
fn test_list_backtest_runs() {
    let (temp_dir, catalog) = create_temp_catalog();

    // Initially should be empty
    let runs = catalog.list_backtest_runs().unwrap();
    assert!(runs.is_empty());

    // Create a backtest directory manually (simulating a backtest run)
    // Need to create a file inside so the directory is visible to object store listing
    let backtest_dir = temp_dir.path().join("backtest").join("test_run_123");
    fs::create_dir_all(&backtest_dir).unwrap();
    // Create a dummy file so the directory shows up in listing
    let dummy_file = backtest_dir.join("dummy.txt");
    let mut file = fs::File::create(&dummy_file).unwrap();
    file.write_all(b"test").unwrap();

    // Now should list the run
    let runs = catalog.list_backtest_runs().unwrap();
    assert!(runs.contains(&"test_run_123".to_string()));
}

#[rstest]
fn test_list_live_runs() {
    let (temp_dir, catalog) = create_temp_catalog();

    // Initially should be empty
    let runs = catalog.list_live_runs().unwrap();
    assert!(runs.is_empty());

    // Create a live directory manually (simulating a live run)
    // Need to create a file inside so the directory is visible to object store listing
    let live_dir = temp_dir.path().join("live").join("test_live_456");
    fs::create_dir_all(&live_dir).unwrap();
    // Create a dummy file so the directory shows up in listing
    let dummy_file = live_dir.join("dummy.txt");
    let mut file = fs::File::create(&dummy_file).unwrap();
    file.write_all(b"test").unwrap();

    // Now should list the run
    let runs = catalog.list_live_runs().unwrap();
    assert!(runs.contains(&"test_live_456".to_string()));
}

#[rstest]
fn test_convert_stream_to_data_no_files() {
    let (_temp_dir, mut catalog) = create_temp_catalog();

    // Should return Ok(()) when no files are found (not an error)
    let result =
        catalog.convert_stream_to_data("test_instance", "quotes", Some("backtest"), None, false);

    assert!(result.is_ok(), "Should return Ok when no files found");
}

#[rstest]
fn test_instrument_roundtrip_with_info_params() {
    // Roundtrip an instrument with info (Params) through the Rust catalog to ensure
    // Params serialization/deserialization is correct.
    let (_temp_dir, catalog) = create_temp_catalog();

    let mut info = Params::new();
    info.insert("venue_extra".to_string(), json!("custom_value"));
    info.insert("count".to_string(), json!(42_u64));
    info.insert("enabled".to_string(), json!(true));

    let instrument_id = InstrumentId::from("AUD/USD.SIM");
    let currency_pair = CurrencyPair::new(
        instrument_id,
        Symbol::from("AUD/USD"),
        Currency::from("AUD"),
        Currency::from("USD"),
        5,
        0,
        Price::new(0.00001, 5),
        Quantity::new(1.0, 0),
        None, // multiplier
        None, // lot_size
        None, // max_quantity
        None, // min_quantity
        None, // max_notional
        None, // min_notional
        None, // max_price
        None, // min_price
        Some(Decimal::from(3) / Decimal::from(100)),
        Some(Decimal::from(3) / Decimal::from(100)),
        Some(Decimal::from(2) / Decimal::from(100_000)),
        Some(Decimal::from(2) / Decimal::from(100_000)),
        Some(info.clone()),
        UnixNanos::default(),
        UnixNanos::default(),
    );

    let instrument_any = InstrumentAny::CurrencyPair(currency_pair);
    let id_str = Instrument::id(&instrument_any).to_string();

    catalog.write_instruments(vec![instrument_any]).unwrap();

    let read = catalog.query_instruments(Some(vec![id_str])).unwrap();
    assert_eq!(read.len(), 1, "Should read back exactly one instrument");

    let read_any = &read[0];
    let InstrumentAny::CurrencyPair(read_cp) = read_any else {
        panic!("Expected CurrencyPair");
    };
    assert_eq!(read_cp.id, instrument_id);
    assert_eq!(
        read_cp.info,
        Some(info),
        "info (Params) must roundtrip unchanged"
    );
}
