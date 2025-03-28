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

use std::path::PathBuf;

use nautilus_core::{ffi::cvec::CVec, python::IntoPyObjectNautilusExt};
use nautilus_model::data::{
    Bar, Data, OrderBookDelta, QuoteTick, TradeTick, is_monotonically_increasing_by_init,
    to_variant,
};
use nautilus_persistence::{
    backend::{
        catalog::ParquetDataCatalog,
        session::{DataBackendSession, DataQueryResult, QueryResult},
    },
    python::backend::session::NautilusDataType,
};
use nautilus_serialization::arrow::ArrowSchemaProvider;
use nautilus_test_kit::common::get_nautilus_test_data_file_path;
#[cfg(target_os = "linux")]
use procfs::{self, process::Process};
use pyo3::{prelude::*, types::PyCapsule};
use rstest::rstest;

/// Memory leak test
///
/// Uses arguments from setup to run function for given number of iterations.
/// Checks that the difference between memory after 1 and iter + 1 runs is
/// less than threshold.
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

#[cfg(target_os = "linux")]
#[rstest]
fn catalog_query_mem_leak_test() {
    mem_leak_test(
        pyo3::prepare_freethreaded_python,
        |_args| {
            let file_path = get_nautilus_test_data_file_path("quotes.parquet");

            let expected_length = 9500;
            let catalog = DataBackendSession::new(1_000_000);
            Python::with_gil(|py| {
                let pycatalog: Py<PyAny> = catalog.into_py_any_unwrap(py);
                pycatalog
                    .call_method1(
                        py,
                        "add_file",
                        (
                            NautilusDataType::QuoteTick,
                            "order_book_deltas",
                            file_path.as_str(),
                        ),
                    )
                    .unwrap();
                let result = pycatalog.call_method0(py, "to_query_result").unwrap();
                let mut count = 0;
                while let Ok(chunk) = result.call_method0(py, "__next__") {
                    let capsule: &Bound<'_, _> = chunk.downcast_bound(py).unwrap();
                    let cvec: &CVec = unsafe { &*(capsule.pointer() as *const CVec) };
                    if cvec.len == 0 {
                        break;
                    } else {
                        let slice: &[Data] = unsafe {
                            std::slice::from_raw_parts(cvec.ptr as *const Data, cvec.len)
                        };
                        count += slice.len();
                        assert!(is_monotonically_increasing_by_init(slice));
                    }
                }

                assert_eq!(expected_length, count);
            });
        },
        1.0,
        5,
    );
}

#[rstest]

fn test_quote_tick_cvec_interface() {
    let file_path = get_nautilus_test_data_file_path("quotes.parquet");

    let expected_length = 9500;
    let mut catalog = DataBackendSession::new(1000);
    catalog
        .add_file::<QuoteTick>("quote_005", file_path.as_str(), None)
        .unwrap();
    let query_result: QueryResult = catalog.get_query_result();
    let query_result = DataQueryResult::new(query_result, catalog.chunk_size);
    let mut count = 0;
    for chunk in query_result {
        if chunk.is_empty() {
            break;
        }
        let chunk: CVec = chunk.into();
        let ticks: &[Data] =
            unsafe { std::slice::from_raw_parts(chunk.ptr as *const Data, chunk.len) };
        count += ticks.len();
        assert!(is_monotonically_increasing_by_init(ticks));

        // Cleanly drop to avoid leaking memory in test
        let CVec { ptr, len, cap } = chunk;
        let data: Vec<Data> = unsafe { Vec::from_raw_parts(ptr.cast::<Data>(), len, cap) };
        drop(data);
    }

    assert_eq!(expected_length, count);
}

#[rstest]
fn test_quote_tick_python_control_flow() {
    pyo3::prepare_freethreaded_python();

    let file_path = get_nautilus_test_data_file_path("quotes.parquet");

    let expected_length = 9500;
    let catalog = DataBackendSession::new(1_000_000);
    Python::with_gil(|py| {
        let pycatalog: Py<PyAny> = catalog.into_py_any_unwrap(py);
        pycatalog
            .call_method1(
                py,
                "add_file",
                (
                    NautilusDataType::QuoteTick,
                    "order_book_deltas",
                    file_path.as_str(),
                ),
            )
            .unwrap();
        let result = pycatalog.call_method0(py, "to_query_result").unwrap();
        let mut count = 0;
        while let Ok(chunk) = result.call_method0(py, "__next__") {
            let capsule: &Bound<'_, PyCapsule> = chunk.downcast_bound::<PyCapsule>(py).unwrap();
            let cvec: &CVec = unsafe { &*(capsule.pointer() as *const CVec) };
            if cvec.len == 0 {
                break;
            } else {
                let slice: &[Data] =
                    unsafe { std::slice::from_raw_parts(cvec.ptr as *const Data, cvec.len) };
                count += slice.len();
                assert!(is_monotonically_increasing_by_init(slice));
            }
        }

        assert_eq!(expected_length, count);
    });
}

