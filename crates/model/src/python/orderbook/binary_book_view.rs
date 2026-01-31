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

//! Python bindings for [`BinaryMarketBookView`].

use pyo3::prelude::*;

use crate::{
    enums::OrderStatus,
    orderbook::{BinaryMarketBookView, OrderBook, own::OwnOrderBook},
};

#[pymethods]
impl BinaryMarketBookView {
    #[new]
    #[pyo3(signature = (book, own_book, own_synthetic_book, depth=None, status=None, accepted_buffer_ns=None, now=None))]
    fn py_new(
        book: OrderBook,
        own_book: OwnOrderBook,
        own_synthetic_book: OwnOrderBook,
        depth: Option<usize>,
        status: Option<std::collections::HashSet<OrderStatus>>,
        accepted_buffer_ns: Option<u64>,
        now: Option<u64>,
    ) -> Self {
        Self::new(
            book,
            own_book,
            own_synthetic_book,
            depth,
            status.map(|s| s.into_iter().collect()),
            accepted_buffer_ns,
            now,
        )
    }

    #[getter]
    fn book(&self) -> OrderBook {
        self.book.clone()
    }
}
