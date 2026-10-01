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

use nautilus_model::{orderbook::OrderBook, types::Quantity};
use pyo3::prelude::*;

use crate::{book::imbalance::BookImbalanceRatio, indicator::Indicator, python::float_precision};

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl BookImbalanceRatio {
    /// Creates a new `BookImbalanceRatio` instance.
    #[new]
    const fn py_new() -> Self {
        Self::new()
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
    const fn py_count(&self) -> usize {
        self.count
    }

    #[getter]
    #[pyo3(name = "value")]
    const fn py_value(&self) -> f64 {
        self.value
    }

    #[getter]
    #[pyo3(name = "has_inputs")]
    fn py_has_inputs(&self) -> bool {
        self.has_inputs()
    }

    #[getter]
    #[pyo3(name = "initialized")]
    const fn py_initialized(&self) -> bool {
        self.initialized
    }

    #[pyo3(name = "handle_book")]
    fn py_handle_book(&mut self, book: &OrderBook) -> PyResult<()> {
        check_sizes(book.best_bid_size(), book.best_ask_size())?;
        self.handle_book(book);
        Ok(())
    }

    #[pyo3(name = "update")]
    #[pyo3(signature = (best_bid=None, best_ask=None))]
    fn py_update(
        &mut self,
        best_bid: Option<Quantity>,
        best_ask: Option<Quantity>,
    ) -> PyResult<()> {
        check_sizes(best_bid, best_ask)?;
        self.update(best_bid, best_ask);
        Ok(())
    }

    #[pyo3(name = "reset")]
    fn py_reset(&mut self) {
        self.reset();
    }
}

fn check_sizes(best_bid: Option<Quantity>, best_ask: Option<Quantity>) -> PyResult<()> {
    // The ratio only converts sizes once both sides are present
    if let (Some(best_bid), Some(best_ask)) = (best_bid, best_ask) {
        float_precision::check(best_bid.precision)?;
        float_precision::check(best_ask.precision)?;
    }

    Ok(())
}
