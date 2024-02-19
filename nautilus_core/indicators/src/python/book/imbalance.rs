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

use nautilus_core::python::to_pyvalue_err;
use nautilus_model::{
    orderbook::{book_mbo::OrderBookMbo, book_mbp::OrderBookMbp},
    types::quantity::Quantity,
};
use pyo3::prelude::*;

use crate::{book::imbalance::BookImbalanceRatio, indicator::Indicator};

#[pymethods]
impl BookImbalanceRatio {
    #[new]
    fn py_new() -> PyResult<Self> {
        Self::new().map_err(to_pyvalue_err)
    }

    fn __repr__(&self) -> String {
        self.to_string()
    }

    #[getter]
    #[pyo3(name = "name")]
    fn py_name(&self) -> String {
        self.name()
    }

    #[getter]
    #[pyo3(name = "count")]
    fn py_count(&self) -> usize {
        self.count
    }

    #[getter]
    #[pyo3(name = "value")]
    fn py_value(&self) -> f64 {
        self.value
    }

    #[getter]
    #[pyo3(name = "has_inputs")]
    fn py_has_inputs(&self) -> bool {
        self.has_inputs()
    }

    #[getter]
    #[pyo3(name = "initialized")]
    fn py_initialized(&self) -> bool {
        self.initialized
    }

    #[pyo3(name = "handle_book_mbo")]
    fn py_handle_book_mbo(&mut self, book: &OrderBookMbo) {
        self.handle_book_mbo(book);
    }

    #[pyo3(name = "handle_book_mbp")]
    fn py_handle_book_mbp(&mut self, book: &OrderBookMbp) {
        self.handle_book_mbp(book);
    }

    #[pyo3(name = "update")]
    fn py_update(&mut self, best_bid: Option<Quantity>, best_ask: Option<Quantity>) {
        self.update(best_bid, best_ask);
    }

    #[pyo3(name = "reset")]
    fn py_reset(&mut self) {
        self.reset();
    }
}