#[ignore] // TODO: Investigate why this is suddenly failing the monotonically increasing assert?
#[rstest]
fn test_order_book_delta_query() {
    let expected_length = 1077;

    let file_path = get_nautilus_test_data_file_path("deltas.parquet");

    let mut catalog = DataBackendSession::new(1_000);
    catalog
        .add_file::<OrderBookDelta>(
            "delta_001",
            file_path.as_str(),
            Some("SELECT * FROM delta_001 ORDER BY ts_init"),
        )
        .unwrap();
    let query_result: QueryResult = catalog.get_query_result();
    let ticks: Vec<Data> = query_result.collect();

    assert_eq!(ticks.len(), expected_length);
    assert!(is_monotonically_increasing_by_init(&ticks));
}

#[rstest]
fn test_order_book_delta_query_py() {
    pyo3::prepare_freethreaded_python();

    let file_path = get_nautilus_test_data_file_path("deltas.parquet");
    let catalog = DataBackendSession::new(2_000);
    Python::with_gil(|py| {
        let pycatalog: Py<PyAny> = catalog.into_py_any_unwrap(py);
        pycatalog
            .call_method1(
                py,
                "add_file",
                (
                    NautilusDataType::OrderBookDelta,
                    "order_book_deltas",
                    file_path.as_str(),
                ),
            )
            .unwrap();
        let result = pycatalog.call_method0(py, "to_query_result").unwrap();
        let chunk = result.call_method0(py, "__next__").unwrap();
        let capsule: &Bound<'_, PyCapsule> = chunk.downcast_bound(py).unwrap();
        let cvec: &CVec = unsafe { &*(capsule.pointer() as *const CVec) };
        assert_eq!(cvec.len, 1077);
    });
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

#[ignore] // TODO: Remove file after asserts
#[rstest]
fn test_catalog_serialization_json_round_trip() {
    // Setup
    // let temp_dir = tempfile::tempdir().unwrap();
    let temp_dir = PathBuf::from(".");
    let catalog = ParquetDataCatalog::new(temp_dir.as_path().to_path_buf(), Some(1000));

    // Read original data from parquet
    let file_path = get_nautilus_test_data_file_path("quotes.parquet");

    let mut session = DataBackendSession::new(1000);
    session
        .add_file::<QuoteTick>("test_data", &file_path, None)
        .unwrap();
    let query_result: QueryResult = session.get_query_result();
    let quote_ticks: Vec<Data> = query_result.collect();
    let quote_ticks: Vec<QuoteTick> = to_variant(quote_ticks);

    // Write to JSON using catalog
    let json_path = catalog
        .write_to_json(quote_ticks.clone(), None, false)
        .unwrap();

    // Read back from JSON
    let json_str = std::fs::read_to_string(json_path).unwrap();
    let loaded_data_variants: Vec<QuoteTick> = serde_json::from_str(&json_str).unwrap();

    // Compare
    assert_eq!(quote_ticks.len(), loaded_data_variants.len());
    for (orig, loaded) in quote_ticks.iter().zip(loaded_data_variants.iter()) {
        assert_eq!(orig, loaded);
    }
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

#[test]
fn test_catalog_export_functionality() {
    // Create a temporary directory for test files
    let temp_dir = tempfile::tempdir().expect("Failed to create temp directory");
    let mut catalog = ParquetDataCatalog::new(temp_dir.path().to_path_buf(), None);

    // Read input file path and determine data type
    let file_path = get_nautilus_test_data_file_path("quotes.parquet");

    let result = catalog
        .query_file::<QuoteTick>(PathBuf::from(file_path), None, None, None)
        .expect("Failed to query file");

    // Extract only the QuoteTick variant from the result stream
    let quotes: Vec<QuoteTick> = result
        .filter_map(|data| {
            if let Data::Quote(quote) = data {
                Some(quote)
            } else {
                None
            }
        })
        .collect();

    // Export to temporary JSON
    let json_file = temp_dir.path().join("temp.json");
    let json_path = catalog
        .write_to_json(quotes.clone(), Some(json_file), false)
        .unwrap();

    // Read JSON file and parse back to Vec<QuoteTick>
    let json_content = std::fs::read_to_string(&json_path).expect("Failed to read JSON file");
    let quotes_from_json: Vec<QuoteTick> =
        serde_json::from_str(&json_content).expect("Failed to parse quotes from JSON");

    // Write back to parquet
    let parquet_path = temp_dir.path().join("temp.parquet");
    let parquet_path = catalog
        .write_to_parquet(quotes_from_json, Some(parquet_path), None, None, None)
        .unwrap();

    // Read parquet and verify data
    let final_result = catalog
        .query_file::<QuoteTick>(parquet_path, None, None, None)
        .expect("Failed to query final file");

    // Extract only the QuoteTick variant from the result stream
    let final_quotes: Vec<QuoteTick> = final_result
        .filter_map(|data| {
            if let Data::Quote(quote) = data {
                Some(quote)
            } else {
                None
            }
        })
        .collect();

    // Compare original and final data
    assert_eq!(quotes.len(), final_quotes.len(), "Quote counts don't match");
    for (original, final_quote) in quotes.iter().zip(final_quotes.iter()) {
        assert_eq!(original, final_quote, "Quotes don't match");
    }
}
