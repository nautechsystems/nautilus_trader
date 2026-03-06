use nautilus_model::{orderbook::OrderBook, types::Quantity};
use pyo3::prelude::*;

use crate::{book::imbalance::BookImbalanceRatio, indicator::Indicator};

#[pymethods]
impl BookImbalanceRatio {
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
    fn py_handle_book(&mut self, book: &OrderBook) {
        self.handle_book(book);
    }

    #[pyo3(name = "update")]
    #[pyo3(signature = (best_bid=None, best_ask=None))]
    fn py_update(&mut self, best_bid: Option<Quantity>, best_ask: Option<Quantity>) {
        self.update(best_bid, best_ask);
    }

    #[pyo3(name = "reset")]
    fn py_reset(&mut self) {
        self.reset();
    }
}
