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

//! Provides a generic L1/L2/L3 order book the trading domain model.

pub mod book;
pub mod level;
pub mod own;

#[cfg(test)]
mod tests {
    use std::sync::Once;

    use pyo3::{exceptions::PyValueError, prelude::*, types::PyDict};
    use rstest::rstest;
    use rust_decimal::Decimal;

    use crate::{
        enums::BookType,
        identifiers::InstrumentId,
        orderbook::{OrderBook, own::OwnOrderBook},
    };

    fn ensure_python_initialized() {
        static INIT: Once = Once::new();
        INIT.call_once(Python::initialize);
    }

    #[rstest]
    #[case::own_bids_to_dict("bids_to_dict", false, false)]
    #[case::own_asks_to_dict("asks_to_dict", false, false)]
    #[case::own_bid_quantity("bid_quantity", false, false)]
    #[case::own_ask_quantity("ask_quantity", false, false)]
    #[case::book_bids_filtered_to_dict("bids_filtered_to_dict", true, false)]
    #[case::book_asks_filtered_to_dict("asks_filtered_to_dict", true, false)]
    #[case::book_group_bids_filtered("group_bids_filtered", true, true)]
    #[case::book_group_asks_filtered("group_asks_filtered", true, true)]
    #[case::book_filtered_view("filtered_view", true, false)]
    fn test_python_acceptance_filter_requires_ts_now(
        #[case] method_name: &str,
        #[case] filtered_book_method: bool,
        #[case] requires_group_size: bool,
    ) {
        ensure_python_initialized();

        Python::attach(|py| {
            let instrument_id = InstrumentId::from("AAPL.XNAS");
            let own_book = Py::new(py, OwnOrderBook::new(instrument_id)).unwrap();
            let kwargs = PyDict::new(py);
            kwargs.set_item("accepted_buffer_ns", 1).unwrap();

            let py_err = if filtered_book_method {
                kwargs.set_item("own_book", own_book.bind(py)).unwrap();
                let book = Py::new(py, OrderBook::new(instrument_id, BookType::L2_MBP)).unwrap();
                let result = if requires_group_size {
                    book.bind(py)
                        .call_method(method_name, (Decimal::ONE,), Some(&kwargs))
                } else {
                    book.bind(py).call_method(method_name, (), Some(&kwargs))
                };
                result.unwrap_err()
            } else {
                own_book
                    .bind(py)
                    .call_method(method_name, (), Some(&kwargs))
                    .unwrap_err()
            };

            assert!(
                py_err.is_instance_of::<PyValueError>(py),
                "expected PyValueError, received {}",
                py_err.get_type(py).name().unwrap().to_str().unwrap()
            );
            assert_eq!(
                py_err.value(py).to_string(),
                "ts_now must be provided when accepted_buffer_ns > 0"
            );
        });
    }

    /// The filtered-book methods reject the invalid pair even with no own book.
    ///
    /// Without `own_book` the underlying Rust method never filters own orders, so
    /// it never reaches the assertion and could not panic. The Python boundary is
    /// deliberately stricter, treating the arguments as an invalid pair in their
    /// own right; this pins that decision, which the cases above cannot because
    /// they all supply an own book.
    #[rstest]
    #[case::bids_filtered_to_dict("bids_filtered_to_dict", false)]
    #[case::asks_filtered_to_dict("asks_filtered_to_dict", false)]
    #[case::group_bids_filtered("group_bids_filtered", true)]
    #[case::group_asks_filtered("group_asks_filtered", true)]
    #[case::filtered_view("filtered_view", false)]
    fn test_python_filtered_book_requires_ts_now_without_own_book(
        #[case] method_name: &str,
        #[case] requires_group_size: bool,
    ) {
        ensure_python_initialized();

        Python::attach(|py| {
            let instrument_id = InstrumentId::from("AAPL.XNAS");
            let book = Py::new(py, OrderBook::new(instrument_id, BookType::L2_MBP)).unwrap();
            let kwargs = PyDict::new(py);
            kwargs.set_item("accepted_buffer_ns", 1).unwrap();

            let result = if requires_group_size {
                book.bind(py)
                    .call_method(method_name, (Decimal::ONE,), Some(&kwargs))
            } else {
                book.bind(py).call_method(method_name, (), Some(&kwargs))
            };

            let py_err = result.unwrap_err();

            assert!(
                py_err.is_instance_of::<PyValueError>(py),
                "expected PyValueError, received {}",
                py_err.get_type(py).name().unwrap().to_str().unwrap()
            );
            assert_eq!(
                py_err.value(py).to_string(),
                "ts_now must be provided when accepted_buffer_ns > 0"
            );
        });
    }
}
