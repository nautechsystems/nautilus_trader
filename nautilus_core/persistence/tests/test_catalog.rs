// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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
use nautilus_core::cvec::CVec;
use nautilus_model::data::{delta::OrderBookDelta, quote::QuoteTick, trade::TradeTick, Data};
use nautilus_persistence::{
    arrow::NautilusDataType,
    backend::session::{DataBackendSession, QueryResult},
};
use pyo3::{types::PyCapsule, IntoPy, Py, PyAny, Python};
use rstest::rstest;

#[tokio::test]
async fn test_quote_ticks() {
    let file_path = "../../tests/test_data/quote_tick_data.parquet";
    let length = 9_500;
    let mut catalog = DataBackendSession::new(10_000);
    catalog
        .add_file_default_query::<QuoteTick>("quotes_0005", file_path)
        .await
        .unwrap();
    let query_result: QueryResult = catalog.get_query_result().await;
    let ticks: Vec<Data> = query_result.flatten().collect();

    if let Data::Quote(q) = &ticks[0] {
        assert_eq!("EUR/USD.SIM", q.instrument_id.to_string());
    } else {
        assert!(false);
    }

    assert_eq!(ticks.len(), length);
    assert!(is_ascending_by_init(&ticks));
}

#[tokio::test]
async fn test_data_ticks() {
    let mut catalog = DataBackendSession::new(5_000);
    catalog
        .add_file_default_query::<QuoteTick>(
            "quote_tick",
            "../../tests/test_data/quote_tick_data.parquet",
        )
        .await
        .unwrap();
    catalog
        .add_file_default_query::<TradeTick>(
            "quote_tick_2",
            "../../tests/test_data/trade_tick_data.parquet",
        )
        .await
        .unwrap();
    let query_result: QueryResult = catalog.get_query_result().await;
    let ticks: Vec<Data> = query_result.flatten().collect();

    assert_eq!(ticks.len(), 9600);
    assert!(is_ascending_by_init(&ticks));
}

#[tokio::test]
async fn test_order_book_delta() {
    let file_path = "../../tests/test_data/order_book_deltas.parquet";
    let mut catalog = DataBackendSession::new(1000);
    catalog
        .add_file_default_query::<OrderBookDelta>("order_book_delta", file_path)
        .await
        .unwrap();
    let query_result: QueryResult = catalog.get_query_result().await;
    let ticks: Vec<Data> = query_result.flatten().collect();

    assert_eq!(ticks.len(), 1077);
    assert!(is_ascending_by_init(&ticks));
}

#[rstest]
fn test_order_book_delta_py() {
    pyo3::prepare_freethreaded_python();

    let file_path = "../../tests/test_data/order_book_deltas.parquet";
    let catalog = DataBackendSession::new(2000);
    Python::with_gil(|py| {
        let pycatalog: Py<PyAny> = catalog.into_py(py);
        pycatalog
            .call_method1(
                py,
                "add_file",
                (
                    "order_book_deltas",
                    file_path,
                    NautilusDataType::OrderBookDelta,
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

// NOTE: is_sorted_by_key is unstable otherwise use
// ticks.is_sorted_by_key(|tick| tick.ts_init)
// https://github.com/rust-lang/rust/issues/53485
fn is_ascending_by_init(ticks: &Vec<Data>) -> bool {
    for i in 1..ticks.len() {
        // previous tick is more recent than current tick
        // this is not ascending order
        if ticks[i - 1].get_ts_init() > ticks[i].get_ts_init() {
            return false;
        }
    }
    true
}
