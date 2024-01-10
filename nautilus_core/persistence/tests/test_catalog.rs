// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

use nautilus_core::ffi::cvec::CVec;
use nautilus_model::data::{
    bar::Bar, delta::OrderBookDelta, is_monotonically_increasing_by_init, quote::QuoteTick,
    trade::TradeTick, Data,
};
use nautilus_persistence::{
    backend::session::{DataBackendSession, DataQueryResult, QueryResult},
    python::backend::session::NautilusDataType,
};
use pyo3::{types::PyCapsule, IntoPy, Py, PyAny, Python};
use rstest::rstest;

#[rstest]
fn test_quote_tick_python_interface() {
    let file_path = "../../tests/test_data/nautilus/quotes.parquet";
    let expected_length = 9500;
    for i in 0..1000_00 {
        let mut catalog = DataBackendSession::new(1000);
        catalog
            .add_file::<QuoteTick>("quote_005", file_path, None)
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
        }

        assert_eq!(expected_length, count);
        if i % 1000 == 0 {
            println!("iteration: {}", i);
        }
    }
}

#[rstest]
fn test_quote_tick_python_control_flow() {
    pyo3::prepare_freethreaded_python();

    let file_path = "../../tests/test_data/nautilus/quotes.parquet";
    let expected_length = 9500;
    let catalog = DataBackendSession::new(1_000_000);
    Python::with_gil(|py| {
        let pycatalog: Py<PyAny> = catalog.into_py(py);
        pycatalog
            .call_method1(
                py,
                "add_file",
                (NautilusDataType::QuoteTick, "order_book_deltas", file_path),
            )
            .unwrap();
        let result = pycatalog.call_method0(py, "to_query_result").unwrap();
        let mut count = 0;
        while let Ok(chunk) = result.call_method0(py, "__next__") {
            let capsule: &PyCapsule = chunk.downcast(py).unwrap();
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
    let file_path = "../../tests/test_data/nautilus/deltas.parquet";
    let mut catalog = DataBackendSession::new(1_000);
    catalog
        .add_file::<OrderBookDelta>(
            "delta_001",
            file_path,
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

    let file_path = "../../tests/test_data/nautilus/deltas.parquet";
    let catalog = DataBackendSession::new(2_000);
    Python::with_gil(|py| {
        let pycatalog: Py<PyAny> = catalog.into_py(py);
        pycatalog
            .call_method1(
                py,
                "add_file",
                (
                    NautilusDataType::OrderBookDelta,
                    "order_book_deltas",
                    file_path,
                ),
            )
            .unwrap();
        let result = pycatalog.call_method0(py, "to_query_result").unwrap();
        let chunk = result.call_method0(py, "__next__").unwrap();
        let capsule: &PyCapsule = chunk.downcast(py).unwrap();
        let cvec: &CVec = unsafe { &*(capsule.pointer() as *const CVec) };
        assert_eq!(cvec.len, 1077);
    });
}

#[rstest]
fn test_quote_tick_query() {
    let expected_length = 9_500;
    let file_path = "../../tests/test_data/nautilus/quotes.parquet";
    let mut catalog = DataBackendSession::new(10_000);
    catalog
        .add_file::<QuoteTick>("quote_005", file_path, None)
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
fn test_quote_tick_multiple_query() {
    let expected_length = 9_600;
    let mut catalog = DataBackendSession::new(5_000);
    catalog
        .add_file::<QuoteTick>(
            "quote_tick",
            "../../tests/test_data/nautilus/quotes.parquet",
            None,
        )
        .unwrap();
    catalog
        .add_file::<TradeTick>(
            "quote_tick_2",
            "../../tests/test_data/nautilus/trades.parquet",
            None,
        )
        .unwrap();
    let query_result: QueryResult = catalog.get_query_result();
    let ticks: Vec<Data> = query_result.collect();

    assert_eq!(ticks.len(), expected_length);
    assert!(is_monotonically_increasing_by_init(&ticks));
}

#[rstest]
fn test_trade_tick_query() {
    let expected_length = 100;
    let file_path = "../../tests/test_data/nautilus/trades.parquet";
    let mut catalog = DataBackendSession::new(10_000);
    catalog
        .add_file::<TradeTick>("trade_001", file_path, None)
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
    let file_path = "../../tests/test_data/nautilus/bars.parquet";
    let mut catalog = DataBackendSession::new(10_000);
    catalog.add_file::<Bar>("bar_001", file_path, None).unwrap();
    let query_result: QueryResult = catalog.get_query_result();
    let ticks: Vec<Data> = query_result.collect();

    if let Data::Bar(b) = &ticks[0] {
        assert_eq!("ADABTC.BINANCE", b.bar_type.instrument_id.to_string());
    } else {
        panic!("Invalid test");
    }

    assert_eq!(ticks.len(), expected_length);
    assert!(is_monotonically_increasing_by_init(&ticks));
}
